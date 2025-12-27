//! Conversion between SDK types and OpenAI types

use super::types::*;
use crate::error::{Error, Result};
use crate::types::*;
use serde_json::json;

/// Convert SDK request to OpenAI request
pub fn to_openai_request(req: &GenerateRequest, stream: bool) -> ChatCompletionRequest {
    // Convert tools to OpenAI format
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

    // Convert tool_choice to OpenAI format
    let tool_choice = req.options.tool_choice.as_ref().map(|choice| match choice {
        crate::types::ToolChoice::Auto => json!("auto"),
        crate::types::ToolChoice::None => json!("none"),
        crate::types::ToolChoice::Required { name } => json!({
            "type": "function",
            "function": { "name": name }
        }),
    });

    ChatCompletionRequest {
        model: req.model.clone(),
        messages: req.messages.iter().map(to_openai_message).collect(),
        temperature: req.options.temperature,
        max_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stop: req.options.stop_sequences.clone(),
        stream: Some(stream),
        tools,
        tool_choice,
    }
}

/// Convert SDK message to OpenAI message
fn to_openai_message(msg: &Message) -> ChatMessage {
    let role = match msg.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };

    // Get content parts from the message
    let parts = msg.parts();

    // Check if this is a tool result message
    let tool_call_id = parts.iter().find_map(|part| match part {
        ContentPart::ToolResult { tool_call_id, .. } => Some(tool_call_id.clone()),
        _ => None,
    });

    // Check if this message contains tool calls
    let tool_calls = parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::ToolCall {
                id,
                name,
                arguments,
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
        .collect::<Vec<_>>();

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    let content = if parts.len() == 1 {
        // Single content part - use string format
        match &parts[0] {
            ContentPart::Text { text } => Some(json!(text)),
            ContentPart::Image { url, detail } => Some(json!([{
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
            ContentPart::ToolCall { .. } => None, // Handled via tool_calls field
            ContentPart::ToolResult { content, .. } => Some(content.clone()),
        }
    } else {
        // Multiple content parts - use array format
        Some(json!(
            parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(json!({
                        "type": "text",
                        "text": text
                    })),
                    ContentPart::Image { url, detail } => Some(json!({
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
                    ContentPart::ToolResult { .. } => None, // Handled separately via tool_call_id
                })
                .collect::<Vec<_>>()
        ))
    };

    ChatMessage {
        role: role.to_string(),
        content,
        name: msg.name.clone(),
        tool_calls,
        tool_call_id,
    }
}

/// Convert OpenAI response to SDK response
pub fn from_openai_response(resp: ChatCompletionResponse) -> Result<GenerateResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| Error::invalid_response("No choices in response"))?;

    let content = parse_message_content(&choice.message)?;

    let finish_reason = match choice.finish_reason.as_deref() {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("content_filter") => FinishReason::ContentFilter,
        Some("tool_calls") => FinishReason::ToolCalls,
        _ => FinishReason::Other,
    };

    Ok(GenerateResponse {
        content,
        usage: Usage {
            prompt_tokens: resp.usage.prompt_tokens,
            completion_tokens: resp.usage.completion_tokens,
            total_tokens: resp.usage.total_tokens,
        },
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
            "created": resp.created,
        })),
    })
}

/// Parse message content from OpenAI format
fn parse_message_content(msg: &ChatMessage) -> Result<Vec<ResponseContent>> {
    let mut content = Vec::new();

    // Handle string content
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
            }));
        }
    }

    Ok(content)
}
