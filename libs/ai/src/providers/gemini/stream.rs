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

        let mut bytes_stream = response.bytes_stream();
        let mut buffer = Vec::new();

        while let Some(chunk_result) = bytes_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    buffer.extend_from_slice(&chunk);

                    // Process complete lines
                    while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                        let line = String::from_utf8_lossy(&line_bytes);
                        let line = line.trim();

                        if line.is_empty() {
                            continue;
                        }

                        // Parse JSON response
                        match serde_json::from_str::<GeminiResponse>(line) {
                            Ok(gemini_resp) => {
                                if let Some(event) = process_gemini_response(
                                    gemini_resp,
                                    &mut accumulated_usage,
                                    &mut stream_id
                                ) {
                                    yield Ok(event);
                                }
                            }
                            Err(e) => {
                                yield Err(Error::stream_error(format!("Failed to parse JSON: {}", e)));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Err(Error::stream_error(format!("Stream error: {}", e)));
                    break;
                }
            }
        }

        // Emit final usage if we have any
        if accumulated_usage.total_tokens > 0 {
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
) -> Option<StreamEvent> {
    // Update usage if available
    if let Some(usage) = resp.usage_metadata {
        accumulated_usage.prompt_tokens = usage.prompt_token_count.unwrap_or(0);
        accumulated_usage.completion_tokens = usage.candidates_token_count.unwrap_or(0);
        accumulated_usage.total_tokens = usage.total_token_count.unwrap_or(0);
    }

    // Get first candidate
    let candidate = resp.candidates.first()?;

    // Check if this is the start
    if stream_id.is_empty() {
        *stream_id = format!(
            "gemini-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        // Don't emit start event, just track ID
    }

    // Extract text and function calls from parts
    for part in &candidate.content.parts {
        if let Some(text) = &part.text
            && !text.is_empty()
        {
            return Some(StreamEvent::text_delta(stream_id.clone(), text.clone()));
        }

        // Handle function calls (Gemini sends complete function calls, not deltas)
        if let Some(function_call) = &part.function_call {
            let call_id = format!("call_{}", uuid::Uuid::new_v4());
            return Some(StreamEvent::tool_call_end(
                call_id,
                function_call.name.clone(),
                function_call.args.clone(),
            ));
        }
    }

    // Check if finished
    if let Some(finish_reason) = &candidate.finish_reason {
        let reason = match finish_reason.as_str() {
            "STOP" => FinishReason::Stop,
            "MAX_TOKENS" => FinishReason::Length,
            "SAFETY" => FinishReason::ContentFilter,
            _ => FinishReason::Other,
        };
        return Some(StreamEvent::finish(accumulated_usage.clone(), reason));
    }

    None
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
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart {
                        text: Some("Hello".to_string()),
                        inline_data: None,
                        function_call: None,
                        function_response: None,
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };

        let result = process_gemini_response(resp, &mut usage, &mut stream_id);
        assert!(result.is_some());

        if let Some(StreamEvent::TextDelta { delta, .. }) = result {
            assert_eq!(delta, "Hello");
        }
    }
}
