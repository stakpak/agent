//! Anthropic streaming support

use super::types::{AnthropicContent, AnthropicStreamEvent};
use crate::error::{Error, Result};
use crate::types::{GenerateStream, StreamEvent, Usage};
use futures::stream::StreamExt;
use reqwest_eventsource::{Event, EventSource};

/// Create a stream from Anthropic EventSource
pub async fn create_stream(mut event_source: EventSource) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut accumulated_usage = Usage::default();

        while let Some(event) = event_source.next().await {
            match event {
                Ok(Event::Open) => {
                    // Connection opened
                    continue;
                }
                Ok(Event::Message(message)) => {
                    // Parse the event
                    if message.data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<AnthropicStreamEvent>(&message.data) {
                        Ok(event) => {
                            if let Some(stream_event) = process_anthropic_event(event, &mut accumulated_usage) {
                                yield Ok(stream_event);
                            }
                        }
                        Err(e) => {
                            yield Err(Error::stream_error(format!("Failed to parse event: {}", e)));
                            break;
                        }
                    }
                }
                Err(reqwest_eventsource::Error::StreamEnded) => {
                    // Stream ended normally without message_stop - this is OK
                    break;
                }
                Err(reqwest_eventsource::Error::InvalidStatusCode(status, response)) => {
                    // HTTP error - try to get error body for better message
                    let error_body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unable to read error body".to_string());
                    yield Err(Error::provider_error(format!(
                        "Anthropic API error {}: {}",
                        status, error_body
                    )));
                    break;
                }
                Err(e) => {
                    yield Err(Error::stream_error(format!("Stream error: {}", e)));
                    break;
                }
            }
        }

        event_source.close();
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Process Anthropic stream event and convert to unified StreamEvent
fn process_anthropic_event(
    event: AnthropicStreamEvent,
    accumulated_usage: &mut Usage,
) -> Option<StreamEvent> {
    match event.type_.as_str() {
        "message_start" => {
            // Message started - could extract usage info
            if let Some(message) = event.message {
                accumulated_usage.prompt_tokens = message.usage.input_tokens;
            }
            None // Don't emit event for message start
        }
        "content_block_start" => {
            // Content block started - check if it's a tool use
            if let Some(AnthropicContent::ToolUse { id, name, .. }) = event.content_block {
                return Some(StreamEvent::tool_call_start(id, name));
            }
            None
        }
        "content_block_delta" => {
            // Content delta - this is where we get text chunks or tool input
            if let Some(delta) = event.delta {
                match delta.type_.as_deref() {
                    Some("text_delta") => {
                        if let Some(text) = delta.text {
                            return Some(StreamEvent::text_delta("", text));
                        }
                    }
                    Some("thinking_delta") => {
                        // Use dedicated ReasoningDelta event for proper handling
                        if let Some(thinking) = delta.thinking {
                            return Some(StreamEvent::reasoning_delta("", thinking));
                        }
                    }
                    Some("input_json_delta") => {
                        // Tool call arguments delta
                        if let Some(partial_json) = delta.partial_json {
                            let index = event.index.unwrap_or(0);
                            return Some(StreamEvent::tool_call_delta(
                                index.to_string(),
                                partial_json,
                            ));
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        "content_block_stop" => {
            // Content block finished
            None
        }
        "message_delta" => {
            // Message delta - could have usage updates
            if let Some(usage) = event.usage {
                accumulated_usage.completion_tokens = usage.output_tokens;
                accumulated_usage.total_tokens =
                    accumulated_usage.prompt_tokens + usage.output_tokens;
            }
            None
        }
        "message_stop" => {
            // Message finished - emit final usage
            Some(StreamEvent::finish(
                accumulated_usage.clone(),
                crate::types::FinishReason::Stop,
            ))
        }
        "error" => {
            // Error event
            let message = event
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Anthropic API error".to_string());
            Some(StreamEvent::error(message))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::providers::anthropic::types::AnthropicDelta;

    use super::*;

    #[test]
    fn test_process_text_delta() {
        let mut usage = Usage::default();
        let event = AnthropicStreamEvent {
            type_: "content_block_delta".to_string(),
            message: None,
            index: Some(0),
            content_block: None,
            delta: Some(AnthropicDelta {
                type_: Some("text_delta".to_string()),
                text: Some("Hello".to_string()),
                thinking: None,
                _signature: None,
                partial_json: None,
                _stop_reason: None,
                _stop_sequence: None,
            }),
            usage: None,
            error: None,
        };

        let result = process_anthropic_event(event, &mut usage);
        assert!(result.is_some());

        if let Some(StreamEvent::TextDelta { delta, .. }) = result {
            assert_eq!(delta, "Hello");
        }
    }
}
