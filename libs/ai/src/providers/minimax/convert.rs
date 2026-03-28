//! Conversion from SDK types to MiniMax's OpenAI-compatible request format
//!
//! MiniMax's API is OpenAI-compatible, so we reuse the OpenAI wire types.
//! Key difference: temperature must be in the range (0.0, 1.0].

use crate::providers::openai::types::{
    ChatCompletionRequest, ChatMessage, OpenAIFunctionCall, OpenAIToolCall, StreamOptions,
};
use crate::types::{ContentPart, GenerateRequest, ImageDetail, Message, Role};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MessageContent, Model};

    fn make_request(messages: Vec<Message>) -> GenerateRequest {
        GenerateRequest::new(
            Model::custom("MiniMax-M2.7", "minimax"),
            messages,
        )
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
