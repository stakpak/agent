//! Conversion from SDK types to MiniMax's OpenAI-compatible request format
//!
//! MiniMax's API is OpenAI-compatible, so we reuse the OpenAI wire types.
//! Key difference: temperature must be in the range (0.0, 1.0].

use crate::error::{Error, Result};
use crate::providers::openai::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatUsage, OpenAIFunctionCall,
    OpenAIToolCall, StreamOptions,
};
use crate::types::{ContentPart, ImageDetail, Message, Role};
use crate::types::{
    FinishReason, FinishReasonKind, GenerateRequest, GenerateResponse, InputTokenDetails,
    OutputTokenDetails, ResponseContent, ToolCall, Usage,
};
use serde_json::json;

/// Clamp temperature to MiniMax's valid range (0.0, 1.0].
/// MiniMax rejects temperature = 0.0, so we use a small epsilon.
fn clamp_temperature(temp: Option<f32>) -> f32 {
    match temp {
        Some(t) if t <= 0.0 => 0.01,
        Some(t) if t > 1.0 => 1.0,
        Some(t) => t,
        None => 0.01,
    }
}

/// Parse MiniMax message content
fn parse_minimax_message(msg: &ChatMessage) -> Result<Vec<ResponseContent>> {
    let mut content = Vec::new();

    // Handle text content
    if let Some(content_value) = &msg.content
        && let Some(text) = content_value.as_str()
        && !text.is_empty()
    {
        content.push(ResponseContent::Text {
            text: text.to_string(),
        });
    }

    // Handle tool calls
    if let Some(tool_calls) = &msg.tool_calls {
        for tc in tool_calls {
            content.push(ResponseContent::ToolCall(ToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| json!({})),
                metadata: None,
            }));
        }
    }

    Ok(content)
}

/// Convert an SDK request to a MiniMax-compatible OpenAI chat completion request.
pub fn to_minimax_request(req: &GenerateRequest, stream: bool) -> ChatCompletionRequest {
    let tools = req.options.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": tool.tool_type,
                    "function": {
                        "name": tool.function.name,
                        "description": tool.function.description,
                        "parameters": tool.function.parameters,
                    }
                })
            })
            .collect::<Vec<_>>()
    });

    let tool_choice = req.options.tool_choice.as_ref().map(|choice| match choice {
        crate::types::ToolChoice::Auto => json!("auto"),
        crate::types::ToolChoice::None => json!("none"),
        crate::types::ToolChoice::Required { name } => json!({
            "type": "function",
            "function": { "name": name }
        }),
    });

    let messages: Vec<ChatMessage> = req.messages.iter().flat_map(to_minimax_messages).collect();

    let stream_options = if stream {
        Some(StreamOptions {
            include_usage: true,
        })
    } else {
        None
    };

    ChatCompletionRequest {
        model: req.model.id.clone(),
        messages,
        temperature: Some(clamp_temperature(req.options.temperature)),
        max_completion_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stop: req.options.stop_sequences.clone(),
        stream: Some(stream),
        stream_options,
        tools,
        tool_choice,
    }
}

/// Convert a single SDK message into one or more OpenAI-format `ChatMessage`s.
///
/// Messages with multiple `ToolResult` parts are expanded so that each tool result
/// gets its own message with the correct `tool_call_id`.
fn to_minimax_messages(msg: &Message) -> Vec<ChatMessage> {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    let parts = msg.parts();

    // Collect tool results -- if more than one, each needs its own ChatMessage
    let tool_results: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::ToolResult {
                tool_call_id,
                content,
                ..
            } => Some((tool_call_id.clone(), content.clone())),
            _ => None,
        })
        .collect();

    if tool_results.len() > 1 {
        return tool_results
            .into_iter()
            .map(|(tool_call_id, content)| ChatMessage {
                role: "tool".to_string(),
                content: Some(content),
                name: msg.name.clone(),
                tool_calls: None,
                tool_call_id: Some(tool_call_id),
            })
            .collect();
    }

    // Single tool result or non-tool message -- standard conversion
    let tool_call_id = parts.iter().find_map(|part| match part {
        ContentPart::ToolResult { tool_call_id, .. } => Some(tool_call_id.clone()),
        _ => None,
    });

    let tool_calls = {
        let calls: Vec<_> = parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => Some(OpenAIToolCall {
                    id: id.clone(),
                    type_: "function".to_string(),
                    function: OpenAIFunctionCall {
                        name: name.clone(),
                        arguments: arguments.to_string(),
                    },
                }),
                _ => None,
            })
            .collect();
        if calls.is_empty() { None } else { Some(calls) }
    };

    let content = if parts.len() == 1 {
        match &parts[0] {
            ContentPart::Text { text, .. } => Some(json!(text)),
            ContentPart::Image { url, detail, .. } => Some(json!([{
                "type": "image_url",
                "image_url": {
                    "url": url,
                    "detail": detail.map(|d| match d {
                        ImageDetail::Low => "low",
                        ImageDetail::High => "high",
                        ImageDetail::Auto => "auto",
                    })
                }
            }])),
            ContentPart::ToolCall { .. } => None,
            ContentPart::ToolResult { content, .. } => Some(content.clone()),
        }
    } else {
        Some(json!(
            parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text, .. } => Some(json!({
                        "type": "text",
                        "text": text
                    })),
                    ContentPart::Image { url, detail, .. } => Some(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": url,
                            "detail": detail.map(|d| match d {
                                ImageDetail::Low => "low",
                                ImageDetail::High => "high",
                                ImageDetail::Auto => "auto",
                            })
                        }
                    })),
                    ContentPart::ToolCall { .. } => None, // Handled via tool_calls field
                    ContentPart::ToolResult { .. } => None, // Handled via tool_call_id field
                })
                .collect::<Vec<_>>()
        ))
    };

    vec![ChatMessage {
        role: role.to_string(),
        content,
        name: msg.name.clone(),
        tool_calls,
        tool_call_id,
    }]
}

/// Convert MiniMax (OpenAI-compatible) response to SDK response
pub fn from_minimax_response(resp: ChatCompletionResponse) -> Result<GenerateResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| Error::invalid_response("No choices in response"))?;

    let content = parse_minimax_message(&choice.message)?;

    let finish_reason = match choice.finish_reason.as_deref() {
        Some("stop") => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
        Some("length") => FinishReason::with_raw(FinishReasonKind::Length, "length"),
        Some("tool_calls") => FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_calls"),
        Some("content_filter") => {
            FinishReason::with_raw(FinishReasonKind::ContentFilter, "content_filter")
        }
        Some(raw) => FinishReason::with_raw(FinishReasonKind::Other, raw),
        None => FinishReason::other(),
    };

    let usage = usage_from_chat_usage(&resp.usage);

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
            "created": resp.created,
            "object": resp.object,
        })),
        warnings: None,
    })
}

/// Convert OpenAI-compatible ChatUsage to SDK Usage
pub fn usage_from_chat_usage(usage: &ChatUsage) -> Usage {
    let cache_read = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);

    Usage::with_details(
        InputTokenDetails {
            total: Some(usage.prompt_tokens),
            no_cache: Some(usage.prompt_tokens.saturating_sub(cache_read)),
            cache_read: (cache_read > 0).then_some(cache_read),
            cache_write: None,
        },
        OutputTokenDetails {
            total: Some(usage.completion_tokens),
            text: None,
            reasoning: usage
                .completion_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens),
        },
        Some(serde_json::to_value(usage).unwrap_or_default()),
    )
}

/// Parse MiniMax API error and return user-friendly message
pub fn parse_minimax_error(error_text: &str, status_code: u16) -> String {
    // Try to parse as JSON error
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(error_text)
        && let Some(error) = json.get("error")
    {
        let message = error.get("message").and_then(|m| m.as_str()).unwrap_or("");
        let error_type = error.get("type").and_then(|t| t.as_str()).unwrap_or("");

        // Check for rate limit
        if error_type == "rate_limit_error" || status_code == 429 {
            return format!(
                "Rate limited. Please wait a moment and try again. {}",
                message
            );
        }

        // Check for authentication errors
        if error_type == "authentication_error" || status_code == 401 {
            return format!(
                "Authentication failed. Please check your MiniMax API key. {}",
                message
            );
        }

        // Return the message if we have one
        if !message.is_empty() {
            return message.to_string();
        }
    }

    // Fallback to raw error
    format!("MiniMax API error {}: {}", status_code, error_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MessageContent, Model};

    fn make_request(messages: Vec<Message>) -> GenerateRequest {
        GenerateRequest::new(Model::custom("MiniMax-M2.7", "minimax"), messages)
    }

    #[test]
    fn test_clamp_temperature_zero() {
        assert_eq!(clamp_temperature(Some(0.0)), 0.01);
    }

    #[test]
    fn test_clamp_temperature_negative() {
        assert_eq!(clamp_temperature(Some(-1.0)), 0.01);
    }

    #[test]
    fn test_clamp_temperature_above_one() {
        assert_eq!(clamp_temperature(Some(2.0)), 1.0);
    }

    #[test]
    fn test_clamp_temperature_valid() {
        assert_eq!(clamp_temperature(Some(0.7)), 0.7);
    }

    #[test]
    fn test_clamp_temperature_none() {
        assert_eq!(clamp_temperature(None), 0.01);
    }

    #[test]
    fn test_basic_user_message() {
        let req = make_request(vec![Message::new(Role::User, "Hello")]);
        let result = to_minimax_request(&req, false);

        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, "user");
        assert_eq!(result.messages[0].content, Some(json!("Hello")));
        // Temperature should be clamped (default 0.01 when not set)
        assert!(result.temperature.unwrap() > 0.0);
    }

    #[test]
    fn test_system_message_stays_system() {
        let req = make_request(vec![
            Message::new(Role::System, "You are helpful"),
            Message::new(Role::User, "Hi"),
        ]);
        let result = to_minimax_request(&req, false);

        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].role, "system");
    }

    #[test]
    fn test_single_tool_result_not_expanded() {
        let tool_msg = Message {
            role: Role::Tool,
            content: MessageContent::Parts(vec![ContentPart::tool_result(
                "call_1",
                json!("result 1"),
            )]),
            name: None,
            provider_options: None,
        };

        let req = make_request(vec![tool_msg]);
        let result = to_minimax_request(&req, false);

        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, "tool");
        assert_eq!(result.messages[0].tool_call_id, Some("call_1".to_string()));
    }

    #[test]
    fn test_merged_tool_results_expanded() {
        let merged_tool_msg = Message {
            role: Role::Tool,
            content: MessageContent::Parts(vec![
                ContentPart::tool_result("call_1", json!("result 1")),
                ContentPart::tool_result("call_2", json!("result 2")),
                ContentPart::tool_result("call_3", json!("result 3")),
            ]),
            name: None,
            provider_options: None,
        };

        let req = make_request(vec![merged_tool_msg]);
        let result = to_minimax_request(&req, false);

        assert_eq!(result.messages.len(), 3);
        for (i, msg) in result.messages.iter().enumerate() {
            assert_eq!(msg.role, "tool");
            assert_eq!(msg.tool_call_id, Some(format!("call_{}", i + 1)));
            assert_eq!(msg.content, Some(json!(format!("result {}", i + 1))));
        }
    }

    #[test]
    fn test_streaming_request() {
        let req = make_request(vec![Message::new(Role::User, "Hello")]);
        let result = to_minimax_request(&req, true);

        assert_eq!(result.stream, Some(true));
        assert!(result.stream_options.is_some());
        assert!(
            result
                .stream_options
                .as_ref()
                .map(|o| o.include_usage)
                .unwrap_or(false)
        );
    }

    #[test]
    fn test_non_streaming_request() {
        let req = make_request(vec![Message::new(Role::User, "Hello")]);
        let result = to_minimax_request(&req, false);

        assert_eq!(result.stream, Some(false));
        assert!(result.stream_options.is_none());
    }

    #[test]
    fn test_assistant_tool_calls_converted() {
        let msg = Message {
            role: Role::Assistant,
            content: MessageContent::Parts(vec![ContentPart::ToolCall {
                id: "call_abc".to_string(),
                name: "get_weather".to_string(),
                arguments: json!({"location": "NYC"}),
                provider_options: None,
                metadata: None,
            }]),
            name: None,
            provider_options: None,
        };

        let req = make_request(vec![msg]);
        let result = to_minimax_request(&req, false);

        assert_eq!(result.messages.len(), 1);
        let tool_calls = result.messages[0]
            .tool_calls
            .as_ref()
            .expect("should have tool_calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_abc");
        assert_eq!(tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn test_tools_converted() {
        use crate::types::{GenerateOptions, Tool, ToolFunction};

        let mut req = make_request(vec![Message::new(Role::User, "Hello")]);
        req.options = GenerateOptions {
            tools: Some(vec![Tool {
                tool_type: "function".to_string(),
                function: ToolFunction {
                    name: "get_weather".to_string(),
                    description: "Get weather".to_string(),
                    parameters: json!({"type": "object"}),
                },
                provider_options: None,
            }]),
            ..Default::default()
        };

        let result = to_minimax_request(&req, false);
        assert!(result.tools.is_some());
        let tools = result.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_temperature_clamped_in_request() {
        use crate::types::GenerateOptions;

        let mut req = make_request(vec![Message::new(Role::User, "Hello")]);
        req.options = GenerateOptions {
            temperature: Some(0.0),
            ..Default::default()
        };

        let result = to_minimax_request(&req, false);
        assert_eq!(result.temperature, Some(0.01));
    }
}
