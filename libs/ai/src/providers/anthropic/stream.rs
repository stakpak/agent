//! Anthropic streaming support
//!
//! Key behaviors:
//! - Track content blocks by index to associate tool call IDs with their deltas
//! - Anthropic sends tool call ID in `content_block_start` but not in `content_block_delta`
//! - Accumulate tool call input JSON and emit `ToolCallEnd` at `content_block_stop`

use super::types::{AnthropicContent, AnthropicStreamEvent};
use crate::error::{Error, Result};
use crate::types::{
    FinishReason, FinishReasonKind, GenerateStream, InputTokenDetails, StreamEvent, Usage,
};
use futures::stream::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use std::error::Error as StdError;

/// Track state for each content block during streaming
#[derive(Debug, Clone)]
enum ContentBlock {
    Text,
    Reasoning,
    ToolCall {
        id: String,
        name: String,
        input: String,
    },
}

/// Create a stream from Anthropic EventSource
pub async fn create_stream(mut event_source: EventSource) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut accumulated_usage = Usage::default();
        // Track content blocks by index - stores both ID and accumulated input
        let mut content_blocks: std::collections::HashMap<u32, ContentBlock> = std::collections::HashMap::new();

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
                            for stream_event in process_anthropic_event(event, &mut accumulated_usage, &mut content_blocks) {
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
                Err(reqwest_eventsource::Error::Transport(e)) => {
                    yield Err(Error::stream_error(format!(
                        "Transport error: {} | source: {:?}",
                        e,
                        e.source()
                    )));
                    break;
                }
                Err(reqwest_eventsource::Error::Utf8(e)) => {
                    yield Err(Error::stream_error(format!(
                        "UTF-8 decode error in stream: {}",
                        e
                    )));
                    break;
                }
                Err(reqwest_eventsource::Error::Parser(e)) => {
                    yield Err(Error::stream_error(format!(
                        "SSE parser error: {}",
                        e
                    )));
                    break;
                }
                Err(reqwest_eventsource::Error::InvalidContentType(content_type, _)) => {
                    yield Err(Error::stream_error(format!(
                        "Invalid content type from server: {:?} (expected text/event-stream)",
                        content_type
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

/// Process Anthropic stream event and convert to unified StreamEvent(s)
/// Returns a Vec because content_block_stop can emit ToolCallEnd
fn process_anthropic_event(
    event: AnthropicStreamEvent,
    accumulated_usage: &mut Usage,
    content_blocks: &mut std::collections::HashMap<u32, ContentBlock>,
) -> Vec<StreamEvent> {
    match event.type_.as_str() {
        "message_start" => {
            // Message started - extract usage info including cache details
            if let Some(message) = event.message {
                let usage = &message.usage;
                let cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
                let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                let input_tokens = usage.input_tokens;
                let total_input = input_tokens + cache_write + cache_read;

                accumulated_usage.prompt_tokens = total_input;
                accumulated_usage.input_token_details = Some(InputTokenDetails {
                    total: Some(total_input),
                    no_cache: Some(input_tokens),
                    cache_read: (cache_read > 0).then_some(cache_read),
                    cache_write: (cache_write > 0).then_some(cache_write),
                });
            }
            Vec::new()
        }
        "content_block_start" => {
            let index = event.index.unwrap_or(0);

            match event.content_block {
                Some(AnthropicContent::ToolUse { id, name, .. }) => {
                    // Store the tool call block for tracking
                    content_blocks.insert(
                        index,
                        ContentBlock::ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input: String::new(),
                        },
                    );
                    vec![StreamEvent::tool_call_start(id, name)]
                }
                Some(AnthropicContent::Text { .. }) => {
                    content_blocks.insert(index, ContentBlock::Text);
                    Vec::new()
                }
                Some(AnthropicContent::Thinking { .. })
                | Some(AnthropicContent::RedactedThinking { .. }) => {
                    content_blocks.insert(index, ContentBlock::Reasoning);
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
        "content_block_delta" => {
            let index = event.index.unwrap_or(0);

            if let Some(delta) = event.delta {
                match delta.type_.as_deref() {
                    Some("text_delta") => {
                        if let Some(text) = delta.text {
                            return vec![StreamEvent::text_delta("", text)];
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(thinking) = delta.thinking {
                            return vec![StreamEvent::reasoning_delta("", thinking)];
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(partial_json) = delta.partial_json {
                            // Look up the content block and accumulate input
                            if let Some(ContentBlock::ToolCall { id, input, .. }) =
                                content_blocks.get_mut(&index)
                            {
                                input.push_str(&partial_json);
                                return vec![StreamEvent::tool_call_delta(
                                    id.clone(),
                                    partial_json,
                                )];
                            }
                            // Fallback if block not found (shouldn't happen)
                            return vec![StreamEvent::tool_call_delta(
                                index.to_string(),
                                partial_json,
                            )];
                        }
                    }
                    _ => {}
                }
            }
            Vec::new()
        }
        "content_block_stop" => {
            // Content block finished - emit ToolCallEnd for tool calls
            let index = event.index.unwrap_or(0);

            if let Some(block) = content_blocks.remove(&index)
                && let ContentBlock::ToolCall { id, name, input } = block
            {
                // Parse the accumulated JSON input
                let input_json = if input.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_str(&input).unwrap_or(serde_json::json!({}))
                };
                return vec![StreamEvent::tool_call_end(id, name, input_json)];
            }
            Vec::new()
        }
        "message_delta" => {
            // Message delta - could have usage updates
            if let Some(usage) = event.usage {
                accumulated_usage.completion_tokens = usage.output_tokens;
                accumulated_usage.total_tokens =
                    accumulated_usage.prompt_tokens + usage.output_tokens;
            }
            Vec::new()
        }
        "message_stop" => {
            // Message finished - emit final usage
            vec![StreamEvent::finish(
                accumulated_usage.clone(),
                FinishReason::with_raw(FinishReasonKind::Stop, "message_stop"),
            )]
        }
        "error" => {
            // Error event
            let message = event
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Anthropic API error".to_string());
            vec![StreamEvent::error(message)]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::providers::anthropic::types::AnthropicDelta;

    use super::*;

    #[test]
    fn test_process_text_delta() {
        let mut usage = Usage::default();
        let mut content_blocks = std::collections::HashMap::new();

        // First start a text block
        let start_event = AnthropicStreamEvent {
            type_: "content_block_start".to_string(),
            message: None,
            index: Some(0),
            content_block: Some(AnthropicContent::Text {
                text: String::new(),
                cache_control: None,
            }),
            delta: None,
            usage: None,
            error: None,
        };
        process_anthropic_event(start_event, &mut usage, &mut content_blocks);

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

        let results = process_anthropic_event(event, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);

        if let StreamEvent::TextDelta { delta, .. } = &results[0] {
            assert_eq!(delta, "Hello");
        } else {
            panic!("Expected TextDelta event");
        }
    }

    #[test]
    fn test_tool_call_complete_flow() {
        let mut usage = Usage::default();
        let mut content_blocks = std::collections::HashMap::new();

        // 1. content_block_start for tool use
        let start_event = AnthropicStreamEvent {
            type_: "content_block_start".to_string(),
            message: None,
            index: Some(1),
            content_block: Some(AnthropicContent::ToolUse {
                id: "toolu_01ABC123".to_string(),
                name: "get_weather".to_string(),
                input: serde_json::json!({}),
                cache_control: None,
            }),
            delta: None,
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(start_event, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);
        if let StreamEvent::ToolCallStart { id, name } = &results[0] {
            assert_eq!(id, "toolu_01ABC123");
            assert_eq!(name, "get_weather");
        } else {
            panic!("Expected ToolCallStart event");
        }

        // 2. content_block_delta with partial JSON
        let delta_event1 = AnthropicStreamEvent {
            type_: "content_block_delta".to_string(),
            message: None,
            index: Some(1),
            content_block: None,
            delta: Some(AnthropicDelta {
                type_: Some("input_json_delta".to_string()),
                text: None,
                thinking: None,
                _signature: None,
                partial_json: Some(r#"{"location":"#.to_string()),
                _stop_reason: None,
                _stop_sequence: None,
            }),
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(delta_event1, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);
        if let StreamEvent::ToolCallDelta { id, delta } = &results[0] {
            assert_eq!(id, "toolu_01ABC123");
            assert_eq!(delta, r#"{"location":"#);
        } else {
            panic!("Expected ToolCallDelta event");
        }

        // 3. More content_block_delta
        let delta_event2 = AnthropicStreamEvent {
            type_: "content_block_delta".to_string(),
            message: None,
            index: Some(1),
            content_block: None,
            delta: Some(AnthropicDelta {
                type_: Some("input_json_delta".to_string()),
                text: None,
                thinking: None,
                _signature: None,
                partial_json: Some(r#""San Francisco"}"#.to_string()),
                _stop_reason: None,
                _stop_sequence: None,
            }),
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(delta_event2, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);

        // 4. content_block_stop - should emit ToolCallEnd with complete JSON
        let stop_event = AnthropicStreamEvent {
            type_: "content_block_stop".to_string(),
            message: None,
            index: Some(1),
            content_block: None,
            delta: None,
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(stop_event, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);
        if let StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            ..
        } = &results[0]
        {
            assert_eq!(id, "toolu_01ABC123");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments["location"], "San Francisco");
        } else {
            panic!("Expected ToolCallEnd event, got {:?}", results[0]);
        }
    }

    #[test]
    fn test_multiple_tool_calls_in_same_message() {
        let mut usage = Usage::default();
        let mut content_blocks = std::collections::HashMap::new();

        // First tool call at index 0
        let event1 = AnthropicStreamEvent {
            type_: "content_block_start".to_string(),
            message: None,
            index: Some(0),
            content_block: Some(AnthropicContent::ToolUse {
                id: "toolu_first".to_string(),
                name: "get_weather".to_string(),
                input: serde_json::json!({}),
                cache_control: None,
            }),
            delta: None,
            usage: None,
            error: None,
        };
        process_anthropic_event(event1, &mut usage, &mut content_blocks);

        // Second tool call at index 1
        let event2 = AnthropicStreamEvent {
            type_: "content_block_start".to_string(),
            message: None,
            index: Some(1),
            content_block: Some(AnthropicContent::ToolUse {
                id: "toolu_second".to_string(),
                name: "get_time".to_string(),
                input: serde_json::json!({}),
                cache_control: None,
            }),
            delta: None,
            usage: None,
            error: None,
        };
        process_anthropic_event(event2, &mut usage, &mut content_blocks);

        // Delta for first tool call
        let delta1 = AnthropicStreamEvent {
            type_: "content_block_delta".to_string(),
            message: None,
            index: Some(0),
            content_block: None,
            delta: Some(AnthropicDelta {
                type_: Some("input_json_delta".to_string()),
                text: None,
                thinking: None,
                _signature: None,
                partial_json: Some(r#"{"city":"NYC"}"#.to_string()),
                _stop_reason: None,
                _stop_sequence: None,
            }),
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(delta1, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);
        if let StreamEvent::ToolCallDelta { id, .. } = &results[0] {
            assert_eq!(id, "toolu_first");
        } else {
            panic!("Expected ToolCallDelta for first tool");
        }

        // Delta for second tool call
        let delta2 = AnthropicStreamEvent {
            type_: "content_block_delta".to_string(),
            message: None,
            index: Some(1),
            content_block: None,
            delta: Some(AnthropicDelta {
                type_: Some("input_json_delta".to_string()),
                text: None,
                thinking: None,
                _signature: None,
                partial_json: Some(r#"{"timezone":"EST"}"#.to_string()),
                _stop_reason: None,
                _stop_sequence: None,
            }),
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(delta2, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);
        if let StreamEvent::ToolCallDelta { id, .. } = &results[0] {
            assert_eq!(id, "toolu_second");
        } else {
            panic!("Expected ToolCallDelta for second tool");
        }

        // Stop first tool call
        let stop1 = AnthropicStreamEvent {
            type_: "content_block_stop".to_string(),
            message: None,
            index: Some(0),
            content_block: None,
            delta: None,
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(stop1, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);
        if let StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            ..
        } = &results[0]
        {
            assert_eq!(id, "toolu_first");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments["city"], "NYC");
        } else {
            panic!("Expected ToolCallEnd for first tool");
        }

        // Stop second tool call
        let stop2 = AnthropicStreamEvent {
            type_: "content_block_stop".to_string(),
            message: None,
            index: Some(1),
            content_block: None,
            delta: None,
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(stop2, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);
        if let StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            ..
        } = &results[0]
        {
            assert_eq!(id, "toolu_second");
            assert_eq!(name, "get_time");
            assert_eq!(arguments["timezone"], "EST");
        } else {
            panic!("Expected ToolCallEnd for second tool");
        }
    }

    #[test]
    fn test_thinking_delta() {
        let mut usage = Usage::default();
        let mut content_blocks = std::collections::HashMap::new();

        let event = AnthropicStreamEvent {
            type_: "content_block_delta".to_string(),
            message: None,
            index: Some(0),
            content_block: None,
            delta: Some(AnthropicDelta {
                type_: Some("thinking_delta".to_string()),
                text: None,
                thinking: Some("Let me think about this...".to_string()),
                _signature: None,
                partial_json: None,
                _stop_reason: None,
                _stop_sequence: None,
            }),
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(event, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);

        if let StreamEvent::ReasoningDelta { delta, .. } = &results[0] {
            assert_eq!(delta, "Let me think about this...");
        } else {
            panic!("Expected ReasoningDelta event");
        }
    }

    #[test]
    fn test_message_stop_emits_finish() {
        let mut usage = Usage::new(10, 20);
        let mut content_blocks = std::collections::HashMap::new();

        let event = AnthropicStreamEvent {
            type_: "message_stop".to_string(),
            message: None,
            index: None,
            content_block: None,
            delta: None,
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(event, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);

        if let StreamEvent::Finish { usage: u, reason } = &results[0] {
            assert_eq!(u.prompt_tokens, 10);
            assert_eq!(u.completion_tokens, 20);
            assert_eq!(u.total_tokens, 30);
            assert!(matches!(
                reason.unified,
                crate::types::FinishReasonKind::Stop
            ));
        } else {
            panic!("Expected Finish event");
        }
    }

    #[test]
    fn test_error_event() {
        let mut usage = Usage::default();
        let mut content_blocks = std::collections::HashMap::new();

        let event = AnthropicStreamEvent {
            type_: "error".to_string(),
            message: None,
            index: None,
            content_block: None,
            delta: None,
            usage: None,
            error: Some(crate::providers::anthropic::types::AnthropicError {
                message: "Rate limit exceeded".to_string(),
            }),
        };

        let results = process_anthropic_event(event, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);

        if let StreamEvent::Error { message } = &results[0] {
            assert_eq!(message, "Rate limit exceeded");
        } else {
            panic!("Expected Error event");
        }
    }

    #[test]
    fn test_tool_call_with_empty_input() {
        let mut usage = Usage::default();
        let mut content_blocks = std::collections::HashMap::new();

        // Start tool call
        let start_event = AnthropicStreamEvent {
            type_: "content_block_start".to_string(),
            message: None,
            index: Some(0),
            content_block: Some(AnthropicContent::ToolUse {
                id: "toolu_empty".to_string(),
                name: "no_args_tool".to_string(),
                input: serde_json::json!({}),
                cache_control: None,
            }),
            delta: None,
            usage: None,
            error: None,
        };
        process_anthropic_event(start_event, &mut usage, &mut content_blocks);

        // Stop immediately without any deltas
        let stop_event = AnthropicStreamEvent {
            type_: "content_block_stop".to_string(),
            message: None,
            index: Some(0),
            content_block: None,
            delta: None,
            usage: None,
            error: None,
        };

        let results = process_anthropic_event(stop_event, &mut usage, &mut content_blocks);
        assert_eq!(results.len(), 1);

        if let StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            ..
        } = &results[0]
        {
            assert_eq!(id, "toolu_empty");
            assert_eq!(name, "no_args_tool");
            assert_eq!(arguments, &serde_json::json!({}));
        } else {
            panic!("Expected ToolCallEnd event");
        }
    }
}
