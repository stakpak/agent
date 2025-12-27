//! Conversion between unified types and Anthropic types

use super::types::{AnthropicMessage, AnthropicRequest, AnthropicResponse, infer_max_tokens};
use crate::error::{Error, Result};
use crate::types::{
    ContentPart, FinishReason, GenerateRequest, GenerateResponse, Message, ResponseContent, Role,
    Usage,
};
use serde_json::{Value, json};

/// Convert unified request to Anthropic request
pub fn to_anthropic_request(req: &GenerateRequest, stream: bool) -> Result<AnthropicRequest> {
    // Extract system messages and combine them
    let system_messages: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .filter_map(|m| m.text())
        .collect();

    let system = if system_messages.is_empty() {
        None
    } else {
        Some(system_messages.join("\n\n"))
    };

    // Convert non-system messages
    let messages: Vec<AnthropicMessage> = req
        .messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(to_anthropic_message)
        .collect::<Result<Vec<_>>>()?;

    // Determine max_tokens (required by Anthropic!)
    let max_tokens = req
        .options
        .max_tokens
        .unwrap_or_else(|| infer_max_tokens(&req.model));

    // Convert tools to Anthropic format
    let tools = req.options.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.function.name,
                    "description": tool.function.description,
                    "input_schema": tool.function.parameters,
                })
            })
            .collect::<Vec<_>>()
    });

    // Convert tool_choice to Anthropic format
    let tool_choice = req.options.tool_choice.as_ref().map(|choice| match choice {
        crate::types::ToolChoice::Auto => json!({"type": "auto"}),
        crate::types::ToolChoice::None => json!({"type": "none"}),
        crate::types::ToolChoice::Required { name } => json!({
            "type": "tool",
            "name": name
        }),
    });

    Ok(AnthropicRequest {
        model: req.model.clone(),
        messages,
        max_tokens,
        system,
        temperature: req.options.temperature,
        top_p: req.options.top_p,
        stop_sequences: req.options.stop_sequences.clone(),
        stream: if stream { Some(true) } else { None },
        thinking: None, // TODO: Add reasoning support via options
        tools,
        tool_choice,
    })
}

/// Convert unified message to Anthropic message
fn to_anthropic_message(msg: &Message) -> Result<AnthropicMessage> {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => {
            return Err(Error::invalid_response(
                "System messages should be filtered out",
            ));
        }
        Role::Tool => {
            return Err(Error::invalid_response(
                "Tool messages not yet supported for Anthropic",
            ));
        }
    };

    // Convert content parts
    let parts = msg.parts();
    let content = if parts.len() == 1 {
        // Single content - use simple string format
        match &parts[0] {
            ContentPart::Text { text } => Value::String(text.clone()),
            ContentPart::Image { url, detail: _ } => {
                // Anthropic uses structured content for images
                json!([{
                    "type": "image",
                    "source": parse_image_source(url)?,
                }])
            }
            ContentPart::ToolCall {
                id,
                name,
                arguments,
            } => {
                // Tool call as structured content
                json!([{
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": arguments
                }])
            }
            ContentPart::ToolResult {
                tool_call_id,
                content,
            } => {
                // Tool result as structured content
                json!([{
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": content
                }])
            }
        }
    } else {
        // Multiple content parts - use array format
        let content_parts: Vec<Value> = parts
            .iter()
            .map(|part| match part {
                ContentPart::Text { text } => Ok(json!({
                    "type": "text",
                    "text": text
                })),
                ContentPart::Image { url, detail: _ } => Ok(json!({
                    "type": "image",
                    "source": parse_image_source(url)?
                })),
                ContentPart::ToolCall {
                    id,
                    name,
                    arguments,
                } => Ok(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": arguments
                })),
                ContentPart::ToolResult {
                    tool_call_id,
                    content,
                } => Ok(json!({
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": content
                })),
            })
            .collect::<Result<Vec<_>>>()?;

        Value::Array(content_parts)
    };

    Ok(AnthropicMessage {
        role: role.to_string(),
        content,
    })
}

/// Parse image URL to Anthropic image source format
fn parse_image_source(url: &str) -> Result<Value> {
    if url.starts_with("data:") {
        // Data URL format: data:image/png;base64,iVBORw0KG...
        let parts: Vec<&str> = url.splitn(2, ',').collect();
        if parts.len() != 2 {
            return Err(Error::invalid_response("Invalid data URL format"));
        }

        let media_type = parts[0]
            .strip_prefix("data:")
            .and_then(|s| s.strip_suffix(";base64"))
            .ok_or_else(|| Error::invalid_response("Invalid data URL media type"))?;

        Ok(json!({
            "type": "base64",
            "media_type": media_type,
            "data": parts[1]
        }))
    } else {
        // URL format (Anthropic doesn't support direct URLs, would need to fetch)
        Err(Error::invalid_response(
            "Anthropic requires base64-encoded images, not URLs",
        ))
    }
}

/// Convert Anthropic response to unified response
pub fn from_anthropic_response(resp: AnthropicResponse) -> Result<GenerateResponse> {
    use crate::types::ToolCall;

    let content: Vec<ResponseContent> = resp
        .content
        .iter()
        .filter_map(|c| match c.type_.as_str() {
            "text" => c
                .text
                .as_ref()
                .map(|t| ResponseContent::Text { text: t.clone() }),
            "thinking" => c.thinking.as_ref().map(|t| ResponseContent::Text {
                text: format!("[Thinking: {}]", t),
            }),
            "tool_use" => {
                // Anthropic tool call format
                Some(ResponseContent::ToolCall(ToolCall {
                    id: c.id.clone().unwrap_or_default(),
                    name: c.name.clone().unwrap_or_default(),
                    arguments: c.input.clone().unwrap_or(json!({})),
                }))
            }
            _ => None,
        })
        .collect();

    if content.is_empty() {
        return Err(Error::invalid_response("No content in response"));
    }

    // Determine finish reason - tool_use should be ToolCalls
    let finish_reason = if content
        .iter()
        .any(|c| matches!(c, ResponseContent::ToolCall(_)))
    {
        FinishReason::ToolCalls
    } else {
        parse_stop_reason(&resp.stop_reason).unwrap_or(FinishReason::Other)
    };

    Ok(GenerateResponse {
        content,
        usage: Usage {
            prompt_tokens: resp.usage.input_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
        },
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
        })),
    })
}

/// Parse Anthropic stop reason to unified finish reason
fn parse_stop_reason(reason: &Option<String>) -> Option<FinishReason> {
    reason.as_ref().and_then(|r| match r.as_str() {
        "end_turn" => Some(FinishReason::Stop),
        "max_tokens" => Some(FinishReason::Length),
        "stop_sequence" => Some(FinishReason::Stop),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_max_tokens() {
        assert_eq!(infer_max_tokens("claude-opus-4-5"), 64000);
        assert_eq!(infer_max_tokens("claude-sonnet-4"), 64000);
        assert_eq!(infer_max_tokens("claude-opus-4"), 32000);
        assert_eq!(infer_max_tokens("claude-3-5-sonnet"), 8192);
        assert_eq!(infer_max_tokens("claude-3-opus"), 4096);
    }

    #[test]
    fn test_parse_image_source() {
        let data_url = "data:image/png;base64,iVBORw0KGgoAAAANS";
        let result = parse_image_source(data_url).unwrap();

        assert_eq!(result["type"], "base64");
        assert_eq!(result["media_type"], "image/png");
        assert_eq!(result["data"], "iVBORw0KGgoAAAANS");
    }
}
