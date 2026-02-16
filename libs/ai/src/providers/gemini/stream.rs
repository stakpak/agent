//! Gemini streaming support

use super::types::GeminiResponse;
use crate::error::{Error, Result};
use crate::types::{
    FinishReason, FinishReasonKind, GenerateStream, InputTokenDetails, OutputTokenDetails,
    StreamEvent, Usage,
};
use futures::stream::StreamExt;
use reqwest::Response;
use std::error::Error as StdError;

/// Create a stream from Gemini response
/// Gemini uses SSE framing (`data: {json}` events).
pub async fn create_stream(response: Response) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut accumulated_usage = Usage::default();
        let mut stream_id = String::new();
        let mut finished_emitted = false;

        let mut bytes_stream = response.bytes_stream();
        let mut line_buffer = String::new();
        let mut current_event_data = String::new();

        while let Some(chunk_result) = bytes_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    let text = String::from_utf8_lossy(&chunk);
                    line_buffer.push_str(&text);

                    // Yield complete lines as they arrive
                    while let Some(pos) = line_buffer.find('\n') {
                        // pos is from find('\n') on the same string, so it's always a valid char boundary
                        #[allow(clippy::string_slice)]
                        let line = line_buffer[..pos].trim_end_matches('\r').to_string();
                        #[allow(clippy::string_slice)]
                        {
                            line_buffer = line_buffer[pos + 1..].to_string();
                        }

                        if let Some(event_data) =
                            process_sse_line(&line, &mut current_event_data)
                        {
                            match parse_sse_event_data(&event_data) {
                                Ok(Some(resp)) => {
                                    for event in process_gemini_response(
                                        resp,
                                        &mut accumulated_usage,
                                        &mut stream_id,
                                    ) {
                                        if matches!(event, StreamEvent::Finish { .. }) {
                                            finished_emitted = true;
                                        }
                                        yield Ok(event);
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    yield Err(e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Err(Error::stream_error(format!(
                        "Stream error: {} | source: {:?}",
                        e,
                        e.source()
                    )));
                    break;
                }
            }
        }

        // Handle a final line that may not end with '\n'.
        if !line_buffer.is_empty() {
            let line = line_buffer.trim_end_matches('\r');
            if let Some(event_data) = process_sse_line(line, &mut current_event_data) {
                match parse_sse_event_data(&event_data) {
                    Ok(Some(resp)) => {
                        for event in process_gemini_response(
                            resp,
                            &mut accumulated_usage,
                            &mut stream_id,
                        ) {
                            if matches!(event, StreamEvent::Finish { .. }) {
                                finished_emitted = true;
                            }
                            yield Ok(event);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        yield Err(e);
                    }
                }
            }
        }

        // Flush a trailing event if the stream closed without an empty separator line.
        if !current_event_data.trim().is_empty() {
            let event_data = std::mem::take(&mut current_event_data);
            match parse_sse_event_data(&event_data) {
                Ok(Some(resp)) => {
                    for event in process_gemini_response(
                        resp,
                        &mut accumulated_usage,
                        &mut stream_id,
                    ) {
                        if matches!(event, StreamEvent::Finish { .. }) {
                            finished_emitted = true;
                        }
                        yield Ok(event);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    yield Err(e);
                }
            }
        }

        if !finished_emitted {
            yield Ok(StreamEvent::finish(accumulated_usage, FinishReason::stop()));
        }
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Process one SSE line and return completed event data on blank-line separator.
fn process_sse_line(line: &str, current_event_data: &mut String) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        if current_event_data.trim().is_empty() {
            current_event_data.clear();
            None
        } else {
            Some(std::mem::take(current_event_data))
        }
    } else if let Some(data) = trimmed.strip_prefix("data:") {
        // Accept both "data:{json}" and "data: {json}".
        let data = data.strip_prefix(' ').unwrap_or(data);
        if !current_event_data.is_empty() {
            current_event_data.push('\n');
        }
        current_event_data.push_str(data);
        None
    } else {
        // Ignore comments/other SSE fields (event, id, retry, ...).
        None
    }
}

/// Parse an SSE event payload into a GeminiResponse.
fn parse_sse_event_data(event_data: &str) -> Result<Option<GeminiResponse>> {
    let payload = event_data.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(None);
    }

    serde_json::from_str::<GeminiResponse>(payload)
        .map(Some)
        .map_err(|e| {
            Error::stream_error(format!(
                "Failed to parse Gemini SSE payload: {}. Payload: {}",
                e, payload
            ))
        })
}

/// Process Gemini response and convert to unified StreamEvent
fn process_gemini_response(
    resp: GeminiResponse,
    accumulated_usage: &mut Usage,
    stream_id: &mut String,
) -> Vec<StreamEvent> {
    // Update usage if available
    if let Some(usage) = resp.usage_metadata {
        let prompt_tokens = usage.prompt_token_count.unwrap_or(0);
        let completion_tokens = usage.candidates_token_count.unwrap_or(0);
        let cached_tokens = usage.cached_content_token_count.unwrap_or(0);
        let reasoning_tokens = usage.thoughts_token_count;

        *accumulated_usage = Usage::with_details(
            InputTokenDetails {
                total: Some(prompt_tokens),
                no_cache: Some(prompt_tokens.saturating_sub(cached_tokens)),
                cache_read: if cached_tokens > 0 {
                    Some(cached_tokens)
                } else {
                    None
                },
                cache_write: None,
            },
            OutputTokenDetails {
                total: Some(completion_tokens),
                text: reasoning_tokens.map(|r| completion_tokens.saturating_sub(r)),
                reasoning: reasoning_tokens,
            },
            Some(serde_json::to_value(&usage).unwrap_or_default()),
        );
    }

    // Get first candidate
    let candidates = resp.candidates.unwrap_or_default();
    let candidate = candidates.first();

    if candidate.is_none() {
        // Just return empty if no candidate, unless we have usage
        return Vec::new();
    }
    let candidate = candidate.unwrap();

    // Check if this is the start
    if stream_id.is_empty() {
        *stream_id = format!(
            "gemini-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
    }

    let mut events = Vec::new();

    // Extract text and function calls from parts
    for part in candidate
        .content
        .as_ref()
        .map(|c| c.parts.as_slice())
        .unwrap_or_default()
    {
        if let Some(text) = &part.text
            && !text.is_empty()
        {
            events.push(StreamEvent::text_delta(stream_id.clone(), text.clone()));
        }

        // Handle function calls (Gemini sends complete function calls, not deltas)
        // We emit ToolCallStart + ToolCallDelta to match the expected streaming pattern.
        // ToolCallEnd is just a completion signal (no arguments) to avoid doubling.
        if let Some(function_call) = &part.function_call {
            let call_id = format!("call_{}", uuid::Uuid::new_v4());
            // Preserve thought_signature from the Part level as metadata
            let metadata = part
                .thought_signature
                .as_ref()
                .map(|sig| serde_json::json!({ "thought_signature": sig }));
            events.push(StreamEvent::tool_call_start(
                call_id.clone(),
                function_call.name.clone(),
            ));
            events.push(StreamEvent::tool_call_delta(
                call_id.clone(),
                function_call.args.to_string(),
            ));
            events.push(StreamEvent::tool_call_end_with_metadata(
                call_id,
                function_call.name.clone(),
                function_call.args.clone(),
                metadata,
            ));
        }
    }

    if let Some(finish_reason) = &candidate.finish_reason {
        let reason = match finish_reason.as_str() {
            "STOP" => FinishReason::with_raw(FinishReasonKind::Stop, "STOP"),
            "MAX_TOKENS" => FinishReason::with_raw(FinishReasonKind::Length, "MAX_TOKENS"),
            "SAFETY" => FinishReason::with_raw(FinishReasonKind::ContentFilter, "SAFETY"),
            raw => FinishReason::with_raw(FinishReasonKind::Other, raw),
        };
        events.push(StreamEvent::finish(accumulated_usage.clone(), reason));
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::gemini::types::{
        GeminiCandidate, GeminiContent, GeminiFunctionCall, GeminiPart,
    };

    #[test]
    fn test_process_sse_line_accepts_data_prefix_with_or_without_space() {
        let mut buf = String::new();
        assert!(process_sse_line(r#"data: {"a":1}"#, &mut buf).is_none());
        assert_eq!(buf, r#"{"a":1}"#);

        let mut buf_no_space = String::new();
        assert!(process_sse_line(r#"data:{"b":2}"#, &mut buf_no_space).is_none());
        assert_eq!(buf_no_space, r#"{"b":2}"#);
    }

    #[test]
    fn test_process_sse_line_flushes_on_blank_separator() {
        let mut buf = String::new();
        assert!(process_sse_line(r#"data: {"first":1}"#, &mut buf).is_none());
        assert!(process_sse_line(r#"data: {"second":2}"#, &mut buf).is_none());

        let flushed = process_sse_line("", &mut buf).expect("expected completed event");
        assert_eq!(flushed, "{\"first\":1}\n{\"second\":2}");
        assert!(buf.is_empty());
    }

    #[test]
    fn test_parse_sse_event_data_returns_error_for_invalid_json() {
        let err = parse_sse_event_data("{not-json}").expect_err("expected parse error");
        assert!(
            err.to_string()
                .contains("Failed to parse Gemini SSE payload")
        );
    }

    #[test]
    fn test_process_gemini_response_text() {
        let mut usage = Usage::default();
        let mut stream_id = String::new();

        let resp = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart {
                        text: Some("Hello".to_string()),
                        inline_data: None,
                        function_call: None,
                        function_response: None,
                        thought_signature: None,
                    }],
                }),
                finish_reason: None,
                safety_ratings: None,
            }]),
            usage_metadata: None,
            model_version: None,
            response_id: None,
        };

        let result = process_gemini_response(resp, &mut usage, &mut stream_id);
        assert!(!result.is_empty());

        if let Some(StreamEvent::TextDelta { delta, .. }) = result.first() {
            assert_eq!(delta, "Hello");
        }
    }

    #[test]
    fn test_process_gemini_response_function_call() {
        let mut usage = Usage::default();
        let mut stream_id = String::new();

        let resp = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart {
                        text: None,
                        inline_data: None,
                        function_call: Some(GeminiFunctionCall {
                            id: None,
                            name: "get_weather".to_string(),
                            args: serde_json::json!({"location": "San Francisco"}),
                        }),
                        function_response: None,
                        thought_signature: None,
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
            }]),
            usage_metadata: None,
            model_version: None,
            response_id: None,
        };

        let result = process_gemini_response(resp, &mut usage, &mut stream_id);

        // Should have ToolCallStart, ToolCallDelta, ToolCallEnd, and Finish
        assert_eq!(result.len(), 4);

        // Verify ToolCallStart
        if let StreamEvent::ToolCallStart { name, .. } = &result[0] {
            assert_eq!(name, "get_weather");
        } else {
            panic!("Expected ToolCallStart, got {:?}", result[0]);
        }

        // Verify ToolCallDelta has the arguments
        if let StreamEvent::ToolCallDelta { delta, .. } = &result[1] {
            assert!(delta.contains("San Francisco"));
        } else {
            panic!("Expected ToolCallDelta, got {:?}", result[1]);
        }

        // Verify ToolCallEnd
        if let StreamEvent::ToolCallEnd { name, .. } = &result[2] {
            assert_eq!(name, "get_weather");
        } else {
            panic!("Expected ToolCallEnd, got {:?}", result[2]);
        }

        if let StreamEvent::Finish { reason, .. } = &result[3] {
            assert!(matches!(reason.unified, FinishReasonKind::Stop));
        } else {
            panic!("Expected Finish");
        }
    }

    #[test]
    fn test_process_gemini_response_multiple_function_calls() {
        let mut usage = Usage::default();
        let mut stream_id = String::new();

        let resp = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![
                        GeminiPart {
                            text: None,
                            inline_data: None,
                            function_call: Some(GeminiFunctionCall {
                                id: None,
                                name: "get_weather".to_string(),
                                args: serde_json::json!({"location": "NYC"}),
                            }),
                            function_response: None,
                            thought_signature: None,
                        },
                        GeminiPart {
                            text: None,
                            inline_data: None,
                            function_call: Some(GeminiFunctionCall {
                                id: None,
                                name: "get_time".to_string(),
                                args: serde_json::json!({"timezone": "EST"}),
                            }),
                            function_response: None,
                            thought_signature: None,
                        },
                    ],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
            }]),
            usage_metadata: None,
            model_version: None,
            response_id: None,
        };

        let result = process_gemini_response(resp, &mut usage, &mut stream_id);

        // Should have 2 * (ToolCallStart + ToolCallDelta + ToolCallEnd) + Finish = 7
        assert_eq!(result.len(), 7);

        // First tool call
        if let StreamEvent::ToolCallStart { name, .. } = &result[0] {
            assert_eq!(name, "get_weather");
        } else {
            panic!("Expected ToolCallStart for first tool");
        }

        if let StreamEvent::ToolCallDelta { delta, .. } = &result[1] {
            assert!(delta.contains("NYC"));
        } else {
            panic!("Expected ToolCallDelta for first tool");
        }

        // Second tool call
        if let StreamEvent::ToolCallStart { name, .. } = &result[3] {
            assert_eq!(name, "get_time");
        } else {
            panic!("Expected ToolCallStart for second tool");
        }

        if let StreamEvent::ToolCallDelta { delta, .. } = &result[4] {
            assert!(delta.contains("EST"));
        } else {
            panic!("Expected ToolCallDelta for second tool");
        }
    }

    #[test]
    fn test_process_gemini_response_with_usage() {
        let mut usage = Usage::default();
        let mut stream_id = String::new();

        let resp = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart {
                        text: Some("Hello".to_string()),
                        inline_data: None,
                        function_call: None,
                        function_response: None,
                        thought_signature: None,
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
            }]),
            usage_metadata: Some(crate::providers::gemini::types::GeminiUsageMetadata {
                prompt_token_count: Some(10),
                cached_content_token_count: None,
                candidates_token_count: Some(20),
                total_token_count: Some(30),
                thoughts_token_count: None,
                prompt_tokens_details: None,
                candidates_tokens_details: None,
            }),
            model_version: None,
            response_id: None,
        };

        let result = process_gemini_response(resp, &mut usage, &mut stream_id);

        // Check usage was accumulated
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);

        // Should have TextDelta and Finish
        assert_eq!(result.len(), 2);
    }
}
