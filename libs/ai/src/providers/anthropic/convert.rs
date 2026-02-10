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
use std::collections::HashSet;

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
/// Tail caching runs **last** — after all structural mutations (merging,
/// per-message sanitization, and sequence-level sanitization) are complete.
/// This guarantees cache breakpoints land on the final stable message
/// boundaries, preventing stale breakpoints from messages that get
/// inserted, removed, or re-merged by sanitization phases.
fn build_messages_with_caching(
    messages: &[Message],
    validator: &mut CacheControlValidator,
    tail_count: usize,
) -> Result<Vec<AnthropicMessage>> {
    let non_system: Vec<&Message> = messages.iter().filter(|m| m.role != Role::System).collect();

    // Phase 1: Convert each message individually (no auto-caching yet)
    let converted: Vec<AnthropicMessage> = non_system
        .iter()
        .map(|msg| to_anthropic_message_with_caching(msg, validator, false))
        .collect::<Result<Vec<_>>>()?;

    // Phase 2: Merge consecutive same-role messages
    let mut merged = merge_consecutive_messages(converted);

    // Phase 3: Sanitize individual messages to enforce per-message constraints.
    // Runs before sequence sanitization so that empty text blocks are removed
    // before tool-pairing logic inspects message content.
    for msg in &mut merged {
        sanitize_anthropic_message(msg);
    }

    // Phase 4: Enforce message-sequence-level Anthropic constraints.
    // This handles structural invariants that span multiple messages:
    // - Every tool_use must have a matching tool_result in the next user message
    // - Orphan tool_results without matching tool_use are removed
    // - Conversation must start with a user message
    // - Conversation must not end with an assistant message (unless prefill-safe)
    //
    // This phase can insert, remove, and re-merge messages, so caching
    // must run after it to avoid stale breakpoint placement.
    sanitize_message_sequence(&mut merged);

    // Phase 5: Apply tail caching to the last N messages of the *final* array.
    // Running after all mutations ensures breakpoints land on stable positions
    // and won't be shifted by later inserts/removes/re-merges.
    if tail_count > 0 {
        let len = merged.len();
        let cache_start = len.saturating_sub(tail_count);
        for msg in &mut merged[cache_start..] {
            if !is_empty_content_message(msg) {
                apply_tail_cache_to_message(msg, validator);
            }
        }
    }

    Ok(merged)
}

/// Apply ephemeral cache control to the last content block of a message.
///
/// Used for tail-caching after message merging to ensure cache breakpoints
/// land on the actual last block of each merged message.
fn apply_tail_cache_to_message(msg: &mut AnthropicMessage, validator: &mut CacheControlValidator) {
    let cache = crate::types::CacheControl::ephemeral();
    let context = if msg.role == "assistant" {
        CacheContext::assistant_message_part()
    } else {
        CacheContext::user_message_part()
    };

    let Some(validated_cache) = validator.validate(Some(&cache), context) else {
        return; // Breakpoint limit exceeded
    };

    let anthropic_cc = AnthropicCacheControl::from(&validated_cache);
    match &mut msg.content {
        AnthropicMessageContent::Blocks(blocks) => {
            if let Some(last) = blocks.last_mut() {
                set_block_cache_control(last, Some(anthropic_cc));
            }
        }
        AnthropicMessageContent::String(s) => {
            // Convert to blocks format to attach cache control
            msg.content = AnthropicMessageContent::Blocks(vec![AnthropicContent::Text {
                text: std::mem::take(s),
                cache_control: Some(anthropic_cc),
            }]);
        }
    }
}

/// Returns true if the message contains only empty text content (no cacheable substance).
///
/// Used to skip tail-caching on messages that would waste a cache breakpoint,
/// since Phase 4 would strip the `cache_control` from empty text blocks anyway.
fn is_empty_content_message(msg: &AnthropicMessage) -> bool {
    match &msg.content {
        AnthropicMessageContent::String(s) => s.is_empty(),
        AnthropicMessageContent::Blocks(blocks) => blocks
            .iter()
            .all(|b| matches!(b, AnthropicContent::Text { text, .. } if text.is_empty())),
    }
}

/// Sanitize an Anthropic message to enforce per-message API constraints.
///
/// This is the **single boundary** that fixes structural issues before the
/// message is sent to the API. All Anthropic-specific content invariants
/// are enforced here, rather than scattering guards across conversion,
/// merging, and caching phases.
///
/// Rules (validated against live API + informed by Vercel AI SDK / OpenCode):
/// - Strip empty text blocks from blocks content
///   (prevents "all messages must have non-empty content" when only empty text remains)
/// - Strip `cache_control` from any remaining empty text blocks
///   (Anthropic rejects: "cache_control cannot be set for empty text blocks")
fn sanitize_anthropic_message(msg: &mut AnthropicMessage) {
    match &mut msg.content {
        AnthropicMessageContent::Blocks(blocks) => {
            // Remove empty text blocks entirely (OpenCode pattern: filter empty text/reasoning).
            // Keep non-text blocks (tool_result, tool_use, image) and non-empty text.
            blocks.retain(
                |block| !matches!(block, AnthropicContent::Text { text, .. } if text.is_empty()),
            );

            // Safety: strip cache_control from any remaining empty text blocks
            // (e.g., if a block somehow slipped through)
            for block in blocks.iter_mut() {
                if let AnthropicContent::Text {
                    text,
                    cache_control,
                } = block
                    && text.is_empty()
                    && cache_control.is_some()
                {
                    *cache_control = None;
                }
            }
        }
        AnthropicMessageContent::String(_) => {
            // String content has no cache_control field; nothing to sanitize.
        }
    }
}

/// Enforce Anthropic message-sequence-level constraints on the complete array.
///
/// This runs as the final phase after conversion, merging, and caching.
/// It handles structural invariants that span multiple messages.
///
/// Constraints enforced (validated against live Anthropic API 2025-02):
///
/// 1. Every `tool_use` must have exactly one `tool_result` in the immediately
///    following user message (adds placeholders for missing ones)
/// 2. Orphan `tool_result` blocks (not referencing any `tool_use` in the
///    immediately preceding assistant message) are removed
/// 3. No duplicate `tool_result` blocks for the same `tool_use_id`
///    (Anthropic rejects: "each tool_use must have a single result")
/// 4. No empty-content messages — empty string or empty blocks array
///    (Anthropic rejects: "all messages must have non-empty content")
/// 5. Conversation must start with role="user"
/// 6. Conversation must not end with role="assistant" (no prefill — some
///    models reject it; defensive for cross-model compatibility)
/// 7. Re-merges consecutive same-role messages after mutations
fn sanitize_message_sequence(messages: &mut Vec<AnthropicMessage>) {
    if messages.is_empty() {
        return;
    }

    // Step 1: Ensure every tool_use has a matching tool_result.
    patch_tool_result_coverage(messages);

    // Step 2: Remove orphan tool_results that don't match any tool_use
    // in the immediately preceding assistant message.
    remove_orphan_tool_results(messages);

    // Step 3: Deduplicate tool_results — keep only the last result per tool_use_id.
    // Anthropic rejects: "each tool_use must have a single result. Found multiple
    // `tool_result` blocks with id: <id>"
    dedup_tool_results(messages);

    // Step 4: Remove messages with empty content (empty string or empty blocks).
    // Anthropic rejects: "all messages must have non-empty content except for
    // the optional final assistant message"
    remove_empty_content_messages(messages);

    // Step 5: Re-merge consecutive same-role messages that may have been
    // introduced by insertions/removals in steps 1-4.
    let re_merged = merge_consecutive_messages(std::mem::take(messages));
    *messages = re_merged;

    // Step 6: Ensure the first message is role="user".
    if messages.first().is_some_and(|m| m.role != "user") {
        messages.insert(
            0,
            AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicMessageContent::String(".".to_string()),
            },
        );
    }

    // Step 7: Ensure the conversation does not end with an assistant message.
    ensure_not_trailing_assistant(messages);
}

/// Ensure every `tool_use` in assistant messages has a matching `tool_result`
/// in the immediately following user message.
///
/// If the next message is a user message with missing tool_results, placeholder
/// results are injected. If no user message follows, a new one is inserted.
fn patch_tool_result_coverage(messages: &mut Vec<AnthropicMessage>) {
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "assistant" {
            i += 1;
            continue;
        }

        let tool_use_ids = extract_tool_use_ids(&messages[i]);
        if tool_use_ids.is_empty() {
            i += 1;
            continue;
        }

        let next_is_user = messages.get(i + 1).is_some_and(|m| m.role == "user");
        if next_is_user {
            // Check which tool_use IDs are already covered
            let covered_ids = extract_tool_result_ids(&messages[i + 1]);
            let missing: Vec<String> = tool_use_ids
                .into_iter()
                .filter(|id| !covered_ids.contains(id))
                .collect();

            if !missing.is_empty() {
                inject_placeholder_tool_results(&mut messages[i + 1], &missing);
            }
        } else {
            // No user message follows — insert one with all tool_results
            let tool_results: Vec<AnthropicContent> = tool_use_ids
                .into_iter()
                .map(|id| AnthropicContent::ToolResult {
                    tool_use_id: id,
                    content: Some(AnthropicMessageContent::String(
                        "[Tool call not executed]".to_string(),
                    )),
                    is_error: Some(true),
                    cache_control: None,
                })
                .collect();
            messages.insert(
                i + 1,
                AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Blocks(tool_results),
                },
            );
        }

        // Skip over the assistant + user pair
        i += 2;
    }
}

/// Remove `tool_result` blocks from user messages that don't match any
/// `tool_use` in the immediately preceding assistant message.
///
/// Also removes user messages that become empty after orphan removal.
fn remove_orphan_tool_results(messages: &mut Vec<AnthropicMessage>) {
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "user" {
            i += 1;
            continue;
        }

        // Collect valid tool_use IDs from the immediately preceding assistant message
        let valid_ids: HashSet<String> = if i > 0 && messages[i - 1].role == "assistant" {
            extract_tool_use_ids(&messages[i - 1]).into_iter().collect()
        } else {
            HashSet::new()
        };

        if let AnthropicMessageContent::Blocks(blocks) = &mut messages[i].content {
            let had_tool_results = blocks
                .iter()
                .any(|b| matches!(b, AnthropicContent::ToolResult { .. }));

            if had_tool_results {
                blocks.retain(|block| match block {
                    AnthropicContent::ToolResult { tool_use_id, .. } => {
                        valid_ids.contains(tool_use_id)
                    }
                    _ => true,
                });
            }

            // If all blocks were removed, drop the message entirely
            if blocks.is_empty() {
                messages.remove(i);
                continue; // Don't increment — next message shifted into position i
            }
        }

        i += 1;
    }
}

/// Deduplicate `tool_result` blocks within user messages.
///
/// Anthropic rejects: "each tool_use must have a single result. Found multiple
/// `tool_result` blocks with id: <id>". When duplicates exist (e.g., from
/// retry flows or checkpoint corruption), keep only the **last** result per
/// `tool_use_id`.
fn dedup_tool_results(messages: &mut [AnthropicMessage]) {
    for msg in messages.iter_mut() {
        if msg.role != "user" {
            continue;
        }

        if let AnthropicMessageContent::Blocks(blocks) = &mut msg.content {
            let has_tool_results = blocks
                .iter()
                .any(|b| matches!(b, AnthropicContent::ToolResult { .. }));

            if !has_tool_results {
                continue;
            }

            // Find the last occurrence index for each tool_use_id
            let mut last_index: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for (i, block) in blocks.iter().enumerate() {
                if let AnthropicContent::ToolResult { tool_use_id, .. } = block {
                    last_index.insert(tool_use_id.clone(), i);
                }
            }

            // Retain non-tool-result blocks and only the last tool_result per ID
            let mut i = 0;
            blocks.retain(|block| {
                let keep = match block {
                    AnthropicContent::ToolResult { tool_use_id, .. } => {
                        last_index.get(tool_use_id) == Some(&i)
                    }
                    _ => true,
                };
                i += 1;
                keep
            });
        }
    }
}

/// Remove messages with empty content.
///
/// Anthropic rejects: "all messages must have non-empty content except for
/// the optional final assistant message". This covers:
/// - Empty string content (`""`)
/// - Empty blocks array (`[]`)
fn remove_empty_content_messages(messages: &mut Vec<AnthropicMessage>) {
    messages.retain(|msg| match &msg.content {
        AnthropicMessageContent::String(s) => !s.is_empty(),
        AnthropicMessageContent::Blocks(blocks) => !blocks.is_empty(),
    });
}

/// Ensure the conversation does not end with an assistant message that would
/// cause API errors.
///
/// Handling by case:
/// - **tool_use blocks present**: append a user message with placeholder
///   `tool_result` blocks (API requires every tool_use to have a result).
/// - **Empty or whitespace-only text**: remove the trailing assistant
///   (Anthropic rejects trailing whitespace-only assistant content, and
///   empty responses indicate incomplete/dangling state).
/// - **Substantive text content**: preserve it as-is. The Anthropic API
///   accepts trailing assistant messages as "prefill" for continuation on
///   models that support it (Claude Sonnet 4, Opus 4, etc.). Removing
///   valid context would lose information from checkpoints and context
///   managers that legitimately produce this state.
fn ensure_not_trailing_assistant(messages: &mut Vec<AnthropicMessage>) {
    // Loop in case removing an assistant reveals another trailing assistant.
    while messages.last().is_some_and(|m| m.role == "assistant") {
        let last = messages.last().expect("checked above");
        let tool_use_ids = extract_tool_use_ids(last);

        if !tool_use_ids.is_empty() {
            // Has tool_use — add user message with placeholder tool_results
            let tool_results: Vec<AnthropicContent> = tool_use_ids
                .into_iter()
                .map(|id| AnthropicContent::ToolResult {
                    tool_use_id: id,
                    content: Some(AnthropicMessageContent::String(
                        "[Tool call interrupted]".to_string(),
                    )),
                    is_error: Some(true),
                    cache_control: None,
                })
                .collect();
            messages.push(AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicMessageContent::Blocks(tool_results),
            });
            break; // We added a user message, so we're done
        }

        // No tool_use — check if content is substantive (worth keeping as prefill)
        let is_substantive = match &last.content {
            AnthropicMessageContent::String(s) => !s.trim().is_empty(),
            AnthropicMessageContent::Blocks(blocks) => blocks.iter().any(|b| match b {
                AnthropicContent::Text { text, .. } => !text.trim().is_empty(),
                // Non-text blocks (images, thinking) count as substantive
                AnthropicContent::Image { .. }
                | AnthropicContent::Thinking { .. }
                | AnthropicContent::RedactedThinking { .. } => true,
                // tool_use handled above; tool_result in assistant is unusual
                _ => false,
            }),
        };

        if is_substantive {
            // Preserve trailing assistant with real content (API accepts prefill)
            break;
        }

        // Empty/whitespace-only — discard (dangling/incomplete response)
        messages.pop();
    }
}

/// Extract all `tool_use` IDs from an Anthropic message's content blocks.
fn extract_tool_use_ids(msg: &AnthropicMessage) -> Vec<String> {
    match &msg.content {
        AnthropicMessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if let AnthropicContent::ToolUse { id, .. } = b {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

/// Extract all `tool_result` tool_use_ids from an Anthropic message's content blocks.
fn extract_tool_result_ids(msg: &AnthropicMessage) -> HashSet<String> {
    match &msg.content {
        AnthropicMessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if let AnthropicContent::ToolResult { tool_use_id, .. } = b {
                    Some(tool_use_id.clone())
                } else {
                    None
                }
            })
            .collect(),
        _ => HashSet::new(),
    }
}

/// Inject placeholder `tool_result` blocks for missing tool_use IDs into a user message.
fn inject_placeholder_tool_results(msg: &mut AnthropicMessage, missing_ids: &[String]) {
    let new_blocks: Vec<AnthropicContent> = missing_ids
        .iter()
        .map(|id| AnthropicContent::ToolResult {
            tool_use_id: id.clone(),
            content: Some(AnthropicMessageContent::String(
                "[Tool call not executed]".to_string(),
            )),
            is_error: Some(true),
            cache_control: None,
        })
        .collect();

    match &mut msg.content {
        AnthropicMessageContent::Blocks(blocks) => {
            blocks.extend(new_blocks);
        }
        AnthropicMessageContent::String(s) => {
            // Convert String content to Blocks. Skip creating an empty text block
            // from String("") — this avoids reintroducing empty text blocks that
            // per-message sanitization already stripped.
            let mut blocks = Vec::new();
            if !s.is_empty() {
                blocks.push(AnthropicContent::Text {
                    text: std::mem::take(s),
                    cache_control: None,
                });
            }
            blocks.extend(new_blocks);
            msg.content = AnthropicMessageContent::Blocks(blocks);
        }
    }
}

/// Set cache_control on an AnthropicContent block.
fn set_block_cache_control(block: &mut AnthropicContent, cc: Option<AnthropicCacheControl>) {
    match block {
        AnthropicContent::Text { cache_control, .. }
        | AnthropicContent::ToolUse { cache_control, .. }
        | AnthropicContent::ToolResult { cache_control, .. }
        | AnthropicContent::Image { cache_control, .. } => *cache_control = cc,
        AnthropicContent::Thinking { .. } | AnthropicContent::RedactedThinking { .. } => {
            // Thinking blocks don't support cache_control
        }
    }
}

/// Merge consecutive messages with the same role into single messages.
///
/// Anthropic requires that tool_result blocks appear in a single user message
/// immediately after the assistant message containing the matching tool_use blocks.
/// When multiple tool results are converted individually, each becomes a separate
/// "user" message. This function combines them (and any other consecutive same-role
/// messages) into one.
fn merge_consecutive_messages(messages: Vec<AnthropicMessage>) -> Vec<AnthropicMessage> {
    if messages.is_empty() {
        return messages;
    }

    let mut result: Vec<AnthropicMessage> = Vec::with_capacity(messages.len());

    for msg in messages {
        let should_merge = result.last().is_some_and(|last| last.role == msg.role);

        if should_merge {
            let Some(last) = result.last_mut() else {
                // unreachable: guarded by is_some_and check above
                result.push(msg);
                continue;
            };
            let prev = std::mem::take(&mut last.content);
            last.content = merge_content(prev, msg.content);
        } else {
            result.push(msg);
        }
    }

    result
}

/// Convert AnthropicMessageContent to a Vec<AnthropicContent> blocks.
fn content_to_blocks(content: AnthropicMessageContent) -> Vec<AnthropicContent> {
    match content {
        AnthropicMessageContent::Blocks(blocks) => blocks,
        AnthropicMessageContent::String(s) => {
            vec![AnthropicContent::Text {
                text: s,
                cache_control: None,
            }]
        }
    }
}

/// Merge two AnthropicMessageContent values into one Blocks variant.
fn merge_content(
    a: AnthropicMessageContent,
    b: AnthropicMessageContent,
) -> AnthropicMessageContent {
    let mut blocks = content_to_blocks(a);
    blocks.extend(content_to_blocks(b));
    AnthropicMessageContent::Blocks(blocks)
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

    // --- merge_consecutive_messages tests ---

    fn user_msg(text: &str) -> AnthropicMessage {
        AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicMessageContent::String(text.to_string()),
        }
    }

    fn assistant_msg(text: &str) -> AnthropicMessage {
        AnthropicMessage {
            role: "assistant".to_string(),
            content: AnthropicMessageContent::String(text.to_string()),
        }
    }

    fn user_blocks_msg(blocks: Vec<AnthropicContent>) -> AnthropicMessage {
        AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicMessageContent::Blocks(blocks),
        }
    }

    fn assistant_blocks_msg(blocks: Vec<AnthropicContent>) -> AnthropicMessage {
        AnthropicMessage {
            role: "assistant".to_string(),
            content: AnthropicMessageContent::Blocks(blocks),
        }
    }

    fn text_block(text: &str) -> AnthropicContent {
        AnthropicContent::Text {
            text: text.to_string(),
            cache_control: None,
        }
    }

    fn tool_result_block(tool_use_id: &str, content: &str) -> AnthropicContent {
        AnthropicContent::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: Some(AnthropicMessageContent::String(content.to_string())),
            is_error: None,
            cache_control: None,
        }
    }

    fn tool_use_block(id: &str, name: &str) -> AnthropicContent {
        AnthropicContent::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input: serde_json::json!({}),
            cache_control: None,
        }
    }

    fn count_blocks(content: &AnthropicMessageContent) -> usize {
        match content {
            AnthropicMessageContent::String(_) => 1,
            AnthropicMessageContent::Blocks(b) => b.len(),
        }
    }

    #[test]
    fn test_merge_consecutive_user_messages() {
        let messages = vec![user_msg("Hello"), user_msg("World")];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].role, "user");
        assert_eq!(count_blocks(&merged[0].content), 2);
    }

    #[test]
    fn test_merge_consecutive_tool_result_messages() {
        // Three consecutive user messages (each with a tool_result) should merge into one
        let messages = vec![
            user_blocks_msg(vec![tool_result_block("t1", "result1")]),
            user_blocks_msg(vec![tool_result_block("t2", "result2")]),
            user_blocks_msg(vec![tool_result_block("t3", "result3")]),
        ];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].role, "user");
        assert_eq!(count_blocks(&merged[0].content), 3);

        // Verify all tool_result blocks are present
        if let AnthropicMessageContent::Blocks(blocks) = &merged[0].content {
            for (i, block) in blocks.iter().enumerate() {
                match block {
                    AnthropicContent::ToolResult { tool_use_id, .. } => {
                        assert_eq!(tool_use_id, &format!("t{}", i + 1));
                    }
                    _ => panic!("Expected ToolResult block at index {}", i),
                }
            }
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn test_merge_consecutive_assistant_messages() {
        let messages = vec![assistant_msg("Part 1"), assistant_msg("Part 2")];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].role, "assistant");
        assert_eq!(count_blocks(&merged[0].content), 2);
    }

    #[test]
    fn test_no_merge_alternating_roles() {
        let messages = vec![user_msg("Hi"), assistant_msg("Hello"), user_msg("Bye")];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].role, "user");
        assert_eq!(merged[1].role, "assistant");
        assert_eq!(merged[2].role, "user");
    }

    #[test]
    fn test_merge_mixed_string_and_blocks() {
        let messages = vec![
            user_msg("Hello"),
            user_blocks_msg(vec![text_block("World")]),
        ];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 1);
        assert_eq!(count_blocks(&merged[0].content), 2);

        if let AnthropicMessageContent::Blocks(blocks) = &merged[0].content {
            match &blocks[0] {
                AnthropicContent::Text { text, .. } => assert_eq!(text, "Hello"),
                _ => panic!("Expected Text block"),
            }
            match &blocks[1] {
                AnthropicContent::Text { text, .. } => assert_eq!(text, "World"),
                _ => panic!("Expected Text block"),
            }
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn test_merge_preserves_cache_control_on_last() {
        let cached_block = AnthropicContent::Text {
            text: "cached".to_string(),
            cache_control: Some(AnthropicCacheControl::ephemeral()),
        };
        let messages = vec![user_msg("first"), user_blocks_msg(vec![cached_block])];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 1);
        if let AnthropicMessageContent::Blocks(blocks) = &merged[0].content {
            assert_eq!(blocks.len(), 2);
            // First block should have no cache control
            match &blocks[0] {
                AnthropicContent::Text { cache_control, .. } => {
                    assert!(cache_control.is_none());
                }
                _ => panic!("Expected Text block"),
            }
            // Last block should preserve cache control
            match &blocks[1] {
                AnthropicContent::Text { cache_control, .. } => {
                    assert!(cache_control.is_some());
                }
                _ => panic!("Expected Text block"),
            }
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn test_single_message_no_merge() {
        let messages = vec![user_msg("solo")];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].role, "user");
        match &merged[0].content {
            AnthropicMessageContent::String(s) => assert_eq!(s, "solo"),
            _ => panic!("Expected String content for single message"),
        }
    }

    #[test]
    fn test_empty_messages() {
        let messages: Vec<AnthropicMessage> = vec![];
        let merged = merge_consecutive_messages(messages);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_full_conversation_with_multiple_tool_results() {
        // Simulate: assistant with 3 tool_use, followed by 3 tool result messages
        let messages = vec![
            assistant_blocks_msg(vec![
                tool_use_block("t1", "tool_a"),
                tool_use_block("t2", "tool_b"),
                tool_use_block("t3", "tool_c"),
            ]),
            user_blocks_msg(vec![tool_result_block("t1", "result_a")]),
            user_blocks_msg(vec![tool_result_block("t2", "result_b")]),
            user_blocks_msg(vec![tool_result_block("t3", "result_c")]),
        ];
        let merged = merge_consecutive_messages(messages);

        // Should produce [assistant, user(3 tool_results)]
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].role, "assistant");
        assert_eq!(merged[1].role, "user");

        // Assistant should have 3 tool_use blocks
        assert_eq!(count_blocks(&merged[0].content), 3);

        // User should have 3 tool_result blocks
        assert_eq!(count_blocks(&merged[1].content), 3);
        if let AnthropicMessageContent::Blocks(blocks) = &merged[1].content {
            for block in blocks {
                assert!(
                    matches!(block, AnthropicContent::ToolResult { .. }),
                    "Expected ToolResult block"
                );
            }
        }
    }

    #[test]
    fn test_user_message_followed_by_tool_results_merges() {
        // A user text message followed by tool result messages should merge
        let messages = vec![
            user_msg("Here are the results:"),
            user_blocks_msg(vec![tool_result_block("t1", "result1")]),
            user_blocks_msg(vec![tool_result_block("t2", "result2")]),
        ];
        let merged = merge_consecutive_messages(messages);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].role, "user");
        assert_eq!(count_blocks(&merged[0].content), 3);

        if let AnthropicMessageContent::Blocks(blocks) = &merged[0].content {
            match &blocks[0] {
                AnthropicContent::Text { text, .. } => {
                    assert_eq!(text, "Here are the results:");
                }
                _ => panic!("Expected Text block first"),
            }
            assert!(matches!(&blocks[1], AnthropicContent::ToolResult { .. }));
            assert!(matches!(&blocks[2], AnthropicContent::ToolResult { .. }));
        }
    }

    // --- apply_tail_cache_to_message tests ---

    #[test]
    fn test_apply_tail_cache_to_string_message() {
        let mut validator = CacheControlValidator::new();
        let mut msg = user_msg("hello");

        apply_tail_cache_to_message(&mut msg, &mut validator);

        // Should convert to blocks and add cache_control
        match &msg.content {
            AnthropicMessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AnthropicContent::Text {
                        text,
                        cache_control,
                    } => {
                        assert_eq!(text, "hello");
                        assert!(cache_control.is_some());
                    }
                    _ => panic!("Expected Text block"),
                }
            }
            _ => panic!("Expected Blocks content after tail cache"),
        }
        assert_eq!(validator.breakpoint_count(), 1);
    }

    #[test]
    fn test_apply_tail_cache_to_blocks_message() {
        let mut validator = CacheControlValidator::new();
        let mut msg = user_blocks_msg(vec![
            tool_result_block("t1", "result1"),
            tool_result_block("t2", "result2"),
        ]);

        apply_tail_cache_to_message(&mut msg, &mut validator);

        // Only the LAST block should get cache_control
        if let AnthropicMessageContent::Blocks(blocks) = &msg.content {
            assert_eq!(blocks.len(), 2);
            match &blocks[0] {
                AnthropicContent::ToolResult { cache_control, .. } => {
                    assert!(cache_control.is_none(), "First block should NOT be cached");
                }
                _ => panic!("Expected ToolResult"),
            }
            match &blocks[1] {
                AnthropicContent::ToolResult { cache_control, .. } => {
                    assert!(cache_control.is_some(), "Last block SHOULD be cached");
                }
                _ => panic!("Expected ToolResult"),
            }
        } else {
            panic!("Expected Blocks content");
        }
        assert_eq!(validator.breakpoint_count(), 1);
    }

    #[test]
    fn test_apply_tail_cache_respects_breakpoint_limit() {
        let mut validator = CacheControlValidator::new();
        let cache = crate::types::CacheControl::ephemeral();

        // Exhaust all 4 breakpoints
        for _ in 0..4 {
            validator.validate(Some(&cache), CacheContext::user_message_part());
        }
        assert!(validator.is_at_limit());

        let mut msg = user_msg("no room");
        apply_tail_cache_to_message(&mut msg, &mut validator);

        // Should remain a String (no conversion) since breakpoint was rejected
        match &msg.content {
            AnthropicMessageContent::String(s) => assert_eq!(s, "no room"),
            _ => panic!("Should not convert to blocks when breakpoint limit exceeded"),
        }
    }

    #[test]
    fn test_tail_cache_after_merge_uses_one_breakpoint_for_merged_tool_results() {
        // Scenario: 3 consecutive tool_result user messages merge into 1.
        // Tail caching should use only 1 breakpoint (on the merged message),
        // NOT 3 breakpoints (one per pre-merge message).
        let mut validator = CacheControlValidator::new();

        let mut merged = [
            assistant_blocks_msg(vec![
                tool_use_block("t1", "tool_a"),
                tool_use_block("t2", "tool_b"),
                tool_use_block("t3", "tool_c"),
            ]),
            // Simulate 3 tool_result messages already merged into 1
            user_blocks_msg(vec![
                tool_result_block("t1", "result_a"),
                tool_result_block("t2", "result_b"),
                tool_result_block("t3", "result_c"),
            ]),
        ];

        // Apply tail caching with tail_count=2 (both messages)
        let len = merged.len();
        let cache_start = len.saturating_sub(2);
        for msg in &mut merged[cache_start..] {
            apply_tail_cache_to_message(msg, &mut validator);
        }

        // Should use exactly 2 breakpoints (one per merged message)
        assert_eq!(
            validator.breakpoint_count(),
            2,
            "Should use 2 breakpoints, not more"
        );

        // Assistant message: last block (tool_use t3) should be cached
        if let AnthropicMessageContent::Blocks(blocks) = &merged[0].content {
            match &blocks[2] {
                AnthropicContent::ToolUse { cache_control, .. } => {
                    assert!(cache_control.is_some(), "Last tool_use should be cached");
                }
                _ => panic!("Expected ToolUse"),
            }
            // First two should NOT be cached
            for block in &blocks[..2] {
                match block {
                    AnthropicContent::ToolUse { cache_control, .. } => {
                        assert!(cache_control.is_none());
                    }
                    _ => panic!("Expected ToolUse"),
                }
            }
        }

        // User message: last block (tool_result t3) should be cached
        if let AnthropicMessageContent::Blocks(blocks) = &merged[1].content {
            match &blocks[2] {
                AnthropicContent::ToolResult { cache_control, .. } => {
                    assert!(cache_control.is_some(), "Last tool_result should be cached");
                }
                _ => panic!("Expected ToolResult"),
            }
            // First two should NOT be cached
            for block in &blocks[..2] {
                match block {
                    AnthropicContent::ToolResult { cache_control, .. } => {
                        assert!(cache_control.is_none());
                    }
                    _ => panic!("Expected ToolResult"),
                }
            }
        }
    }

    #[test]
    fn test_set_block_cache_control() {
        let cc = AnthropicCacheControl::ephemeral();

        // Text block
        let mut block = text_block("hello");
        set_block_cache_control(&mut block, Some(cc.clone()));
        match &block {
            AnthropicContent::Text { cache_control, .. } => assert!(cache_control.is_some()),
            _ => panic!("Expected Text"),
        }

        // ToolResult block
        let mut block = tool_result_block("t1", "result");
        set_block_cache_control(&mut block, Some(cc.clone()));
        match &block {
            AnthropicContent::ToolResult { cache_control, .. } => {
                assert!(cache_control.is_some())
            }
            _ => panic!("Expected ToolResult"),
        }

        // ToolUse block
        let mut block = tool_use_block("t1", "tool_a");
        set_block_cache_control(&mut block, Some(cc.clone()));
        match &block {
            AnthropicContent::ToolUse { cache_control, .. } => assert!(cache_control.is_some()),
            _ => panic!("Expected ToolUse"),
        }
    }

    // --- sanitize_anthropic_message tests ---

    #[test]
    fn test_sanitize_removes_empty_text_blocks() {
        let cc = AnthropicCacheControl::ephemeral();

        // Empty text block (with or without cache_control) should be removed entirely
        let mut msg = user_blocks_msg(vec![AnthropicContent::Text {
            text: String::new(),
            cache_control: Some(cc.clone()),
        }]);
        sanitize_anthropic_message(&mut msg);
        match &msg.content {
            AnthropicMessageContent::Blocks(blocks) => {
                assert!(
                    blocks.is_empty(),
                    "Empty text blocks must be removed entirely"
                );
            }
            _ => panic!("Expected Blocks"),
        }
    }

    #[test]
    fn test_sanitize_preserves_cache_control_on_non_empty_text() {
        let cc = AnthropicCacheControl::ephemeral();

        let mut msg = user_blocks_msg(vec![AnthropicContent::Text {
            text: "hello".to_string(),
            cache_control: Some(cc.clone()),
        }]);
        sanitize_anthropic_message(&mut msg);
        match &msg.content {
            AnthropicMessageContent::Blocks(blocks) => match &blocks[0] {
                AnthropicContent::Text { cache_control, .. } => {
                    assert!(
                        cache_control.is_some(),
                        "cache_control must be preserved on non-empty text"
                    );
                }
                _ => panic!("Expected Text block"),
            },
            _ => panic!("Expected Blocks"),
        }
    }

    #[test]
    fn test_sanitize_handles_mixed_blocks() {
        let cc = AnthropicCacheControl::ephemeral();

        // Mix of empty text (with cache), non-empty text (with cache), and tool_result
        let mut msg = user_blocks_msg(vec![
            AnthropicContent::Text {
                text: String::new(),
                cache_control: Some(cc.clone()),
            },
            AnthropicContent::Text {
                text: "real content".to_string(),
                cache_control: Some(cc.clone()),
            },
            AnthropicContent::ToolResult {
                tool_use_id: "t1".to_string(),
                content: Some(AnthropicMessageContent::String("ok".to_string())),
                is_error: None,
                cache_control: Some(cc.clone()),
            },
        ]);
        sanitize_anthropic_message(&mut msg);
        match &msg.content {
            AnthropicMessageContent::Blocks(blocks) => {
                // Empty text block should be removed entirely
                assert_eq!(blocks.len(), 2);
                // Non-empty text: cache_control preserved
                match &blocks[0] {
                    AnthropicContent::Text {
                        text,
                        cache_control,
                    } => {
                        assert_eq!(text, "real content");
                        assert!(cache_control.is_some());
                    }
                    _ => panic!("Expected Text"),
                }
                // ToolResult: cache_control preserved
                match &blocks[1] {
                    AnthropicContent::ToolResult { cache_control, .. } => {
                        assert!(cache_control.is_some());
                    }
                    _ => panic!("Expected ToolResult"),
                }
            }
            _ => panic!("Expected Blocks"),
        }
    }

    #[test]
    fn test_sanitize_noop_on_string_content() {
        // String content has no cache_control field — sanitize should be a no-op
        let mut msg = user_msg("hello");
        sanitize_anthropic_message(&mut msg);
        match &msg.content {
            AnthropicMessageContent::String(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected String content"),
        }
    }

    // --- is_empty_content_message tests ---

    #[test]
    fn test_is_empty_content_message() {
        // Empty string content
        assert!(is_empty_content_message(&user_msg("")));

        // Non-empty string content
        assert!(!is_empty_content_message(&user_msg("hello")));

        // Blocks with only empty text
        assert!(is_empty_content_message(&user_blocks_msg(vec![
            text_block(""),
        ])));

        // Blocks with non-empty text
        assert!(!is_empty_content_message(&user_blocks_msg(vec![
            text_block("hello"),
        ])));

        // Blocks with tool_result (not empty even if no text)
        assert!(!is_empty_content_message(&user_blocks_msg(vec![
            tool_result_block("t1", "result"),
        ])));

        // Mixed: empty text + tool_result → not empty
        assert!(!is_empty_content_message(&user_blocks_msg(vec![
            text_block(""),
            tool_result_block("t1", "result"),
        ])));
    }

    #[test]
    fn test_empty_message_does_not_waste_cache_breakpoint() {
        // Phase 5 (tail cache) should skip empty messages, preserving breakpoints for real content.
        let mut validator = CacheControlValidator::new();

        // Simulate tail caching over [non-empty, empty, non-empty]
        let mut messages = vec![
            user_msg("real content"),
            assistant_msg(""), // empty — should be skipped
            user_msg("more content"),
        ];

        for msg in &mut messages {
            if !is_empty_content_message(msg) {
                apply_tail_cache_to_message(msg, &mut validator);
            }
        }

        // Only 2 breakpoints consumed, not 3
        assert_eq!(
            validator.breakpoint_count(),
            2,
            "Empty message must not consume a cache breakpoint"
        );
    }

    // --- sanitize_message_sequence tests ---

    #[test]
    fn test_sanitize_sequence_adds_missing_tool_results() {
        // Assistant with 3 tool_use, but user only has 1 tool_result
        let mut messages = vec![
            user_msg("Hello"),
            assistant_blocks_msg(vec![
                tool_use_block("t1", "tool_a"),
                tool_use_block("t2", "tool_b"),
                tool_use_block("t3", "tool_c"),
            ]),
            user_blocks_msg(vec![tool_result_block("t1", "result_a")]),
        ];
        sanitize_message_sequence(&mut messages);

        // Should still have 3 messages (user, assistant, user)
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[2].role, "user");

        // The user message should now have 3 tool_result blocks
        let result_ids = extract_tool_result_ids(&messages[2]);
        assert!(result_ids.contains("t1"), "Original tool_result preserved");
        assert!(result_ids.contains("t2"), "Placeholder added for t2");
        assert!(result_ids.contains("t3"), "Placeholder added for t3");
    }

    #[test]
    fn test_sanitize_sequence_inserts_user_for_dangling_tool_use() {
        // Assistant with tool_use but NO following user message
        let mut messages = vec![
            user_msg("Hello"),
            assistant_blocks_msg(vec![
                tool_use_block("t1", "tool_a"),
                tool_use_block("t2", "tool_b"),
            ]),
        ];
        sanitize_message_sequence(&mut messages);

        // Should insert a user message with placeholder tool_results
        // and the conversation should end with user
        assert!(messages.last().is_some_and(|m| m.role == "user"));

        // Check the last user message has both tool_results
        let last = messages.last().expect("non-empty");
        let result_ids = extract_tool_result_ids(last);
        assert!(result_ids.contains("t1"));
        assert!(result_ids.contains("t2"));
    }

    #[test]
    fn test_sanitize_sequence_preserves_trailing_assistant_with_substantive_text() {
        // Conversation ending with substantive assistant text (no tool_use)
        let mut messages = vec![user_msg("Hello"), assistant_msg("I'll help you with that.")];
        sanitize_message_sequence(&mut messages);

        // Substantive trailing assistant should be preserved (API accepts prefill)
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_sanitize_sequence_removes_trailing_assistant_empty_text() {
        // Conversation ending with empty/whitespace assistant text
        let mut messages = vec![user_msg("Hello"), assistant_msg("   ")];
        sanitize_message_sequence(&mut messages);

        // Whitespace-only trailing assistant should be removed
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
    }

    #[test]
    fn test_sanitize_sequence_trailing_assistant_with_tool_use() {
        // Conversation ending with assistant that has tool_use
        let mut messages = vec![
            user_msg("Hello"),
            assistant_blocks_msg(vec![
                text_block("Let me check..."),
                tool_use_block("t1", "search"),
            ]),
        ];
        sanitize_message_sequence(&mut messages);

        // Should add user message with tool_result placeholder
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[2].role, "user");

        let result_ids = extract_tool_result_ids(&messages[2]);
        assert!(result_ids.contains("t1"));
    }

    #[test]
    fn test_sanitize_sequence_removes_orphan_tool_results() {
        // User message has tool_results that don't match any tool_use in preceding assistant
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("Sure!"), // No tool_use blocks
            user_blocks_msg(vec![
                tool_result_block("orphan_id", "stale result"),
                text_block("Follow-up text"),
            ]),
        ];
        sanitize_message_sequence(&mut messages);

        // The orphan tool_result should be removed, but text should remain
        let last = messages.last().expect("non-empty");
        assert_eq!(last.role, "user");
        let result_ids = extract_tool_result_ids(last);
        assert!(
            result_ids.is_empty(),
            "Orphan tool_result should be removed"
        );

        // Text block should be preserved
        if let AnthropicMessageContent::Blocks(blocks) = &last.content {
            assert!(blocks.iter().any(
                |b| matches!(b, AnthropicContent::Text { text, .. } if text == "Follow-up text")
            ));
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn test_sanitize_sequence_removes_user_with_only_orphan_results() {
        // User message that becomes empty after orphan removal
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("Sure!"), // No tool_use
            user_blocks_msg(vec![tool_result_block("orphan_id", "stale result")]),
        ];
        sanitize_message_sequence(&mut messages);

        // After removing orphan, user message becomes empty and should be removed.
        // The assistant "Sure!" is substantive, so it's preserved as prefill.
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_sanitize_sequence_ensures_starts_with_user() {
        // Conversation that starts with assistant
        let mut messages = vec![assistant_msg("Hello"), user_msg("Hi")];
        sanitize_message_sequence(&mut messages);

        assert_eq!(messages[0].role, "user");
    }

    #[test]
    fn test_sanitize_sequence_noop_for_valid_conversation() {
        // Already valid alternating conversation
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("Hi there!"),
            user_msg("How are you?"),
        ];
        let original_len = messages.len();
        sanitize_message_sequence(&mut messages);

        assert_eq!(messages.len(), original_len);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[2].role, "user");
    }

    #[test]
    fn test_sanitize_sequence_full_tool_call_flow() {
        // Realistic scenario: assistant with 6 tool calls, all results present
        let mut messages = vec![
            user_msg("Run all checks"),
            assistant_blocks_msg(vec![
                tool_use_block("t1", "check_a"),
                tool_use_block("t2", "check_b"),
                tool_use_block("t3", "check_c"),
                tool_use_block("t4", "check_d"),
                tool_use_block("t5", "check_e"),
                tool_use_block("t6", "check_f"),
            ]),
            user_blocks_msg(vec![
                tool_result_block("t1", "ok"),
                tool_result_block("t2", "ok"),
                tool_result_block("t3", "ok"),
                tool_result_block("t4", "ok"),
                tool_result_block("t5", "ok"),
                tool_result_block("t6", "ok"),
            ]),
        ];
        sanitize_message_sequence(&mut messages);

        // Should remain unchanged
        assert_eq!(messages.len(), 3);
        let result_ids = extract_tool_result_ids(&messages[2]);
        assert_eq!(result_ids.len(), 6);
    }

    #[test]
    fn test_sanitize_sequence_partial_tool_results_missing() {
        // Error scenario from the bug report: 6 tool_use but 0 tool_results
        let mut messages = vec![
            user_msg("Run all checks"),
            assistant_blocks_msg(vec![
                tool_use_block("t1", "check_a"),
                tool_use_block("t2", "check_b"),
                tool_use_block("t3", "check_c"),
                tool_use_block("t4", "check_d"),
                tool_use_block("t5", "check_e"),
                tool_use_block("t6", "check_f"),
            ]),
            user_msg("Continue"), // No tool_results at all
        ];
        sanitize_message_sequence(&mut messages);

        // Should have placeholders for all 6 tool_use IDs
        assert_eq!(messages[2].role, "user");
        let result_ids = extract_tool_result_ids(&messages[2]);
        assert_eq!(
            result_ids.len(),
            6,
            "All 6 missing tool_results should have placeholders"
        );
    }

    #[test]
    fn test_sanitize_sequence_empty_messages() {
        let mut messages: Vec<AnthropicMessage> = vec![];
        sanitize_message_sequence(&mut messages);
        assert!(messages.is_empty());
    }

    #[test]
    fn test_sanitize_sequence_multiple_consecutive_trailing_assistants_substantive() {
        // Edge case: multiple trailing assistants merge into one with content
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("Part 1"),
            assistant_msg("Part 2"),
        ];
        sanitize_message_sequence(&mut messages);

        // After merge, becomes [user, assistant("Part 1\nPart 2")].
        // Substantive content is preserved as prefill.
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_sanitize_sequence_multiple_consecutive_trailing_assistants_empty() {
        // Edge case: multiple trailing assistants that are all empty/whitespace
        let mut messages = vec![user_msg("Hello"), assistant_msg(""), assistant_msg("   ")];
        sanitize_message_sequence(&mut messages);

        // Empty messages removed by step 4, whitespace-only removed as non-substantive
        assert!(
            messages.last().is_some_and(|m| m.role == "user"),
            "Must end with user after empty trailing assistants removed"
        );
    }

    #[test]
    fn test_extract_tool_use_ids() {
        let msg = assistant_blocks_msg(vec![
            text_block("I'll run some tools"),
            tool_use_block("t1", "search"),
            tool_use_block("t2", "fetch"),
        ]);
        let ids = extract_tool_use_ids(&msg);
        assert_eq!(ids, vec!["t1", "t2"]);
    }

    #[test]
    fn test_extract_tool_use_ids_from_string_content() {
        let msg = assistant_msg("No tools here");
        let ids = extract_tool_use_ids(&msg);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_extract_tool_result_ids() {
        let msg = user_blocks_msg(vec![
            tool_result_block("t1", "result1"),
            tool_result_block("t2", "result2"),
            text_block("Some text"),
        ]);
        let ids = extract_tool_result_ids(&msg);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains("t1"));
        assert!(ids.contains("t2"));
    }

    #[test]
    fn test_inject_placeholder_tool_results_into_blocks() {
        let mut msg = user_blocks_msg(vec![tool_result_block("t1", "result1")]);
        inject_placeholder_tool_results(&mut msg, &["t2".to_string(), "t3".to_string()]);

        if let AnthropicMessageContent::Blocks(blocks) = &msg.content {
            assert_eq!(blocks.len(), 3);
            // Original
            assert!(
                matches!(&blocks[0], AnthropicContent::ToolResult { tool_use_id, .. } if tool_use_id == "t1")
            );
            // Injected placeholders
            assert!(
                matches!(&blocks[1], AnthropicContent::ToolResult { tool_use_id, is_error, .. } if tool_use_id == "t2" && *is_error == Some(true))
            );
            assert!(
                matches!(&blocks[2], AnthropicContent::ToolResult { tool_use_id, .. } if tool_use_id == "t3")
            );
        } else {
            panic!("Expected Blocks");
        }
    }

    #[test]
    fn test_inject_placeholder_tool_results_into_string() {
        let mut msg = user_msg("Continue");
        inject_placeholder_tool_results(&mut msg, &["t1".to_string()]);

        if let AnthropicMessageContent::Blocks(blocks) = &msg.content {
            assert_eq!(blocks.len(), 2);
            // Original text preserved as Text block
            assert!(
                matches!(&blocks[0], AnthropicContent::Text { text, .. } if text == "Continue")
            );
            // Injected placeholder
            assert!(
                matches!(&blocks[1], AnthropicContent::ToolResult { tool_use_id, .. } if tool_use_id == "t1")
            );
        } else {
            panic!("Expected Blocks");
        }
    }

    #[test]
    fn test_inject_placeholder_into_empty_string_skips_empty_text_block() {
        // Issue 2: inject_placeholder_tool_results on String("") should NOT create
        // an empty text block that would reintroduce the problem sanitize_anthropic_message fixed.
        let mut msg = AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicMessageContent::String(String::new()),
        };
        inject_placeholder_tool_results(&mut msg, &["t1".to_string()]);

        if let AnthropicMessageContent::Blocks(blocks) = &msg.content {
            // Should contain ONLY the tool_result — no empty text block
            assert_eq!(
                blocks.len(),
                1,
                "Empty string should not produce a text block"
            );
            assert!(
                matches!(&blocks[0], AnthropicContent::ToolResult { tool_use_id, .. } if tool_use_id == "t1")
            );
        } else {
            panic!("Expected Blocks");
        }
    }

    #[test]
    fn test_sanitize_sequence_distant_orphan_tool_result() {
        // Vercel-inspired: tool_result appears in a distant user message,
        // not immediately after the assistant with the matching tool_use.
        // This simulates context manager truncation or checkpoint corruption.
        let mut messages = vec![
            assistant_blocks_msg(vec![tool_use_block("t1", "search")]),
            user_msg("Intermediate text"), // No tool_result for t1
            assistant_msg("I found something"),
            user_blocks_msg(vec![tool_result_block("t1", "late result")]), // Orphan
        ];
        sanitize_message_sequence(&mut messages);

        // Step 1 should inject placeholder for t1 after first assistant.
        // Step 2 should remove orphan t1 from the last user message.
        // Step 4 should prepend a user message since first msg is assistant.
        // Step 5 should handle trailing state.

        // Verify: first message is user
        assert_eq!(messages[0].role, "user");

        // Verify: the assistant with tool_use(t1) is followed by a user with tool_result(t1)
        let assistant_idx = messages
            .iter()
            .position(|m| m.role == "assistant" && !extract_tool_use_ids(m).is_empty())
            .expect("Should have assistant with tool_use");
        let next = &messages[assistant_idx + 1];
        assert_eq!(next.role, "user");
        let result_ids = extract_tool_result_ids(next);
        assert!(
            result_ids.contains("t1"),
            "tool_result for t1 must follow its tool_use"
        );

        // The trailing assistant "I found something" is substantive → preserved
        let last = messages.last().expect("non-empty");
        assert!(
            last.role == "user" || last.role == "assistant",
            "Must end with user or substantive assistant"
        );

        // Verify: no orphan tool_result(t1) in later messages
        for msg in &messages[(assistant_idx + 2)..] {
            let orphans = extract_tool_result_ids(msg);
            assert!(
                !orphans.contains("t1"),
                "Orphan tool_result(t1) should be removed from later messages"
            );
        }
    }

    #[test]
    fn test_sanitize_sequence_context_manager_truncated_results() {
        // Simulates context manager dropping tool_result messages to save tokens.
        // Multi-turn conversation where middle tool_results were removed.
        let mut messages = vec![
            user_msg("Start"),
            // Turn 1: assistant calls tools, results present
            assistant_blocks_msg(vec![
                tool_use_block("t1", "search"),
                tool_use_block("t2", "fetch"),
            ]),
            user_blocks_msg(vec![
                tool_result_block("t1", "ok"),
                tool_result_block("t2", "ok"),
            ]),
            // Turn 2: assistant calls more tools, but results were TRUNCATED
            assistant_blocks_msg(vec![
                tool_use_block("t3", "analyze"),
                tool_use_block("t4", "summarize"),
            ]),
            // Context manager dropped the tool_result messages here
            user_msg("What did you find?"),
            assistant_msg("Based on my analysis..."),
        ];
        sanitize_message_sequence(&mut messages);

        // Verify: t3 and t4 have placeholder results
        // After sanitization, find the assistant with t3/t4
        let assistant_idx = messages
            .iter()
            .position(|m| {
                m.role == "assistant"
                    && extract_tool_use_ids(m)
                        .iter()
                        .any(|id| id == "t3" || id == "t4")
            })
            .expect("Should find assistant with t3/t4");

        let next = &messages[assistant_idx + 1];
        assert_eq!(next.role, "user");
        let result_ids = extract_tool_result_ids(next);
        assert!(result_ids.contains("t3"), "Placeholder for t3");
        assert!(result_ids.contains("t4"), "Placeholder for t4");

        // Verify conversation is structurally valid
        assert_eq!(messages[0].role, "user", "Must start with user");

        // The trailing assistant "Based on my analysis..." is substantive,
        // so it's preserved as prefill (not removed).
        let last = messages.last().expect("non-empty");
        assert!(
            last.role == "user" || last.role == "assistant",
            "Must end with user or substantive assistant"
        );

        // Verify alternating roles
        for window in messages.windows(2) {
            assert_ne!(
                window[0].role, window[1].role,
                "Roles must alternate: {:?} followed by {:?}",
                window[0].role, window[1].role
            );
        }
    }

    #[test]
    fn test_sanitize_sequence_preserves_valid_tool_results() {
        // Ensure sanitization doesn't remove valid tool_results
        let mut messages = vec![
            user_msg("Hello"),
            assistant_blocks_msg(vec![
                tool_use_block("t1", "search"),
                tool_use_block("t2", "fetch"),
            ]),
            user_blocks_msg(vec![
                tool_result_block("t1", "found it"),
                tool_result_block("t2", "fetched it"),
            ]),
            assistant_msg("Here's what I found"),
            user_msg("Thanks"),
        ];
        let original_len = messages.len();
        sanitize_message_sequence(&mut messages);

        // Should remain unchanged
        assert_eq!(messages.len(), original_len);

        // Verify tool_results are preserved
        let result_ids = extract_tool_result_ids(&messages[2]);
        assert!(result_ids.contains("t1"));
        assert!(result_ids.contains("t2"));
    }

    // --- dedup_tool_results tests ---

    #[test]
    fn test_dedup_tool_results_removes_duplicates() {
        // API error: "each tool_use must have a single result. Found multiple
        // `tool_result` blocks with id: t1"
        let mut messages = vec![
            user_msg("Hi"),
            assistant_blocks_msg(vec![tool_use_block("t1", "test_tool")]),
            user_blocks_msg(vec![
                tool_result_block("t1", "first"),
                tool_result_block("t1", "second"),
            ]),
        ];
        dedup_tool_results(&mut messages);

        if let AnthropicMessageContent::Blocks(blocks) = &messages[2].content {
            let results: Vec<_> = blocks
                .iter()
                .filter(|b| matches!(b, AnthropicContent::ToolResult { .. }))
                .collect();
            assert_eq!(results.len(), 1, "Should keep only one tool_result per ID");
            // Should keep the LAST one
            match &results[0] {
                AnthropicContent::ToolResult { content, .. } => match content {
                    Some(AnthropicMessageContent::String(s)) => {
                        assert_eq!(s, "second", "Should keep last result");
                    }
                    _ => panic!("Expected string content"),
                },
                _ => panic!("Expected ToolResult"),
            }
        } else {
            panic!("Expected Blocks");
        }
    }

    #[test]
    fn test_dedup_tool_results_preserves_different_ids() {
        let mut messages = vec![user_blocks_msg(vec![
            tool_result_block("t1", "result1"),
            tool_result_block("t2", "result2"),
        ])];
        dedup_tool_results(&mut messages);

        if let AnthropicMessageContent::Blocks(blocks) = &messages[0].content {
            assert_eq!(blocks.len(), 2, "Different IDs should both be kept");
        }
    }

    #[test]
    fn test_dedup_tool_results_preserves_non_tool_blocks() {
        let mut messages = vec![user_blocks_msg(vec![
            text_block("hello"),
            tool_result_block("t1", "first"),
            tool_result_block("t1", "second"),
            text_block("world"),
        ])];
        dedup_tool_results(&mut messages);

        if let AnthropicMessageContent::Blocks(blocks) = &messages[0].content {
            assert_eq!(blocks.len(), 3, "2 text blocks + 1 deduped tool_result");
            assert!(matches!(&blocks[0], AnthropicContent::Text { text, .. } if text == "hello"));
            assert!(matches!(&blocks[1], AnthropicContent::ToolResult { .. }));
            assert!(matches!(&blocks[2], AnthropicContent::Text { text, .. } if text == "world"));
        }
    }

    #[test]
    fn test_dedup_skips_assistant_messages() {
        // dedup only applies to user messages (tool_results are in user messages)
        let mut messages = vec![assistant_blocks_msg(vec![
            tool_use_block("t1", "a"),
            tool_use_block("t1", "a"), // weird but not tool_result
        ])];
        let original_len = match &messages[0].content {
            AnthropicMessageContent::Blocks(b) => b.len(),
            _ => panic!(),
        };
        dedup_tool_results(&mut messages);
        match &messages[0].content {
            AnthropicMessageContent::Blocks(b) => assert_eq!(b.len(), original_len),
            _ => panic!(),
        }
    }

    // --- remove_empty_content_messages tests ---

    #[test]
    fn test_remove_empty_string_content() {
        // API error: "all messages must have non-empty content"
        let mut messages = vec![user_msg("Hello"), assistant_msg(""), user_msg("World")];
        remove_empty_content_messages(&mut messages);
        assert_eq!(messages.len(), 2);
        match &messages[0].content {
            AnthropicMessageContent::String(s) => assert_eq!(s, "Hello"),
            _ => panic!(),
        }
        match &messages[1].content {
            AnthropicMessageContent::String(s) => assert_eq!(s, "World"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_remove_empty_blocks_content() {
        // API error: "all messages must have non-empty content"
        let mut messages = vec![
            user_msg("Hello"),
            AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicMessageContent::Blocks(vec![]),
            },
            user_msg("World"),
        ];
        remove_empty_content_messages(&mut messages);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_remove_empty_preserves_non_empty() {
        let mut messages = vec![user_msg("Hello"), assistant_msg("Hi"), user_msg("Bye")];
        remove_empty_content_messages(&mut messages);
        assert_eq!(messages.len(), 3);
    }

    // --- sanitize_message_sequence integration with new steps ---

    #[test]
    fn test_sanitize_sequence_dedup_tool_results() {
        // End-to-end: duplicate tool_results should be deduped
        let mut messages = vec![
            user_msg("Hi"),
            assistant_blocks_msg(vec![tool_use_block("t1", "tool_a")]),
            user_blocks_msg(vec![
                tool_result_block("t1", "first"),
                tool_result_block("t1", "second"),
            ]),
        ];
        sanitize_message_sequence(&mut messages);

        // Should have exactly 1 tool_result for t1
        let result_ids = extract_tool_result_ids(&messages[2]);
        assert_eq!(result_ids.len(), 1);
        assert!(result_ids.contains("t1"));

        // Count actual tool_result blocks
        if let AnthropicMessageContent::Blocks(blocks) = &messages[2].content {
            let count = blocks
                .iter()
                .filter(|b| matches!(b, AnthropicContent::ToolResult { .. }))
                .count();
            assert_eq!(count, 1, "Only 1 tool_result block after dedup");
        }
    }

    #[test]
    fn test_sanitize_sequence_removes_empty_content_preserves_substantive_assistant() {
        // Empty user messages should be removed; substantive trailing
        // assistant is preserved as prefill
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("Response"),
            user_msg(""), // empty — should be removed
        ];
        sanitize_message_sequence(&mut messages);

        // Empty user removed → trailing assistant "Response" is substantive → kept
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_sanitize_sequence_removes_empty_content_and_empty_trailing_assistant() {
        // Both empty user AND empty trailing assistant
        let mut messages = vec![
            user_msg("Hello"),
            assistant_msg("  "), // whitespace only
            user_msg(""),        // empty
        ];
        sanitize_message_sequence(&mut messages);

        // Empty user removed → whitespace assistant removed → just "Hello"
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
    }
}
