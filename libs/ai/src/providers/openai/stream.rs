//! OpenAI streaming implementation

use super::types::ChatCompletionChunk;
use crate::error::{Error, Result};
use crate::types::{FinishReason, GenerateStream, StreamEvent, Usage};
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};

/// Create a streaming response from OpenAI
pub async fn create_stream(event_source: EventSource) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut event_stream = event_source;
        let mut accumulated_usage: Option<Usage> = None;

        while let Some(event) = event_stream.next().await {
            match event {
                Ok(Event::Open) => {
                    // Connection opened
                }
                Ok(Event::Message(message)) => {
                    if message.data == "[DONE]" {
                        break;
                    }

                    match parse_chunk(&message.data, &mut accumulated_usage) {
                        Ok(Some(event)) => yield Ok(event),
                        Ok(None) => continue,
                        Err(e) => yield Err(e),
                    }
                }
                Err(e) => {
                    yield Err(Error::stream_error(format!("Stream error: {}", e)));
                    break;
                }
            }
        }
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Parse a streaming chunk from OpenAI
fn parse_chunk(data: &str, accumulated_usage: &mut Option<Usage>) -> Result<Option<StreamEvent>> {
    let chunk: ChatCompletionChunk = serde_json::from_str(data)
        .map_err(|e| Error::invalid_response(format!("Failed to parse chunk: {}", e)))?;

    // Capture usage if present (OpenAI sends this in the final chunk when stream_options.include_usage is true)
    if let Some(chat_usage) = chunk.usage {
        *accumulated_usage = Some(Usage {
            prompt_tokens: chat_usage.prompt_tokens,
            completion_tokens: chat_usage.completion_tokens,
            total_tokens: chat_usage.total_tokens,
        });
    }

    let choice = match chunk.choices.first() {
        Some(c) => c,
        None => return Ok(None),
    };

    // Handle finish reason
    if let Some(reason) = &choice.finish_reason {
        let finish_reason = match reason.as_str() {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            "content_filter" => FinishReason::ContentFilter,
            "tool_calls" => FinishReason::ToolCalls,
            _ => FinishReason::Other,
        };

        return Ok(Some(StreamEvent::finish(
            accumulated_usage.clone().unwrap_or_default(),
            finish_reason,
        )));
    }

    // Handle tool calls
    if let Some(tool_calls) = &choice.delta.tool_calls {
        for tc in tool_calls {
            if let Some(function) = &tc.function {
                // Tool call started (has name)
                if let Some(name) = &function.name {
                    return Ok(Some(StreamEvent::tool_call_start(
                        tc.id.clone().unwrap_or_default(),
                        name,
                    )));
                }
                // Tool call arguments delta
                if let Some(args) = &function.arguments {
                    return Ok(Some(StreamEvent::tool_call_delta(
                        tc.id.clone().unwrap_or_default(),
                        args,
                    )));
                }
            }
        }
    }

    // Handle content delta
    if let Some(content) = &choice.delta.content {
        return Ok(Some(StreamEvent::text_delta(chunk.id, content)));
    }

    // Start event (role present but no content)
    if choice.delta.role.is_some() {
        return Ok(Some(StreamEvent::start(chunk.id)));
    }

    Ok(None)
}
