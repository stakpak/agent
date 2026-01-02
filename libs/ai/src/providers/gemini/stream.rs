//! Gemini streaming support

use super::types::GeminiResponse;
use crate::error::{Error, Result};
use crate::types::{FinishReason, GenerateStream, StreamEvent, Usage};
use futures::stream::StreamExt;
use reqwest::Response;

/// Create a stream from Gemini response
/// Gemini uses JSON streaming (not SSE) - each line is a complete JSON object
pub async fn create_stream(response: Response) -> Result<GenerateStream> {
    let stream = async_stream::stream! {
        let mut accumulated_usage = Usage::default();
        let mut stream_id = String::new();
        let mut finished_emitted = false;

        let mut bytes_stream = response.bytes_stream();
        let mut line_buffer = String::new();
        let mut json_accumulator = String::new();
        let mut brace_depth = 0;
        let mut in_object = false;

        while let Some(chunk_result) = bytes_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    let text = String::from_utf8_lossy(&chunk);
                    line_buffer.push_str(&text);

                    // Process complete lines from buffer
                    while let Some(newline_pos) = line_buffer.find('\n') {
                        let line = line_buffer[..newline_pos].trim().to_string();
                        line_buffer = line_buffer[newline_pos + 1..].to_string();

                        if line.is_empty() {
                            continue;
                        }

                        for ch in line.chars() {
                            match ch {
                                '{' => {
                                    brace_depth += 1;
                                    if !in_object {
                                        in_object = true;
                                    }
                                }
                                '}' => {
                                    brace_depth -= 1;
                                }
                                _ => {}
                            }
                        }

                        if in_object {
                            if !json_accumulator.is_empty() {
                                json_accumulator.push('\n');
                            }
                            json_accumulator.push_str(&line);
                        }

                        if in_object && brace_depth == 0 {
                            let mut json_str = json_accumulator.trim();

                            if json_str.starts_with('[') {
                                json_str = json_str[1..].trim();
                            }
                            if json_str.ends_with(',') {
                                json_str = json_str[..json_str.len() - 1].trim();
                            }
                            if json_str.ends_with(']') {
                                json_str = json_str[..json_str.len() - 1].trim();
                            }

                            if !json_str.is_empty() {
                                match serde_json::from_str::<GeminiResponse>(json_str) {
                                    Ok(gemini_resp) => {
                                        let events = process_gemini_response(
                                            gemini_resp,
                                            &mut accumulated_usage,
                                            &mut stream_id
                                        );
                                        for event in events {
                                            if matches!(event, StreamEvent::Finish { .. }) {
                                                finished_emitted = true;
                                            }
                                            yield Ok(event);
                                        }
                                    }
                                    Err(e) => {
                                        yield Err(Error::stream_error(format!("Failed to parse JSON: {}. JSON: {}", e, json_str)));
                                    }
                                }
                            }

                            // Reset for next object
                            json_accumulator.clear();
                            in_object = false;
                        }
                    }
                }
                Err(e) => {
                    yield Err(Error::stream_error(format!("Stream error: {}", e)));
                    break;
                }
            }
        }

        let line = line_buffer.trim();
        if !line.is_empty()
            && (line.starts_with('{') || line.starts_with('[')) {
                let mut json_str = line;
                if json_str.starts_with('[') { json_str = json_str[1..].trim(); }
                if json_str.ends_with(']') { json_str = json_str[..json_str.len()-1].trim(); }
                if json_str.ends_with(',') { json_str = json_str[..json_str.len()-1].trim(); }

                if !json_str.is_empty() && let Ok(gemini_resp) = serde_json::from_str::<GeminiResponse>(json_str) {
                    let events = process_gemini_response(
                        gemini_resp,
                        &mut accumulated_usage,
                        &mut stream_id
                    );
                    for event in events {
                        if matches!(event, StreamEvent::Finish { .. }) {
                            finished_emitted = true;
                        }
                        yield Ok(event);
                    }
                }
            }

        // Emit final finish event if we haven't yet
        if !finished_emitted {
            yield Ok(StreamEvent::finish(accumulated_usage, FinishReason::Stop));
        }
    };

    Ok(GenerateStream::new(Box::pin(stream)))
}

/// Process Gemini response and convert to unified StreamEvent
fn process_gemini_response(
    resp: GeminiResponse,
    accumulated_usage: &mut Usage,
    stream_id: &mut String,
) -> Vec<StreamEvent> {
    // Update usage if available
    if let Some(usage) = resp.usage_metadata {
        accumulated_usage.prompt_tokens = usage.prompt_token_count.unwrap_or(0);
        accumulated_usage.completion_tokens = usage.candidates_token_count.unwrap_or(0);
        accumulated_usage.total_tokens = usage.total_token_count.unwrap_or(0);
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
        if let Some(function_call) = &part.function_call {
            let call_id = format!("call_{}", uuid::Uuid::new_v4());
            events.push(StreamEvent::tool_call_end(
                call_id,
                function_call.name.clone(),
                function_call.args.clone(),
            ));
        }
    }

    if let Some(finish_reason) = &candidate.finish_reason {
        let reason = match finish_reason.as_str() {
            "STOP" => FinishReason::Stop,
            "MAX_TOKENS" => FinishReason::Length,
            "SAFETY" => FinishReason::ContentFilter,
            _ => FinishReason::Other,
        };
        events.push(StreamEvent::finish(accumulated_usage.clone(), reason));
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::gemini::types::{GeminiCandidate, GeminiContent, GeminiPart};

    #[test]
    fn test_process_gemini_response() {
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
}
