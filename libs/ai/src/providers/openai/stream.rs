//! OpenAI streaming implementation
//!
//! Supports both Chat Completions API and Responses API streaming.
//!
//! Key behaviors for Completions API:
//! - Track tool call IDs by index - OpenAI only sends ID on first chunk for each tool call
//! - Subsequent chunks for the same tool call have id: None and use index to identify
//! - Accumulate tool call input and emit ToolCallEnd when finish_reason is "tool_calls"

use super::types::ChatCompletionChunk;
use crate::error::{Error, Result};
use crate::types::{FinishReason, FinishReasonKind, GenerateStream, StreamEvent, Usage};
use futures::StreamExt;
use reqwest_eventsource::{self, Event, EventSource};

/// Track state for each tool call during streaming
#[derive(Debug, Clone)]
struct ToolCallState {
    id: String,
    name: String,
    arguments: String,
}

// ============================================================================
// Chat Completions API Streaming
// ============================================================================

/// Create a streaming response from OpenAI Chat Completions API
pub async fn create_completions_stream(event_source: EventSource) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut event_stream = event_source;
        let mut accumulated_usage: Option<Usage> = None;
        // Track tool calls by index - stores ID, name, and accumulated arguments
        let mut tool_calls: std::collections::HashMap<u32, ToolCallState> = std::collections::HashMap::new();

        while let Some(event) = event_stream.next().await {
            match event {
                Ok(Event::Open) => {
                    // Connection opened
                }
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
                Err(e) => {
                    match e {
                        reqwest_eventsource::Error::StreamEnded => {
                            break;
                        }
                        reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                            let body = response.text().await.unwrap_or_default();
                            yield Err(Error::provider_error(format!(
                                "OpenAI API error {}: {}", status, body
                            )));
                            break;
                        }
                        other => {
                            yield Err(Error::stream_error(format!("Stream error: {}", other)));
                            break;
                        }
                    }
                }
            }
        }
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Parse a streaming chunk from OpenAI
/// Returns a Vec because finish can emit multiple ToolCallEnd events
fn parse_chunk(
    data: &str,
    accumulated_usage: &mut Option<Usage>,
    tool_calls: &mut std::collections::HashMap<u32, ToolCallState>,
) -> Result<Vec<StreamEvent>> {
    let chunk: ChatCompletionChunk = serde_json::from_str(data)
        .map_err(|e| Error::invalid_response(format!("Failed to parse chunk: {}", e)))?;

    // Capture usage if present (OpenAI sends this in the final chunk when stream_options.include_usage is true)
    if let Some(chat_usage) = &chunk.usage {
        *accumulated_usage = Some(Usage::new(
            chat_usage.prompt_tokens,
            chat_usage.completion_tokens,
        ));
    }

    let choice = match chunk.choices.first() {
        Some(c) => c,
        None => {
            // OpenAI sends usage in a final chunk with empty choices
            // Emit the usage event if we have accumulated usage
            if let Some(usage) = accumulated_usage.take() {
                return Ok(vec![StreamEvent::finish(
                    usage,
                    FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
                )]);
            }
            return Ok(Vec::new());
        }
    };

    let mut events = Vec::new();

    // Handle tool calls
    if let Some(tc_deltas) = &choice.delta.tool_calls {
        for tc in tc_deltas {
            // Get or create tool call state by index
            let tool_call = tool_calls.entry(tc.index).or_insert_with(|| ToolCallState {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });

            // Update ID if present (only on first chunk)
            if let Some(id) = &tc.id
                && !id.is_empty()
            {
                tool_call.id = id.clone();
            }

            if let Some(function) = &tc.function {
                // Update name if present (only on first chunk)
                if let Some(name) = &function.name {
                    tool_call.name = name.clone();
                    events.push(StreamEvent::tool_call_start(
                        tool_call.id.clone(),
                        name.clone(),
                    ));
                }

                // Accumulate arguments
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
            "content_filter" => {
                FinishReason::with_raw(FinishReasonKind::ContentFilter, "content_filter")
            }
            "tool_calls" => FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_calls"),
            raw => FinishReason::with_raw(FinishReasonKind::Other, raw),
        };

        // Emit ToolCallEnd for all accumulated tool calls
        if finish_reason.unified == FinishReasonKind::ToolCalls {
            // Sort by index to maintain order
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

    // Start event (role present but no content)
    if choice.delta.role.is_some() && events.is_empty() {
        events.push(StreamEvent::start(chunk.id));
    }

    Ok(events)
}

// ============================================================================
// Responses API Streaming
// ============================================================================

/// Track state for Responses API streaming
#[derive(Debug, Default)]
struct ResponsesStreamState {
    response_id: String,
    current_item: Option<CurrentItem>,
    tool_calls: std::collections::HashMap<String, ToolCallState>,
    usage: Option<Usage>,
    has_tool_calls: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum CurrentItem {
    Message {
        id: String,
        text: String,
    },
    Reasoning {
        id: String,
        text: String,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
}

/// Create a streaming response from OpenAI Responses API
pub async fn create_responses_stream(event_source: EventSource) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut event_stream = event_source;
        let mut state = ResponsesStreamState::default();
        let mut started = false;

        while let Some(event) = event_stream.next().await {
            match event {
                Ok(Event::Open) => {
                    // Connection opened
                }
                Ok(Event::Message(message)) => {
                    if message.data == "[DONE]" {
                        break;
                    }

                    match parse_responses_event(&message.event, &message.data, &mut state, &mut started) {
                        Ok(events) => {
                            for event in events {
                                yield Ok(event);
                            }
                        }
                        Err(e) => yield Err(e),
                    }
                }
                Err(e) => {
                    match e {
                        reqwest_eventsource::Error::StreamEnded => {
                            break;
                        }
                        reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                            let body = response.text().await.unwrap_or_default();
                            yield Err(Error::provider_error(format!(
                                "OpenAI Responses API error {}: {}", status, body
                            )));
                            break;
                        }
                        other => {
                            yield Err(Error::stream_error(format!("Stream error: {}", other)));
                            break;
                        }
                    }
                }
            }
        }
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Parse a streaming event from Responses API
fn parse_responses_event(
    event_type: &str,
    data: &str,
    state: &mut ResponsesStreamState,
    started: &mut bool,
) -> Result<Vec<StreamEvent>> {
    let event: serde_json::Value = serde_json::from_str(data)
        .map_err(|e| Error::invalid_response(format!("Failed to parse event: {}", e)))?;

    let mut events = Vec::new();

    match event_type {
        "response.output_item.added" => {
            let item = &event["item"];
            let item_type = item["type"].as_str().unwrap_or("");
            let item_id = item["id"].as_str().unwrap_or("").to_string();

            match item_type {
                "reasoning" => {
                    state.current_item = Some(CurrentItem::Reasoning {
                        id: item_id,
                        text: String::new(),
                    });
                }
                "message" => {
                    if !*started {
                        events.push(StreamEvent::start(state.response_id.clone()));
                        *started = true;
                    }
                    state.current_item = Some(CurrentItem::Message {
                        id: item_id,
                        text: String::new(),
                    });
                }
                "function_call" => {
                    state.has_tool_calls = true;
                    let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("").to_string();
                    let arguments = item["arguments"].as_str().unwrap_or("").to_string();

                    // Composite ID format: call_id|item_id
                    let composite_id = format!("{}|{}", call_id, item_id);

                    state.current_item = Some(CurrentItem::FunctionCall {
                        id: item_id.clone(),
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments,
                    });

                    state.tool_calls.insert(
                        item_id,
                        ToolCallState {
                            id: composite_id.clone(),
                            name: name.clone(),
                            arguments: String::new(),
                        },
                    );

                    events.push(StreamEvent::tool_call_start(composite_id, name));
                }
                _ => {}
            }
        }

        "response.output_text.delta" => {
            let delta = event["delta"].as_str().unwrap_or("");

            if !delta.is_empty() {
                if let Some(CurrentItem::Message { ref mut text, .. }) = state.current_item {
                    text.push_str(delta);
                }
                events.push(StreamEvent::text_delta(
                    state.response_id.clone(),
                    delta.to_string(),
                ));
            }
        }

        "response.reasoning_summary_text.delta" => {
            let delta = event["delta"].as_str().unwrap_or("");

            if !delta.is_empty()
                && let Some(CurrentItem::Reasoning { ref mut text, .. }) = state.current_item
            {
                text.push_str(delta);
            }
        }

        "response.function_call_arguments.delta" => {
            let delta = event["delta"].as_str().unwrap_or("");

            if let Some(CurrentItem::FunctionCall {
                ref id,
                ref mut arguments,
                ..
            }) = state.current_item
            {
                arguments.push_str(delta);

                if let Some(tc) = state.tool_calls.get_mut(id) {
                    tc.arguments.push_str(delta);
                    events.push(StreamEvent::tool_call_delta(
                        tc.id.clone(),
                        delta.to_string(),
                    ));
                }
            }
        }

        "response.function_call_arguments.done" => {
            if let Some(CurrentItem::FunctionCall {
                ref id,
                ref mut arguments,
                ..
            }) = state.current_item
            {
                let final_args = event["arguments"].as_str().unwrap_or("{}");
                *arguments = final_args.to_string();

                if let Some(tc) = state.tool_calls.get_mut(id) {
                    tc.arguments = final_args.to_string();
                }
            }
        }

        "response.output_item.done" => {
            let item = &event["item"];
            let item_type = item["type"].as_str().unwrap_or("");

            match item_type {
                "function_call" => {
                    let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                    let item_id = item["id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("").to_string();

                    // Get arguments from state or from item
                    let args_str =
                        if let Some(CurrentItem::FunctionCall { ref arguments, .. }) =
                            state.current_item
                        {
                            if !arguments.is_empty() {
                                arguments.clone()
                            } else {
                                item["arguments"].as_str().unwrap_or("{}").to_string()
                            }
                        } else {
                            item["arguments"].as_str().unwrap_or("{}").to_string()
                        };

                    let args_json: serde_json::Value =
                        serde_json::from_str(&args_str).unwrap_or(serde_json::json!({}));

                    let composite_id = format!("{}|{}", call_id, item_id);

                    state.tool_calls.remove(&item_id);
                    state.current_item = None;

                    events.push(StreamEvent::tool_call_end(composite_id, name, args_json));
                }
                "message" | "reasoning" => {
                    state.current_item = None;
                }
                _ => {}
            }
        }

        "response.completed" => {
            let response = &event["response"];

            // Parse usage
            // Note: input_tokens is the TOTAL input tokens (including cached)
            // cached_tokens is just metadata about billing, not a reduction in token count
            if let Some(usage) = response.get("usage") {
                let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0) as u32;
                let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0) as u32;

                state.usage = Some(Usage::new(input_tokens, output_tokens));
            }

            // Map status to finish reason
            let status = response["status"].as_str().unwrap_or("completed");
            let mut finish_reason = match status {
                "completed" => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
                "incomplete" => FinishReason::with_raw(FinishReasonKind::Length, "length"),
                "failed" | "cancelled" => FinishReason::with_raw(FinishReasonKind::Other, "error"),
                "in_progress" | "queued" => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
                _ => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
            };

            // If we had tool calls and completed, change to ToolCalls
            if state.has_tool_calls && finish_reason.unified == FinishReasonKind::Stop {
                finish_reason = FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_calls");
            }

            events.push(StreamEvent::finish(
                state.usage.clone().unwrap_or_default(),
                finish_reason,
            ));
        }

        "error" => {
            let code = event["code"].as_str().unwrap_or("unknown");
            let message = event["message"].as_str().unwrap_or("Unknown error");
            return Err(Error::provider_error(format!(
                "Error Code {}: {}",
                code, message
            )));
        }

        "response.failed" => {
            let error_msg = event["response"]["error"]["message"]
                .as_str()
                .or_else(|| event["response"]["status_details"]["error"]["message"].as_str())
                .unwrap_or("Unknown error");
            return Err(Error::provider_error(format!(
                "Response failed: {}",
                error_msg
            )));
        }

        _ => {
            // Ignore unknown event types for forward compatibility
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::openai::types::{
        ChatCompletionChunk, ChatDelta, ChatUsage, ChunkChoice, OpenAIFunctionCallDelta,
        OpenAIToolCallDelta,
    };

    fn make_chunk(
        id: &str,
        role: Option<&str>,
        content: Option<&str>,
        tool_calls: Option<Vec<OpenAIToolCallDelta>>,
        finish_reason: Option<&str>,
        usage: Option<ChatUsage>,
    ) -> String {
        let chunk = ChatCompletionChunk {
            id: id.to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "gpt-4".to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChatDelta {
                    role: role.map(|s| s.to_string()),
                    content: content.map(|s| s.to_string()),
                    tool_calls,
                },
                finish_reason: finish_reason.map(|s| s.to_string()),
            }],
            usage,
        };
        serde_json::to_string(&chunk).unwrap()
    }

    #[test]
    fn test_text_delta() {
        let mut usage = None;
        let mut tool_calls = std::collections::HashMap::new();

        let chunk = make_chunk("chatcmpl-123", None, Some("Hello"), None, None, None);
        let events = parse_chunk(&chunk, &mut usage, &mut tool_calls).unwrap();

        assert_eq!(events.len(), 1);
        if let StreamEvent::TextDelta { delta, .. } = &events[0] {
            assert_eq!(delta, "Hello");
        } else {
            panic!("Expected TextDelta");
        }
    }

    #[test]
    fn test_tool_call_complete_flow() {
        let mut usage = None;
        let mut tool_calls = std::collections::HashMap::new();

        // First chunk: tool call start with ID and name
        let chunk1 = make_chunk(
            "chatcmpl-123",
            None,
            None,
            Some(vec![OpenAIToolCallDelta {
                index: 0,
                id: Some("call_abc123".to_string()),
                type_: Some("function".to_string()),
                function: Some(OpenAIFunctionCallDelta {
                    name: Some("get_weather".to_string()),
                    arguments: Some("{\"loc".to_string()),
                }),
            }]),
            None,
            None,
        );

        let events = parse_chunk(&chunk1, &mut usage, &mut tool_calls).unwrap();
        assert_eq!(events.len(), 2); // ToolCallStart + ToolCallDelta

        if let StreamEvent::ToolCallStart { id, name } = &events[0] {
            assert_eq!(id, "call_abc123");
            assert_eq!(name, "get_weather");
        } else {
            panic!("Expected ToolCallStart");
        }

        // Second chunk: more arguments (no ID)
        let chunk2 = make_chunk(
            "chatcmpl-123",
            None,
            None,
            Some(vec![OpenAIToolCallDelta {
                index: 0,
                id: None, // ID not sent on subsequent chunks
                type_: None,
                function: Some(OpenAIFunctionCallDelta {
                    name: None,
                    arguments: Some("ation\":\"SF\"}".to_string()),
                }),
            }]),
            None,
            None,
        );

        let events = parse_chunk(&chunk2, &mut usage, &mut tool_calls).unwrap();
        assert_eq!(events.len(), 1);

        if let StreamEvent::ToolCallDelta { id, delta } = &events[0] {
            assert_eq!(id, "call_abc123"); // Should use stored ID
            assert_eq!(delta, "ation\":\"SF\"}");
        } else {
            panic!("Expected ToolCallDelta");
        }

        // Final chunk: finish with tool_calls reason
        let chunk3 = make_chunk(
            "chatcmpl-123",
            None,
            None,
            None,
            Some("tool_calls"),
            Some(ChatUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        );

        let events = parse_chunk(&chunk3, &mut usage, &mut tool_calls).unwrap();
        assert_eq!(events.len(), 2); // ToolCallEnd + Finish

        if let StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            ..
        } = &events[0]
        {
            assert_eq!(id, "call_abc123");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments["location"], "SF");
        } else {
            panic!("Expected ToolCallEnd");
        }

        if let StreamEvent::Finish { reason, usage: u } = &events[1] {
            assert!(matches!(reason.unified, FinishReasonKind::ToolCalls));
            assert_eq!(u.prompt_tokens, 10);
        } else {
            panic!("Expected Finish");
        }
    }

    #[test]
    fn test_multiple_tool_calls() {
        let mut usage = None;
        let mut tool_calls = std::collections::HashMap::new();

        // First tool call
        let chunk1 = make_chunk(
            "chatcmpl-123",
            None,
            None,
            Some(vec![OpenAIToolCallDelta {
                index: 0,
                id: Some("call_first".to_string()),
                type_: Some("function".to_string()),
                function: Some(OpenAIFunctionCallDelta {
                    name: Some("get_weather".to_string()),
                    arguments: Some("{\"city\":\"NYC\"}".to_string()),
                }),
            }]),
            None,
            None,
        );
        parse_chunk(&chunk1, &mut usage, &mut tool_calls).unwrap();

        // Second tool call
        let chunk2 = make_chunk(
            "chatcmpl-123",
            None,
            None,
            Some(vec![OpenAIToolCallDelta {
                index: 1,
                id: Some("call_second".to_string()),
                type_: Some("function".to_string()),
                function: Some(OpenAIFunctionCallDelta {
                    name: Some("get_time".to_string()),
                    arguments: Some("{\"tz\":\"EST\"}".to_string()),
                }),
            }]),
            None,
            None,
        );
        parse_chunk(&chunk2, &mut usage, &mut tool_calls).unwrap();

        // Finish
        let chunk3 = make_chunk("chatcmpl-123", None, None, None, Some("tool_calls"), None);

        let events = parse_chunk(&chunk3, &mut usage, &mut tool_calls).unwrap();
        assert_eq!(events.len(), 3); // 2 ToolCallEnd + Finish

        // Check first tool call end
        if let StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            ..
        } = &events[0]
        {
            assert_eq!(id, "call_first");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments["city"], "NYC");
        } else {
            panic!("Expected ToolCallEnd for first tool");
        }

        // Check second tool call end
        if let StreamEvent::ToolCallEnd {
            id,
            name,
            arguments,
            ..
        } = &events[1]
        {
            assert_eq!(id, "call_second");
            assert_eq!(name, "get_time");
            assert_eq!(arguments["tz"], "EST");
        } else {
            panic!("Expected ToolCallEnd for second tool");
        }
    }

    #[test]
    fn test_start_event() {
        let mut usage = None;
        let mut tool_calls = std::collections::HashMap::new();

        let chunk = make_chunk("chatcmpl-123", Some("assistant"), None, None, None, None);
        let events = parse_chunk(&chunk, &mut usage, &mut tool_calls).unwrap();

        assert_eq!(events.len(), 1);
        if let StreamEvent::Start { id } = &events[0] {
            assert_eq!(id, "chatcmpl-123");
        } else {
            panic!("Expected Start event");
        }
    }

    #[test]
    fn test_finish_stop() {
        let mut usage = None;
        let mut tool_calls = std::collections::HashMap::new();

        let chunk = make_chunk(
            "chatcmpl-123",
            None,
            None,
            None,
            Some("stop"),
            Some(ChatUsage {
                prompt_tokens: 5,
                completion_tokens: 10,
                total_tokens: 15,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        );

        let events = parse_chunk(&chunk, &mut usage, &mut tool_calls).unwrap();
        assert_eq!(events.len(), 1);

        if let StreamEvent::Finish { reason, usage: u } = &events[0] {
            assert!(matches!(reason.unified, FinishReasonKind::Stop));
            assert_eq!(u.total_tokens, 15);
        } else {
            panic!("Expected Finish event");
        }
    }
}
