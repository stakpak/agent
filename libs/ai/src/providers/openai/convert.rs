//! Conversion between SDK types and OpenAI types

use super::types::*;
use crate::error::{Error, Result};
use crate::types::{
    ContentPart, FinishReason, FinishReasonKind, GenerateRequest, GenerateResponse, ImageDetail,
    InputTokenDetails, Message, OutputTokenDetails, ProviderOptions, ResponseContent, Role,
    SystemMessageMode, ToolCall, Usage,
};
use serde_json::json;

/// Check if a model is a reasoning model (o1, o3, o4, gpt-5)
fn is_reasoning_model(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.starts_with("gpt-5")
}

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

    // Determine system message mode
    // Default: for reasoning models, convert system to developer; otherwise keep as system
    let system_message_mode = req
        .provider_options
        .as_ref()
        .and_then(|opts| {
            if let ProviderOptions::OpenAI(openai_opts) = opts {
                openai_opts.system_message_mode
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            if is_reasoning_model(&req.model.id) {
                SystemMessageMode::Developer
            } else {
                SystemMessageMode::System
            }
        });

    // Convert messages with system message mode handling
    let messages: Vec<ChatMessage> = req
        .messages
        .iter()
        .filter_map(|msg| to_openai_message_with_mode(msg, system_message_mode))
        .collect();

    let temp = match is_reasoning_model(&req.model.id) {
        false => Some(0.0),
        true => None,
    };

    ChatCompletionRequest {
        model: req.model.id.clone(),
        messages,
        temperature: temp,
        max_completion_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stop: req.options.stop_sequences.clone(),
        stream: Some(stream),
        tools,
        tool_choice,
    }
}

/// Convert SDK message to OpenAI message with system message mode handling
fn to_openai_message_with_mode(msg: &Message, mode: SystemMessageMode) -> Option<ChatMessage> {
    let role = match msg.role {
        Role::System => {
            match mode {
                SystemMessageMode::System => "system",
                SystemMessageMode::Developer => "developer",
                SystemMessageMode::Remove => return None, // Skip system messages
            }
        }
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
        .collect::<Vec<_>>();

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    let content = if parts.len() == 1 {
        // Single content part - use string format
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
            ContentPart::ToolCall { .. } => None, // Handled via tool_calls field
            ContentPart::ToolResult { content, .. } => Some(content.clone()),
        }
    } else {
        // Multiple content parts - use array format
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
                    ContentPart::ToolResult { .. } => None, // Handled separately via tool_call_id
                })
                .collect::<Vec<_>>()
        ))
    };

    Some(ChatMessage {
        role: role.to_string(),
        content,
        name: msg.name.clone(),
        tool_calls,
        tool_call_id,
    })
}

/// Convert OpenAI response to SDK response
pub fn from_openai_response(resp: ChatCompletionResponse) -> Result<GenerateResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| Error::invalid_response("No choices in response"))?;

    let content = parse_message_content(&choice.message)?;

    let finish_reason = parse_openai_finish_reason(choice.finish_reason.as_deref());

    // OpenAI: prompt_tokens_details.cached_tokens -> cacheRead (OpenAI doesn't report cacheWrite)
    let prompt_tokens = resp.usage.prompt_tokens;
    let completion_tokens = resp.usage.completion_tokens;

    let cached_tokens = resp
        .usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);

    let reasoning_tokens = resp
        .usage
        .completion_tokens_details
        .as_ref()
        .and_then(|d| d.reasoning_tokens);

    let usage = Usage::with_details(
        InputTokenDetails {
            total: Some(prompt_tokens),
            no_cache: Some(prompt_tokens.saturating_sub(cached_tokens)),
            cache_read: if cached_tokens > 0 {
                Some(cached_tokens)
            } else {
                None
            },
            cache_write: None, // OpenAI doesn't report cache writes
        },
        OutputTokenDetails {
            total: Some(completion_tokens),
            text: reasoning_tokens.map(|r| completion_tokens.saturating_sub(r)),
            reasoning: reasoning_tokens,
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
            "created": resp.created,
            "object": resp.object,
        })),
        warnings: None, // OpenAI caching is automatic, no SDK-level validation warnings
    })
}

/// Parse OpenAI finish reason to unified finish reason
fn parse_openai_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
        Some("length") => FinishReason::with_raw(FinishReasonKind::Length, "length"),
        Some("content_filter") => {
            FinishReason::with_raw(FinishReasonKind::ContentFilter, "content_filter")
        }
        Some("tool_calls") => FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_calls"),
        Some("function_call") => {
            FinishReason::with_raw(FinishReasonKind::ToolCalls, "function_call")
        }
        Some(raw) => FinishReason::with_raw(FinishReasonKind::Other, raw),
        None => FinishReason::other(),
    }
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
