//! Conversion between unified types and Anthropic types

use super::types::{
    infer_max_tokens, AnthropicContent, AnthropicMessage, AnthropicMessageContent,
    AnthropicRequest, AnthropicResponse, AnthropicSource,
    AnthropicThinkingConfig as AnthropicThinking,
};
use crate::error::{Error, Result};
use crate::types::{
    ContentPart, FinishReason, FinishReasonKind, GenerateRequest, GenerateResponse,
    InputTokenDetails, Message, OutputTokenDetails, ResponseContent, Role, Usage,
};
use serde_json::json;

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
        Some(AnthropicMessageContent::String(
            system_messages.join("\n\n"),
        ))
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

    // Convert thinking config from provider options to Anthropic format
    let thinking = req.provider_options.as_ref().and_then(|opts| {
        if let crate::types::ProviderOptions::Anthropic(anthropic) = opts {
            anthropic.thinking.as_ref().map(|t| AnthropicThinking {
                type_: "enabled".to_string(),
                budget_tokens: t.budget_tokens.max(1024),
            })
        } else {
            None
        }
    });

    Ok(AnthropicRequest {
        model: req.model.clone(),
        messages,
        max_tokens,
        system,
        temperature: req.options.temperature,
        top_p: req.options.top_p,
        top_k: None,
        metadata: None,
        stop_sequences: req.options.stop_sequences.clone(),
        stream: if stream { Some(true) } else { None },
        thinking,
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
        // Single content - try to use simple string format if text
        match &parts[0] {
            ContentPart::Text { text } => AnthropicMessageContent::String(text.clone()),
            _ => AnthropicMessageContent::Blocks(vec![to_anthropic_content_part(&parts[0])?]),
        }
    } else {
        // Multiple content parts - use array format
        let content_parts = parts
            .iter()
            .map(to_anthropic_content_part)
            .collect::<Result<Vec<_>>>()?;

        AnthropicMessageContent::Blocks(content_parts)
    };

    Ok(AnthropicMessage {
        role: role.to_string(),
        content,
    })
}

fn to_anthropic_content_part(part: &ContentPart) -> Result<AnthropicContent> {
    match part {
        ContentPart::Text { text } => Ok(AnthropicContent::Text { text: text.clone() }),
        ContentPart::Image { url, detail: _ } => Ok(AnthropicContent::Image {
            source: parse_image_source(url)?,
        }),
        ContentPart::ToolCall {
            id,
            name,
            arguments,
        } => Ok(AnthropicContent::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: arguments.clone(),
        }),
        ContentPart::ToolResult {
            tool_call_id,
            content,
        } => Ok(AnthropicContent::ToolResult {
            tool_use_id: tool_call_id.clone(),
            content: Some(AnthropicMessageContent::String(content.to_string())),
            is_error: None,
        }),
    }
}

/// Parse image URL to Anthropic image source format
fn parse_image_source(url: &str) -> Result<AnthropicSource> {
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

        Ok(AnthropicSource {
            type_: "base64".to_string(),
            media_type: media_type.to_string(),
            data: parts[1].to_string(),
        })
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
        .filter_map(|c| match c {
            AnthropicContent::Text { text } => Some(ResponseContent::Text { text: text.clone() }),
            AnthropicContent::Thinking { thinking, .. } => Some(ResponseContent::Reasoning {
                reasoning: thinking.clone(),
            }),
            AnthropicContent::ToolUse { id, name, input } => {
                Some(ResponseContent::ToolCall(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: input.clone(),
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
        FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_use")
    } else {
        parse_stop_reason(&resp.stop_reason)
    };

    // Calculate cache tokens following Vercel AI SDK structure
    // Anthropic: cache_creation_input_tokens -> cacheWrite, cache_read_input_tokens -> cacheRead
    let cache_creation = resp.usage.cache_creation_input_tokens.unwrap_or(0);
    let cache_read = resp.usage.cache_read_input_tokens.unwrap_or(0);
    let input_tokens = resp.usage.input_tokens;
    let output_tokens = resp.usage.output_tokens;

    let total_input = input_tokens + cache_creation + cache_read;

    let usage = Usage::with_details(
        InputTokenDetails {
            total: Some(total_input),
            no_cache: Some(input_tokens),
            cache_read: if cache_read > 0 {
                Some(cache_read)
            } else {
                None
            },
            cache_write: if cache_creation > 0 {
                Some(cache_creation)
            } else {
                None
            },
        },
        OutputTokenDetails {
            total: Some(output_tokens),
            text: None,      // Anthropic doesn't break down output tokens
            reasoning: None, // Will be populated if extended thinking is used
        },
        Some(serde_json::to_value(&resp.usage).unwrap_or_default()),
    );

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
            "cache_creation_input_tokens": resp.usage.cache_creation_input_tokens,
            "cache_read_input_tokens": resp.usage.cache_read_input_tokens,
            "stop_reason": resp.stop_reason,
        })),
    })
}

/// Parse Anthropic stop reason to unified finish reason
fn parse_stop_reason(reason: &Option<String>) -> FinishReason {
    match reason.as_deref() {
        Some("end_turn") => FinishReason::with_raw(FinishReasonKind::Stop, "end_turn"),
        Some("max_tokens") => FinishReason::with_raw(FinishReasonKind::Length, "max_tokens"),
        Some("stop_sequence") => FinishReason::with_raw(FinishReasonKind::Stop, "stop_sequence"),
        Some("tool_use") => FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_use"),
        Some(raw) => FinishReason::with_raw(FinishReasonKind::Other, raw),
        None => FinishReason::other(),
    }
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

        assert_eq!(result.type_, "base64");
        assert_eq!(result.media_type, "image/png");
        assert_eq!(result.data, "iVBORw0KGgoAAAANS");
    }
}
