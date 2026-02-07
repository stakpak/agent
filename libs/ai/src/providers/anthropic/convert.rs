//! Conversion between unified types and Anthropic types

use super::types::{
    AnthropicAuth, AnthropicCacheControl, AnthropicConfig, AnthropicContent, AnthropicMessage,
    AnthropicMessageContent, AnthropicRequest, AnthropicResponse, AnthropicSource,
    AnthropicSystemBlock, AnthropicSystemContent, AnthropicThinkingConfig as AnthropicThinking,
    CLAUDE_CODE_SYSTEM_PREFIX, infer_max_tokens,
};
use crate::error::{Error, Result};
use crate::types::{
    CacheContext, CacheControlValidator, CacheWarning, ContentPart, FinishReason, FinishReasonKind,
    GenerateRequest, GenerateResponse, InputTokenDetails, Message, OutputTokenDetails,
    ResponseContent, Role, Usage,
};
use serde_json::json;

/// Result of converting a request to Anthropic format
pub struct AnthropicConversionResult {
    /// The converted request
    pub request: AnthropicRequest,
    /// Warnings generated during conversion (e.g., cache validation)
    pub warnings: Vec<CacheWarning>,
    /// Whether any cache control was used (to determine if beta header is needed)
    pub has_cache_control: bool,
}

/// Convert unified request to Anthropic request with smart caching
///
/// This function applies the caching strategy from the request options,
/// falling back to the provider's default strategy if not specified.
pub fn to_anthropic_request(
    req: &GenerateRequest,
    config: &AnthropicConfig,
    stream: bool,
) -> Result<AnthropicConversionResult> {
    let mut validator = CacheControlValidator::new();

    // Determine the effective caching strategy:
    // 1. Request-level strategy takes precedence
    // 2. Fall back to provider default
    let cache_strategy = req
        .options
        .cache_strategy
        .clone()
        .unwrap_or_else(|| config.default_cache_strategy.clone());

    let cache_config = cache_strategy.to_anthropic_config();

    // Check if we have tools (for cache budget calculation)
    let has_tools = req.options.tools.as_ref().is_some_and(|t| !t.is_empty());

    // Build tools with smart caching (cache last tool)
    let tools = build_tools_with_caching(
        &req.options.tools,
        &mut validator,
        cache_config
            .as_ref()
            .is_some_and(|c| c.cache_tools && has_tools),
    )?;

    // Extract and convert system messages with smart caching
    let system = build_system_content_with_caching(
        &req.messages,
        &config.auth,
        &mut validator,
        cache_config.as_ref().is_some_and(|c| c.cache_system),
    )?;

    // Calculate remaining budget for tail messages
    let tail_budget = cache_config.as_ref().map_or(0, |c| {
        let used = validator.breakpoint_count();
        let max = 4usize; // Anthropic limit
        let remaining = max.saturating_sub(used);
        c.tail_message_count.min(remaining)
    });

    // Convert non-system messages with smart tail caching
    let messages = build_messages_with_caching(&req.messages, &mut validator, tail_budget)?;

    // Determine max_tokens (required by Anthropic!)
    let max_tokens = req
        .options
        .max_tokens
        .unwrap_or_else(|| infer_max_tokens(&req.model.id));

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

    let has_cache_control = validator.breakpoint_count() > 0;
    let warnings = validator.take_warnings();

    Ok(AnthropicConversionResult {
        request: AnthropicRequest {
            model: req.model.id.clone(),
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
        },
        warnings,
        has_cache_control,
    })
}

/// Build system content with smart caching and OAuth handling
///
/// When `auto_cache_last` is true, the last system block gets a cache breakpoint.
/// This caches ALL system messages (Anthropic caches the full prefix up to the breakpoint).
fn build_system_content_with_caching(
    messages: &[Message],
    auth: &AnthropicAuth,
    validator: &mut CacheControlValidator,
    auto_cache_last: bool,
) -> Result<Option<AnthropicSystemContent>> {
    let system_messages: Vec<&Message> =
        messages.iter().filter(|m| m.role == Role::System).collect();

    // For OAuth, we need the Claude Code prefix
    let is_oauth = matches!(auth, AnthropicAuth::OAuth { .. });

    if system_messages.is_empty() && !is_oauth {
        return Ok(None);
    }

    // Check if any system message has explicit cache control
    let has_explicit_cache = system_messages.iter().any(|m| m.cache_control().is_some());

    // Determine if we should use blocks format
    let use_blocks = is_oauth || has_explicit_cache || auto_cache_last;

    // For OAuth, always use blocks format with Claude Code prefix
    if is_oauth {
        let mut blocks = vec![];

        // Add Claude Code prefix with 1-hour cache
        blocks.push(AnthropicSystemBlock {
            type_: "text".to_string(),
            text: CLAUDE_CODE_SYSTEM_PREFIX.to_string(),
            cache_control: Some(AnthropicCacheControl::ephemeral_with_ttl("1h")),
        });
        // Count this as a cache breakpoint
        validator.validate(
            Some(&crate::types::CacheControl::ephemeral_with_ttl("1h")),
            CacheContext::system_message(),
        );

        // Add user system messages
        let msg_count = system_messages.len();
        for (i, msg) in system_messages.iter().enumerate() {
            if let Some(text) = msg.text() {
                let is_last = i == msg_count - 1;

                // Use explicit cache or auto-cache last with 1-hour TTL
                let cache_control = msg.cache_control().cloned().or_else(|| {
                    if is_last && auto_cache_last {
                        Some(crate::types::CacheControl::ephemeral_with_ttl("1h"))
                    } else {
                        None
                    }
                });

                let validated_cache =
                    validator.validate(cache_control.as_ref(), CacheContext::system_message());

                blocks.push(AnthropicSystemBlock {
                    type_: "text".to_string(),
                    text,
                    cache_control: validated_cache.map(|c| AnthropicCacheControl::from(&c)),
                });
            }
        }

        return Ok(Some(AnthropicSystemContent::Blocks(blocks)));
    }

    // For API key auth without any caching, use simple string format
    if !use_blocks {
        let combined = system_messages
            .iter()
            .filter_map(|m| m.text())
            .collect::<Vec<_>>()
            .join("\n\n");
        return Ok(Some(AnthropicSystemContent::String(combined)));
    }

    // Complex case: caching needed, use blocks format
    let msg_count = system_messages.len();
    let blocks: Vec<AnthropicSystemBlock> = system_messages
        .iter()
        .enumerate()
        .filter_map(|(i, msg)| {
            let text = msg.text()?;
            let is_last = i == msg_count - 1;

            // Use explicit cache or auto-cache last with 1-hour TTL
            let cache_control = msg.cache_control().cloned().or_else(|| {
                if is_last && auto_cache_last {
                    Some(crate::types::CacheControl::ephemeral_with_ttl("1h"))
                } else {
                    None
                }
            });

            let validated_cache =
                validator.validate(cache_control.as_ref(), CacheContext::system_message());

            Some(AnthropicSystemBlock {
                type_: "text".to_string(),
                text,
                cache_control: validated_cache.map(|c| AnthropicCacheControl::from(&c)),
            })
        })
        .collect();

    if blocks.is_empty() {
        Ok(None)
    } else {
        Ok(Some(AnthropicSystemContent::Blocks(blocks)))
    }
}

/// Build tools with smart caching on the last tool
///
/// When `auto_cache_last` is true, the last tool gets a cache breakpoint.
/// This caches ALL tools as a group (Anthropic caches the full prefix).
fn build_tools_with_caching(
    tools: &Option<Vec<crate::types::Tool>>,
    validator: &mut CacheControlValidator,
    auto_cache_last: bool,
) -> Result<Option<Vec<serde_json::Value>>> {
    let tools = match tools {
        Some(t) if !t.is_empty() => t,
        _ => return Ok(None),
    };

    let len = tools.len();
    let converted: Vec<serde_json::Value> = tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            let is_last = i == len - 1;

            // Use explicit cache_control if set, otherwise auto-cache last tool with 1h TTL
            let cache_control = tool.cache_control().cloned().or_else(|| {
                if is_last && auto_cache_last {
                    Some(crate::types::CacheControl::ephemeral_with_ttl("1h"))
                } else {
                    None
                }
            });

            let validated_cache =
                validator.validate(cache_control.as_ref(), CacheContext::tool_definition());

            let mut tool_json = json!({
                "name": tool.function.name,
                "description": tool.function.description,
                "input_schema": tool.function.parameters,
            });

            if let Some(cache) = validated_cache {
                tool_json["cache_control"] = json!(AnthropicCacheControl::from(&cache));
            }

            tool_json
        })
        .collect();

    Ok(Some(converted))
}

/// Build messages with smart tail caching
///
/// Caches the last N non-system messages to maximize cache hits
/// on subsequent requests in a conversation.
///
/// Also merges consecutive Tool messages into a single user message with multiple
/// tool_result blocks, since Anthropic requires each tool_result to have a
/// corresponding tool_use in the *immediately preceding* assistant message.
/// Separate Tool messages would make the 2nd+ tool_result reference a user message
/// (the previous tool result) instead of the assistant, causing API errors.
fn build_messages_with_caching(
    messages: &[Message],
    validator: &mut CacheControlValidator,
    tail_count: usize,
) -> Result<Vec<AnthropicMessage>> {
    let non_system: Vec<&Message> = messages.iter().filter(|m| m.role != Role::System).collect();
    let merged = merge_consecutive_tool_messages(&non_system);

    let len = merged.len();
    let cache_start_index = len.saturating_sub(tail_count);

    merged
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let should_auto_cache = tail_count > 0 && i >= cache_start_index;
            to_anthropic_message_with_caching(msg, validator, should_auto_cache)
        })
        .collect()
}

/// Merge consecutive Tool messages into single messages.
///
/// Anthropic requires: each tool_result block must have a corresponding tool_use
/// in the *previous* message. With separate Tool messages (assistant → tool_A →
/// tool_B), the "previous" message for tool_B is tool_A (a user message with
/// tool_result), not the assistant—causing "unexpected tool_use_id" errors.
///
/// We merge consecutive Tool messages so we get: assistant → user(tool_A, tool_B).
fn merge_consecutive_tool_messages(messages: &[&Message]) -> Vec<Message> {
    use crate::types::{ContentPart, MessageContent};

    let mut result = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let msg = messages[i];
        if msg.role != Role::Tool {
            result.push((*msg).clone());
            i += 1;
            continue;
        }

        // Collect all consecutive Tool messages and merge their tool_result parts
        let mut tool_result_parts: Vec<ContentPart> = Vec::new();
        while i < messages.len() && messages[i].role == Role::Tool {
            let parts = messages[i].parts();
            for part in parts {
                if let ContentPart::ToolResult { .. } = &part {
                    tool_result_parts.push(part);
                }
            }
            i += 1;
        }

        if !tool_result_parts.is_empty() {
            result.push(Message::new(
                Role::Tool,
                MessageContent::Parts(tool_result_parts),
            ));
        }
    }

    result
}

/// Convert unified message to Anthropic message with optional auto-caching
fn to_anthropic_message_with_caching(
    msg: &Message,
    validator: &mut CacheControlValidator,
    auto_cache: bool,
) -> Result<AnthropicMessage> {
    // Determine the Anthropic role - Tool messages become "user" with tool_result content
    // (Anthropic doesn't support role="tool" like OpenAI)
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "user", // Anthropic expects tool results as user messages
        Role::System => {
            return Err(Error::invalid_response(
                "System messages should be filtered out",
            ));
        }
    };

    // Get the message-level cache control, or use auto-cache
    let msg_cache_control = msg.cache_control().cloned().or_else(|| {
        if auto_cache {
            Some(crate::types::CacheControl::ephemeral())
        } else {
            None
        }
    });

    // Convert content parts
    let parts = msg.parts();

    // Check if any part has cache control, or if message has cache control
    let has_cache_control =
        msg_cache_control.is_some() || parts.iter().any(|p| p.cache_control().is_some());

    // Tool messages always use blocks format (tool_result content blocks)
    let force_blocks = msg.role == Role::Tool;

    let content = if parts.len() == 1 && !has_cache_control && !force_blocks {
        // Single content without cache control - try to use simple string format if text
        match &parts[0] {
            ContentPart::Text { text, .. } => AnthropicMessageContent::String(text.clone()),
            _ => AnthropicMessageContent::Blocks(vec![to_anthropic_content_part(
                &parts[0], None, validator, true,
            )?]),
        }
    } else {
        // Multiple content parts, has cache control, or tool message - use array format
        let num_parts = parts.len();
        let content_parts = parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                let is_last = i == num_parts - 1;
                // For the last part, include message-level cache control as fallback
                let fallback_cache = if is_last {
                    msg_cache_control.as_ref()
                } else {
                    None
                };
                to_anthropic_content_part(part, fallback_cache, validator, is_last)
            })
            .collect::<Result<Vec<_>>>()?;

        AnthropicMessageContent::Blocks(content_parts)
    };

    Ok(AnthropicMessage {
        role: role.to_string(),
        content,
    })
}

/// Convert a single message to Anthropic format (test helper, no auto-caching)
#[cfg(test)]
fn to_anthropic_message(
    msg: &Message,
    validator: &mut CacheControlValidator,
) -> Result<AnthropicMessage> {
    to_anthropic_message_with_caching(msg, validator, false)
}

/// Convert a content part to Anthropic format with cache control
fn to_anthropic_content_part(
    part: &ContentPart,
    fallback_cache: Option<&crate::types::CacheControl>,
    validator: &mut CacheControlValidator,
    is_last_part: bool,
) -> Result<AnthropicContent> {
    // Get the part-level cache control, with fallback to message-level for last part
    let part_cache = part.cache_control();
    let effective_cache = if part_cache.is_some() {
        part_cache
    } else if is_last_part {
        fallback_cache
    } else {
        None
    };

    match part {
        ContentPart::Text { text, .. } => {
            let context = CacheContext::user_message_part();
            let validated_cache = validator.validate(effective_cache, context);

            Ok(AnthropicContent::Text {
                text: text.clone(),
                cache_control: validated_cache.map(|c| AnthropicCacheControl::from(&c)),
            })
        }
        ContentPart::Image { url, .. } => {
            let context = CacheContext::image_content();
            let validated_cache = validator.validate(effective_cache, context);

            Ok(AnthropicContent::Image {
                source: parse_image_source(url)?,
                cache_control: validated_cache.map(|c| AnthropicCacheControl::from(&c)),
            })
        }
        ContentPart::ToolCall {
            id,
            name,
            arguments,
            ..
        } => {
            let context = CacheContext::assistant_message_part();
            let validated_cache = validator.validate(effective_cache, context);

            Ok(AnthropicContent::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: arguments.clone(),
                cache_control: validated_cache.map(|c| AnthropicCacheControl::from(&c)),
            })
        }
        ContentPart::ToolResult {
            tool_call_id,
            content,
            ..
        } => {
            let context = CacheContext::tool_result();
            let validated_cache = validator.validate(effective_cache, context);

            Ok(AnthropicContent::ToolResult {
                tool_use_id: tool_call_id.clone(),
                content: Some(AnthropicMessageContent::String(content.to_string())),
                is_error: None,
                cache_control: validated_cache.map(|c| AnthropicCacheControl::from(&c)),
            })
        }
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

/// Convert Anthropic response to unified response with warnings from conversion
pub fn from_anthropic_response_with_warnings(
    resp: AnthropicResponse,
    warnings: Vec<CacheWarning>,
) -> Result<GenerateResponse> {
    use crate::types::{ResponseWarning, ToolCall};

    let content: Vec<ResponseContent> = resp
        .content
        .iter()
        .filter_map(|c| match c {
            AnthropicContent::Text { text, .. } => {
                Some(ResponseContent::Text { text: text.clone() })
            }
            AnthropicContent::Thinking { thinking, .. } => Some(ResponseContent::Reasoning {
                reasoning: thinking.clone(),
            }),
            AnthropicContent::ToolUse {
                id, name, input, ..
            } => Some(ResponseContent::ToolCall(ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: input.clone(),
                metadata: None,
            })),
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

    // Calculate cache tokens
    // Anthropic token breakdown (per official API docs):
    // - input_tokens: tokens NOT read from or written to cache (non-cached input)
    // - cache_creation_input_tokens: tokens written to cache (cache miss, creating entry)
    // - cache_read_input_tokens: tokens read from cache (cache hit)
    // Total input = non-cached + cache-write + cache-read
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

    // Convert cache warnings to response warnings
    let response_warnings: Option<Vec<ResponseWarning>> = if warnings.is_empty() {
        None
    } else {
        Some(warnings.into_iter().map(ResponseWarning::from).collect())
    };

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
        })),
        warnings: response_warnings,
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
    use crate::types::MessageContent;

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

    #[test]
    fn test_tool_role_message_converted_to_user_with_tool_result() {
        // Test that Role::Tool messages are converted to Anthropic's expected format:
        // role="user" with tool_result content blocks
        // This is critical for Anthropic compatibility - they don't support role="tool"
        let mut validator = CacheControlValidator::new();

        let tool_msg = Message {
            role: Role::Tool,
            content: MessageContent::Parts(vec![ContentPart::ToolResult {
                tool_call_id: "toolu_01Abc123".to_string(),
                content: serde_json::json!("Tool execution result"),
                provider_options: None,
            }]),
            name: None,
            provider_options: None,
        };

        let result = to_anthropic_message(&tool_msg, &mut validator).unwrap();

        // Role should be converted to "user" for Anthropic
        assert_eq!(
            result.role, "user",
            "Tool role should be converted to user for Anthropic"
        );

        // Content should contain tool_result block
        match result.content {
            AnthropicMessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1, "Should have exactly one content block");
                match &blocks[0] {
                    AnthropicContent::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        assert_eq!(tool_use_id, "toolu_01Abc123");
                        // Content should be the stringified JSON
                        match content {
                            Some(AnthropicMessageContent::String(s)) => {
                                assert_eq!(s, "\"Tool execution result\"");
                            }
                            _ => panic!("Expected string content in tool result"),
                        }
                    }
                    _ => panic!("Expected ToolResult content block, got {:?}", blocks[0]),
                }
            }
            _ => panic!("Expected Blocks content, got {:?}", result.content),
        }
    }

    #[test]
    fn test_tool_role_message_with_text_content() {
        // Test tool result with plain text content (common case)
        let mut validator = CacheControlValidator::new();

        let tool_msg = Message {
            role: Role::Tool,
            content: MessageContent::Parts(vec![ContentPart::ToolResult {
                tool_call_id: "toolu_02Xyz789".to_string(),
                content: serde_json::json!({"temperature": 22, "unit": "celsius"}),
                provider_options: None,
            }]),
            name: None,
            provider_options: None,
        };

        let result = to_anthropic_message(&tool_msg, &mut validator).unwrap();

        assert_eq!(result.role, "user");
        match result.content {
            AnthropicMessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AnthropicContent::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        assert_eq!(tool_use_id, "toolu_02Xyz789");
                        match content {
                            Some(AnthropicMessageContent::String(s)) => {
                                // JSON object should be stringified
                                assert!(s.contains("temperature"));
                                assert!(s.contains("22"));
                            }
                            _ => panic!("Expected string content"),
                        }
                    }
                    _ => panic!("Expected ToolResult"),
                }
            }
            _ => panic!("Expected Blocks"),
        }
    }

    #[test]
    fn test_assistant_message_not_affected_by_tool_conversion() {
        // Ensure assistant messages are not affected by the tool role handling
        let mut validator = CacheControlValidator::new();

        let assistant_msg = Message {
            role: Role::Assistant,
            content: MessageContent::Text("I'll help you with that.".to_string()),
            name: None,
            provider_options: None,
        };

        let result = to_anthropic_message(&assistant_msg, &mut validator).unwrap();

        assert_eq!(result.role, "assistant");
        match result.content {
            AnthropicMessageContent::String(s) => {
                assert_eq!(s, "I'll help you with that.");
            }
            _ => panic!("Expected string content for simple assistant message"),
        }
    }

    #[test]
    fn test_user_message_not_affected_by_tool_conversion() {
        // Ensure user messages are not affected by the tool role handling
        let mut validator = CacheControlValidator::new();

        let user_msg = Message {
            role: Role::User,
            content: MessageContent::Text("Hello!".to_string()),
            name: None,
            provider_options: None,
        };

        let result = to_anthropic_message(&user_msg, &mut validator).unwrap();

        assert_eq!(result.role, "user");
        match result.content {
            AnthropicMessageContent::String(s) => {
                assert_eq!(s, "Hello!");
            }
            _ => panic!("Expected string content for simple user message"),
        }
    }

    #[test]
    fn test_consecutive_tool_messages_merged_for_anthropic() {
        // Anthropic requires tool_results in a single user message following the assistant.
        // Separate Tool messages cause "unexpected tool_use_id" errors because the 2nd+
        // tool_result's "previous message" would be another tool result, not the assistant.
        use crate::types::{GenerateRequest, MessageContent};
        use crate::Model;

        let messages = vec![
            Message::new(
                Role::Assistant,
                MessageContent::Parts(vec![
                    ContentPart::tool_call("toolu_1", "tool_a", serde_json::json!({})),
                    ContentPart::tool_call("toolu_2", "tool_b", serde_json::json!({})),
                ]),
            ),
            Message::new(
                Role::Tool,
                MessageContent::Parts(vec![ContentPart::tool_result(
                    "toolu_1",
                    serde_json::json!("result_a"),
                )]),
            ),
            Message::new(
                Role::Tool,
                MessageContent::Parts(vec![ContentPart::tool_result(
                    "toolu_2",
                    serde_json::json!("result_b"),
                )]),
            ),
        ];

        let req = GenerateRequest::new(
            Model::custom("claude-3-5-sonnet-20241022", "anthropic"),
            messages,
        );

        let config = super::super::AnthropicConfig::default();
        let result = to_anthropic_request(&req, &config, false).unwrap();

        // Should have 3 messages: assistant, user (merged tool results)
        assert_eq!(result.request.messages.len(), 2, "Consecutive tool messages should be merged");
        assert_eq!(result.request.messages[0].role, "assistant");
        assert_eq!(result.request.messages[1].role, "user");

        // The merged user message should have 2 tool_result blocks
        match &result.request.messages[1].content {
            AnthropicMessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2, "Should have 2 tool_result blocks");
                if let AnthropicContent::ToolResult { tool_use_id, .. } = &blocks[0] {
                    assert_eq!(tool_use_id, "toolu_1");
                } else {
                    panic!("First block should be ToolResult");
                }
                if let AnthropicContent::ToolResult { tool_use_id, .. } = &blocks[1] {
                    assert_eq!(tool_use_id, "toolu_2");
                } else {
                    panic!("Second block should be ToolResult");
                }
            }
            _ => panic!("Expected Blocks content with merged tool results"),
        }
    }
}
