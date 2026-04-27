//! MiniMax streaming implementation
//!
//! Reuses OpenAI-compatible SSE streaming format.

use crate::error::{Error, Result};
use crate::providers::openai::types::{ChatCompletionChunk};
use crate::types::{
    FinishReason, FinishReasonKind, GenerateStream,
    StreamEvent, Usage,
};
use super::convert::usage_from_chat_usage;
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use std::error::Error as StdError;

/// Track state for each tool call during streaming
#[derive(Debug, Clone)]
struct ToolCallState {
    id: String,
    name: String,
    arguments: String,
}

/// Create a streaming response from MiniMax
pub async fn create_stream(event_source: EventSource) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut event_stream = event_source;
        let mut accumulated_usage: Option<Usage> = None;
        let mut tool_calls: std::collections::HashMap<u32, ToolCallState> = std::collections::HashMap::new();

        while let Some(event) = event_stream.next().await {
            match event {
                Ok(Event::Open) => {}
                Ok(Event::Message(message)) => {
                    if message.data == "[DONE]" {
                        break;
                    }

                    match parse_chunk(&message.data, &mut accumulated_usage, &mut tool_calls) {
                        Ok(events) => {
                            for event in events {
                                yield Ok(event);
                            }
                        }
                        Err(e) => yield Err(e),
                    }
                }
                Err(reqwest_eventsource::Error::StreamEnded) => {
                    break;
                }
                Err(reqwest_eventsource::Error::InvalidStatusCode(status, response)) => {
                    let error_body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unable to read error body".to_string());
                    let friendly_error = super::convert::parse_minimax_error(&error_body, status.as_u16());
                    yield Err(Error::provider_error(friendly_error));
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
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Parse a streaming chunk from MiniMax (OpenAI-compatible format)
fn parse_chunk(
    data: &str,
    accumulated_usage: &mut Option<Usage>,
    tool_calls: &mut std::collections::HashMap<u32, ToolCallState>,
) -> Result<Vec<StreamEvent>> {
    let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
        Ok(c) => c,
        Err(_) => {
            return Err(Error::from_unparseable_chunk(
                data,
                "Failed to parse MiniMax chunk",
            ));
        }
    };

    // Capture usage
    if let Some(usage) = &chunk.usage {
        *accumulated_usage = Some(usage_from_chat_usage(usage));
    }

    let choice = match chunk.choices.first() {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };

    let mut events = Vec::new();

    // Handle tool calls
    if let Some(tc_deltas) = &choice.delta.tool_calls {
        for tc in tc_deltas {
            let tool_call = tool_calls.entry(tc.index).or_insert_with(|| ToolCallState {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });

            if let Some(id) = &tc.id
                && !id.is_empty()
            {
                tool_call.id = id.clone();
            }

            if let Some(function) = &tc.function {
                if let Some(name) = &function.name {
                    tool_call.name = name.clone();
                    events.push(StreamEvent::tool_call_start(
                        tool_call.id.clone(),
                        name.clone(),
                    ));
                }

                if let Some(args) = &function.arguments {
                    tool_call.arguments.push_str(args);
                    events.push(StreamEvent::tool_call_delta(
                        tool_call.id.clone(),
                        args.clone(),
                    ));
                }
            }
        }
    }

    // Handle finish reason
    if let Some(reason) = &choice.finish_reason {
        let finish_reason = match reason.as_str() {
            "stop" => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
            "length" => FinishReason::with_raw(FinishReasonKind::Length, "length"),
            "tool_calls" => FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_calls"),
            "content_filter" => {
                FinishReason::with_raw(FinishReasonKind::ContentFilter, "content_filter")
            }
            raw => FinishReason::with_raw(FinishReasonKind::Other, raw),
        };

        // Emit ToolCallEnd for all accumulated tool calls
        if finish_reason.unified == FinishReasonKind::ToolCalls {
            let mut sorted_indices: Vec<_> = tool_calls.keys().cloned().collect();
            sorted_indices.sort();

            for index in sorted_indices {
                if let Some(tc) = tool_calls.remove(&index) {
                    let args_json = if tc.arguments.is_empty() {
                        serde_json::json!({})
                    } else {
                        serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}))
                    };
                    events.push(StreamEvent::tool_call_end(tc.id, tc.name, args_json));
                }
            }
        }

        events.push(StreamEvent::finish(
            accumulated_usage.clone().unwrap_or_default(),
            finish_reason,
        ));

        return Ok(events);
    }

    // Handle content delta
    if let Some(content) = &choice.delta.content {
        events.push(StreamEvent::text_delta(chunk.id.clone(), content.clone()));
    }

    // Start event
    if choice.delta.role.is_some() && events.is_empty() {
        events.push(StreamEvent::start(chunk.id));
    }

    Ok(events)
}

