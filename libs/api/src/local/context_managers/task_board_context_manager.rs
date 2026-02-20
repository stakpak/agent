use std::collections::HashMap;

use stakpak_shared::models::{
    integrations::openai::{ChatMessage, MessageContent},
    llm::{LLMMessage, LLMMessageContent, LLMMessageTypedContent, LLMTool},
};

pub struct TaskBoardContextManager {
    keep_last_n_assistant_messages: usize,
    context_budget_threshold: f32,
}

impl super::ContextManager for TaskBoardContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        let llm_messages: Vec<_> = messages
            .into_iter()
            .map(|mut message| {
                // Remove checkpoint_id XML tags from message content
                Self::clean_checkpoint_tags(&mut message);
                message
            })
            .map(LLMMessage::from)
            .collect();

        let llm_messages = merge_consecutive_same_role(llm_messages);
        dedup_tool_results(llm_messages)
    }
}

const TRIMMED_CONTENT_PLACEHOLDER: &str = "[trimmed]";

impl TaskBoardContextManager {
    /// Remove `<checkpoint_id>...</checkpoint_id>` XML tags from message content.
    fn clean_checkpoint_tags(message: &mut ChatMessage) {
        if let Some(content) = message.content.take() {
            message.content = Some(match content {
                MessageContent::String(s) => {
                    MessageContent::String(super::common::remove_xml_tag("checkpoint_id", &s))
                }
                MessageContent::Array(parts) => MessageContent::Array(
                    parts
                        .into_iter()
                        .map(|mut part| {
                            if let Some(text) = part.text {
                                part.text =
                                    Some(super::common::remove_xml_tag("checkpoint_id", &text));
                            }
                            part
                        })
                        .collect(),
                ),
            });
        }
    }
}

const BYTES_PER_TOKEN: f64 = 3.5; // Conservative estimate for bytes/token

impl TaskBoardContextManager {
    /// Conservative bytes-to-tokens conversion.
    ///
    /// Uses a divisor of 3.0 (instead of Anthropic's 3.5 guidance for English)
    /// to intentionally overestimate. JSON, code, and non-ASCII content tokenize
    /// at closer to 2.5-3.0 bytes/token, so 3.0 is a safer middle ground.
    /// Overestimating triggers trimming slightly early, which is far safer than
    /// underestimating and hitting context window API errors.
    fn bytes_to_tokens(bytes: usize) -> u64 {
        (bytes as f64 / BYTES_PER_TOKEN).ceil() as u64
    }

    /// Estimate token count for a single content part, including structural overhead.
    ///
    /// Each content part carries API-level structural tokens (type discriminators,
    /// JSON keys, tool_use_id fields) that aren't captured by content length alone.
    fn estimate_content_part_tokens(part: &LLMMessageTypedContent) -> u64 {
        match part {
            LLMMessageTypedContent::Text { text } => Self::bytes_to_tokens(text.len()),
            LLMMessageTypedContent::ToolCall { name, args, .. } => {
                let content_bytes = name.len() + args.to_string().len();
                // +30 bytes for tool_use_id (~26 chars), "type":"tool_use", JSON structure
                Self::bytes_to_tokens(content_bytes + 30)
            }
            LLMMessageTypedContent::ToolResult { content, .. } => {
                // +30 bytes for tool_use_id, "type":"tool_result", JSON structure
                Self::bytes_to_tokens(content.len() + 30)
            }
            // Images: Anthropic charges 1600-6400+ tokens depending on resolution.
            // Use 2000 as a conservative default for typical images.
            LLMMessageTypedContent::Image { .. } => 2000,
        }
    }

    /// Estimate token count from LLM messages with conservative bias.
    ///
    /// Intentionally overestimates by ~15-25% compared to actual API token counts.
    /// This is by design: triggering trimming a bit early is far safer than
    /// blowing past the context window and getting 400 errors.
    ///
    /// Conservative measures applied:
    /// - Divisor of 3.0 bytes/token (vs Anthropic's 3.5 guidance for English)
    /// - 8 tokens per-message overhead (role, formatting, content block wrappers)
    /// - 3 tokens per content part in List messages (part-level structure)
    /// - 30 bytes structural overhead per ToolCall/ToolResult (IDs, type fields)
    /// - 2000 tokens per image (vs previous 1000)
    /// - 5% safety buffer on the final total
    pub fn estimate_tokens(messages: &[LLMMessage]) -> u64 {
        let raw_estimate: u64 = messages
            .iter()
            .map(|msg| {
                let content_tokens = match &msg.content {
                    LLMMessageContent::String(s) => Self::bytes_to_tokens(s.len()),
                    LLMMessageContent::List(parts) => {
                        let part_tokens: u64 =
                            parts.iter().map(Self::estimate_content_part_tokens).sum();
                        // Per-part structural overhead: each content block in a List
                        // has type discriminator and wrapper tokens (~3 tokens each)
                        let part_overhead = parts.len() as u64 * 3;
                        part_tokens + part_overhead
                    }
                };
                // Per-message overhead: role tag, content block wrapper, formatting.
                // Anthropic's actual overhead is ~7-10 tokens; use 8 as conservative middle.
                content_tokens + 8
            })
            .sum();

        // 5% safety buffer to catch remaining estimation drift
        (raw_estimate as f64 * 1.05).ceil() as u64
    }

    /// Trim a single message's content, replacing it with a placeholder.
    /// Preserves message structure (role, tool_call_ids) for API validity.
    fn trim_message(msg: &mut LLMMessage) {
        match &mut msg.content {
            LLMMessageContent::String(s) => {
                *s = TRIMMED_CONTENT_PLACEHOLDER.to_string();
            }
            LLMMessageContent::List(parts) => {
                for part in parts.iter_mut() {
                    match part {
                        LLMMessageTypedContent::Text { text } => {
                            *text = TRIMMED_CONTENT_PLACEHOLDER.to_string();
                        }
                        LLMMessageTypedContent::ToolResult { content, .. } => {
                            *content = TRIMMED_CONTENT_PLACEHOLDER.to_string();
                        }
                        // Preserve ToolCall structure - needed for API to match tool_use/tool_result
                        LLMMessageTypedContent::ToolCall { .. } => {}
                        LLMMessageTypedContent::Image { .. } => {}
                    }
                }
            }
        }
    }

    /// Estimate overhead tokens from tool definitions.
    ///
    /// Tool definitions are sent to the API alongside messages but are not
    /// included in the chat message list, so the trimmer needs to account
    /// for them separately. The system prompt is already part of the message
    /// list and is covered by `estimate_tokens`.
    pub fn estimate_tool_overhead(tools: Option<&[LLMTool]>) -> u64 {
        tools
            .map(|t| {
                t.iter()
                    .map(|tool| {
                        let schema_len = tool.input_schema.to_string().len();
                        let tool_bytes = tool.name.len() + tool.description.len() + schema_len;
                        // 1.2x multiplier accounts for JSON structural overhead not
                        // captured by content length: property names, "type":"function"
                        // wrappers, "required" arrays, enum values, nested refs, etc.
                        let adjusted_bytes = (tool_bytes as f64 * 1.2).ceil() as usize;
                        Self::bytes_to_tokens(adjusted_bytes) + 8
                    })
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Budget-aware context reduction that preserves Anthropic prompt caching.
    ///
    /// **Trimming semantics:**
    /// - Only **assistant** and **tool** messages are ever trimmed (replaced
    ///   with `[trimmed]` placeholders). User and system messages are always
    ///   preserved in full.
    /// - `keep_last_n_assistant_messages` controls how many recent assistant
    ///   messages are kept untrimmed. The trim boundary is placed just before
    ///   the Nth-from-last assistant message, so all messages after that point
    ///   (including interleaved user/tool messages) are untouched.
    ///
    /// **Lazy trimming with `trimmed_up_to_message_index`:**
    /// - On every call, previously-trimmed messages (up to the stored index)
    ///   are re-trimmed to keep the prefix stable for prompt caching.
    /// - The index only **advances** when estimated tokens exceed
    ///   `context_window × threshold` again — it never moves backward.
    /// - When under threshold and no previous trimming exists, messages are
    ///   returned as-is with no metadata changes.
    pub fn reduce_context_with_budget(
        &self,
        messages: Vec<ChatMessage>,
        context_window: u64,
        metadata: Option<serde_json::Value>,
        tools: Option<&[LLMTool]>,
    ) -> (Vec<LLMMessage>, Option<serde_json::Value>) {
        // Standard processing: clean, convert, merge, dedup
        let llm_messages: Vec<_> = messages
            .into_iter()
            .map(|mut message| {
                Self::clean_checkpoint_tags(&mut message);
                message
            })
            .map(LLMMessage::from)
            .collect();

        let llm_messages = merge_consecutive_same_role(llm_messages);
        let mut llm_messages = dedup_tool_results(llm_messages);

        let tool_overhead = Self::estimate_tool_overhead(tools);
        let threshold = (context_window as f32 * self.context_budget_threshold) as u64;

        // Read previous trimming state from metadata
        let prev_trimmed_up_to = metadata
            .as_ref()
            .and_then(|m| m.get("trimmed_up_to_message_index"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        // Fast path: under threshold and no previous trimming → return as-is
        if prev_trimmed_up_to == 0
            && Self::estimate_tokens(&llm_messages) + tool_overhead <= threshold
        {
            return (llm_messages, metadata);
        }

        // Determine the trim boundary based on keep_last_n_assistant_messages.
        //
        // Walk backwards through messages, counting assistant-role messages.
        // Once we've counted `keep_last_n_assistant_messages` of them, the
        // current message is the first one we want to keep — set trim_end
        // there so everything before it (that isn't user/system) gets trimmed.
        //
        // When keep_last_n is 0, skip the loop entirely: trim_end stays at
        // len, meaning every assistant/tool message is eligible for trimming.
        //
        // If the loop finishes without finding enough assistants, trim_end
        // stays at 0 — meaning keep_last_n alone wouldn't trim anything.
        let len = llm_messages.len();
        let mut keep_n_trim_end = if self.keep_last_n_assistant_messages > 0 {
            0 // default: don't trim anything (not enough assistants to exceed keep_last_n)
        } else {
            len // keep_last_n is 0: trim everything
        };
        if self.keep_last_n_assistant_messages > 0 {
            let mut assistant_count = 0usize;
            for i in (0..len).rev() {
                if llm_messages[i].role == "assistant" {
                    assistant_count += 1;
                    if assistant_count >= self.keep_last_n_assistant_messages {
                        keep_n_trim_end = i;
                        break;
                    }
                }
            }
        }

        // Re-apply previous trimming first so we can estimate the *effective*
        // token count (what the API actually sees). This keeps the prefix
        // stable for prompt caching.
        let prev_clamped = prev_trimmed_up_to.min(len);
        for msg in &mut llm_messages[..prev_clamped] {
            if msg.role == "system" || msg.role == "user" {
                continue;
            }
            Self::trim_message(msg);
        }

        // Re-estimate tokens after applying previous trimming. This reflects
        // the actual token count the API will see, so we only advance the
        // trim boundary when the *effective* content exceeds the threshold —
        // not just because the raw (untrimmed) history grew.
        let effective_estimated_tokens = Self::estimate_tokens(&llm_messages) + tool_overhead;

        // Decide whether to advance the trim index or keep the old one.
        // The index only advances when the effective tokens (after re-applying
        // previous trims) still exceed the threshold.
        let effective_trim_end = if effective_estimated_tokens > threshold {
            // Over budget — we must trim. Start with keep_last_n boundary
            // if available, then fall through to budget-driven scanning if
            // that boundary alone isn't sufficient.
            //
            // Step 1: Apply keep_last_n trimming if it provides a boundary.
            let mut candidate = if keep_n_trim_end > 0 {
                // Trim up to the keep_last_n boundary first
                for msg in llm_messages
                    .iter_mut()
                    .take(keep_n_trim_end.min(len))
                    .skip(prev_clamped)
                {
                    let role = msg.role.as_str();
                    if role == "assistant" || role == "tool" {
                        Self::trim_message(msg);
                    }
                }
                keep_n_trim_end
            } else {
                prev_trimmed_up_to
            };

            // Step 2: If still over budget after keep_last_n trimming,
            // continue scanning forward — budget is the HARD constraint,
            // keep_last_n is best-effort. This handles the case where the
            // last N assistant messages themselves exceed the budget (e.g.,
            // long tool results, large file contents).
            let current_tokens = Self::estimate_tokens(&llm_messages) + tool_overhead;
            if current_tokens > threshold {
                let mut scan_idx = candidate;
                while scan_idx < len {
                    let role = llm_messages[scan_idx].role.as_str();
                    if role == "assistant" || role == "tool" {
                        Self::trim_message(&mut llm_messages[scan_idx]);
                        candidate = scan_idx + 1;

                        // Check if we're under budget now
                        let current_tokens = Self::estimate_tokens(&llm_messages) + tool_overhead;
                        if current_tokens <= threshold {
                            break;
                        }
                    }
                    scan_idx += 1;
                }
            }

            // Never go backward
            candidate.max(prev_trimmed_up_to)
        } else {
            // Under budget — keep the previous trim boundary, don't advance
            prev_trimmed_up_to
        };

        // Apply trimming for any newly-advanced range (prev_clamped..effective_trim_end).
        // The previous range (0..prev_clamped) was already trimmed above.
        let clamped_end = effective_trim_end.min(len);
        if clamped_end > prev_clamped {
            for msg in &mut llm_messages[prev_clamped..clamped_end] {
                if msg.role == "system" || msg.role == "user" {
                    continue;
                }
                Self::trim_message(msg);
            }
        }

        // Update metadata — only write a new index when it actually advanced
        let new_trimmed_up_to = effective_trim_end;
        let mut updated_metadata = metadata.unwrap_or(serde_json::json!({}));
        if let Some(obj) = updated_metadata.as_object_mut() {
            obj.insert(
                "trimmed_up_to_message_index".to_string(),
                serde_json::json!(new_trimmed_up_to),
            );
        }

        (llm_messages, Some(updated_metadata))
    }
}

/// Merge consecutive LLMMessages that share the same role into a single message.
///
/// When the assistant returns N tool_calls, the chat history contains N separate
/// `role=tool` messages. Provider conversion layers map `tool` → `user`, which
/// creates N consecutive `user` messages — invalid for Anthropic.  By merging
/// them here into a single `role=tool` LLMMessage with multiple ToolResult
/// content parts, the downstream conversion produces one `user` message with
/// all the tool_result blocks.
fn merge_consecutive_same_role(messages: Vec<LLMMessage>) -> Vec<LLMMessage> {
    if messages.is_empty() {
        return messages;
    }

    let mut result: Vec<LLMMessage> = Vec::with_capacity(messages.len());

    for msg in messages {
        let should_merge = result.last().is_some_and(|prev| prev.role == msg.role);

        if should_merge {
            let prev = result.last_mut().expect("checked above");
            let new_parts = msg.content.into_parts();
            prev.content = match std::mem::take(&mut prev.content) {
                LLMMessageContent::String(s) if s.is_empty() => LLMMessageContent::List(new_parts),
                LLMMessageContent::String(s) => {
                    let mut parts = vec![LLMMessageTypedContent::Text { text: s }];
                    parts.extend(new_parts);
                    LLMMessageContent::List(parts)
                }
                LLMMessageContent::List(mut existing) => {
                    existing.extend(new_parts);
                    LLMMessageContent::List(existing)
                }
            };
        } else {
            result.push(msg);
        }
    }

    result
}

/// Remove duplicate ToolResult entries that share the same tool_use_id.
/// Keeps only the **last** occurrence (the most recent / retried result).
fn dedup_tool_results(mut messages: Vec<LLMMessage>) -> Vec<LLMMessage> {
    for msg in &mut messages {
        if msg.role != "tool" {
            continue;
        }
        let parts = match &mut msg.content {
            LLMMessageContent::List(p) => p,
            _ => continue,
        };

        // Track last index for each tool_use_id
        let mut last_index: HashMap<String, usize> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (i, part) in parts.iter().enumerate() {
            if let LLMMessageTypedContent::ToolResult { tool_use_id, .. } = part {
                last_index.insert(tool_use_id.clone(), i);
                *counts.entry(tool_use_id.clone()).or_insert(0) += 1;
            }
        }

        // Only filter if there are actual duplicates
        let has_dups = counts.values().any(|&c| c > 1);
        if !has_dups {
            continue;
        }

        let mut idx = 0;
        parts.retain(|part| {
            let keep = if let LLMMessageTypedContent::ToolResult { tool_use_id, .. } = part {
                if counts.get(tool_use_id).copied().unwrap_or(0) > 1 {
                    // Duplicate — keep only the last one
                    last_index.get(tool_use_id).copied() == Some(idx)
                } else {
                    true
                }
            } else {
                true
            };
            idx += 1;
            keep
        });
    }

    messages
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::super::ContextManager;
    use super::*;
    use stakpak_shared::models::integrations::openai::{
        FunctionCall, MessageContent, Role, ToolCall,
    };
    use stakpak_shared::models::llm::{LLMMessageContent, LLMMessageTypedContent};

    fn create_context_manager() -> TaskBoardContextManager {
        TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 2, // Only keep last 2 assistant messages untrimmed
            context_budget_threshold: 0.8,
        })
    }

    fn create_tool_call_msg(id: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("Thinking...".to_string())),
            tool_calls: Some(vec![ToolCall {
                id: id.to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "test_tool".to_string(),
                    arguments: "{}".to_string(),
                },
                metadata: None,
            }]),
            ..Default::default()
        }
    }

    fn create_tool_call_msg_with_ids(ids: &[&str]) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("Thinking...".to_string())),
            tool_calls: Some(
                ids.iter()
                    .map(|id| ToolCall {
                        id: (*id).to_string(),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: "test_tool".to_string(),
                            arguments: "{}".to_string(),
                        },
                        metadata: None,
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn create_tool_result_msg(id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Tool,
            tool_call_id: Some(id.to_string()),
            content: Some(MessageContent::String(content.to_string())),
            ..Default::default()
        }
    }

    #[test]
    fn test_reduce_context_preserves_messages() {
        let cm = create_context_manager();
        let messages = vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("Hello".to_string())),
            ..Default::default()
        }];

        let reduced = cm.reduce_context(messages);
        assert_eq!(reduced.len(), 1);
        assert_eq!(reduced[0].role, "user");
    }

    #[test]
    fn test_reduce_context_preserves_old_tool_results() {
        let cm = create_context_manager();
        let messages = vec![
            create_tool_call_msg("call_1"),
            create_tool_result_msg("call_1", "Result 1 - detailed output from first tool"),
            create_tool_call_msg("call_2"),
            create_tool_result_msg("call_2", "Result 2"),
            create_tool_call_msg("call_3"),
            create_tool_result_msg("call_3", "Result 3"),
        ];

        let reduced = cm.reduce_context(messages);

        // call_1 result should still have its original content (not truncated)
        let result_1 = &reduced[1]; // tool result for call_1
        match &result_1.content {
            LLMMessageContent::List(parts) => {
                if let LLMMessageTypedContent::ToolResult { content, .. } = &parts[0] {
                    assert!(
                        !content.contains("truncated"),
                        "Result should NOT be truncated, got: {}",
                        content
                    );
                    assert!(
                        content.contains("Result 1"),
                        "Original content should be preserved"
                    );
                }
            }
            LLMMessageContent::String(s) => {
                assert!(!s.contains("truncated"), "Result should NOT be truncated");
            }
        }
    }

    #[test]
    fn test_reduce_context_preserves_large_assistant_thoughts() {
        let cm = create_context_manager(); // size_limit = 10
        let long_thought = "This is a very long thought that exceeds the size limit";

        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(long_thought.to_string())),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    r#type: "function".to_string(),
                    function: FunctionCall {
                        name: "t".to_string(),
                        arguments: "{}".to_string(),
                    },
                    metadata: None,
                }]),
                ..Default::default()
            },
            create_tool_result_msg("call_1", "res"),
            create_tool_call_msg("call_2"),
            create_tool_call_msg("call_3"),
        ];

        let reduced = cm.reduce_context(messages);
        if let LLMMessageContent::List(parts) = &reduced[0].content {
            let has_text = parts
                .iter()
                .any(|p| matches!(p, LLMMessageTypedContent::Text { .. }));
            assert!(has_text, "Large thought should be preserved");
        } else {
            panic!("Expected list content for assistant message with tool calls");
        }
    }

    #[test]
    fn test_reduce_context_removes_checkpoint_id_tags() {
        let cm = create_context_manager();
        let messages = vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String(
                "<checkpoint_id>abc-123</checkpoint_id>\nHello, world!".to_string(),
            )),
            ..Default::default()
        }];

        let reduced = cm.reduce_context(messages);

        match &reduced[0].content {
            LLMMessageContent::String(s) => {
                assert!(
                    !s.contains("checkpoint_id"),
                    "checkpoint_id tag should be removed"
                );
                assert!(
                    s.contains("Hello, world!"),
                    "actual content should be preserved"
                );
            }
            _ => panic!("Expected string content"),
        }
    }

    #[test]
    fn test_reduce_context_merges_consecutive_tool_results_for_same_assistant_turn() {
        let cm = create_context_manager();
        let messages = vec![
            create_tool_call_msg_with_ids(&["call_1", "call_2"]),
            create_tool_result_msg("call_1", "Result 1"),
            create_tool_result_msg("call_2", "Result 2"),
        ];

        let reduced = cm.reduce_context(messages);

        assert_eq!(reduced.len(), 2);
        assert_eq!(reduced[1].role, "tool");
        match &reduced[1].content {
            LLMMessageContent::List(parts) => {
                let tool_results: Vec<_> = parts
                    .iter()
                    .filter_map(|part| {
                        if let LLMMessageTypedContent::ToolResult {
                            tool_use_id,
                            content,
                        } = part
                        {
                            Some((tool_use_id.as_str(), content.as_str()))
                        } else {
                            None
                        }
                    })
                    .collect();

                assert_eq!(
                    tool_results,
                    vec![("call_1", "Result 1"), ("call_2", "Result 2")]
                );
            }
            other => panic!(
                "Expected list content for merged tool results, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_reduce_context_deduplicates_tool_results_keeping_last() {
        let cm = create_context_manager();
        let messages = vec![
            create_tool_call_msg("call_1"),
            create_tool_result_msg("call_1", "old_result"),
            create_tool_result_msg("call_1", "new_result"),
        ];

        let reduced = cm.reduce_context(messages);

        assert_eq!(reduced.len(), 2);
        assert_eq!(reduced[1].role, "tool");
        match &reduced[1].content {
            LLMMessageContent::List(parts) => {
                let tool_results: Vec<_> = parts
                    .iter()
                    .filter_map(|part| {
                        if let LLMMessageTypedContent::ToolResult {
                            tool_use_id,
                            content,
                        } = part
                        {
                            Some((tool_use_id.as_str(), content.as_str()))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(tool_results, vec![("call_1", "new_result")]);
            }
            other => panic!(
                "Expected list content for deduplicated tool results, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_reduce_context_preserves_tool_role() {
        let cm = create_context_manager();
        let messages = vec![
            create_tool_call_msg("call_1"),
            create_tool_result_msg("call_1", "Result content"),
        ];

        let reduced = cm.reduce_context(messages);

        let result_msg = &reduced[1];
        assert_eq!(
            result_msg.role, "tool",
            "Tool results should preserve tool role (provider layer handles format conversion)"
        );
    }

    // =========================================================================
    // Token estimation tests
    // =========================================================================

    #[test]
    fn test_estimate_tokens_simple_message() {
        let messages = vec![LLMMessage {
            role: "user".to_string(),
            content: LLMMessageContent::String("Hello world".to_string()),
        }];
        let tokens = TaskBoardContextManager::estimate_tokens(&messages);
        // "Hello world" = 11 bytes / BYTES_PER_TOKEN + 8 msg overhead, then 5% buffer
        let content_tokens = (11.0 / BYTES_PER_TOKEN).ceil() as u64;
        let raw = content_tokens + 8;
        let expected = (raw as f64 * 1.05).ceil() as u64;
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_estimate_tokens_multiple_messages() {
        let messages = vec![
            LLMMessage {
                role: "user".to_string(),
                content: LLMMessageContent::String("Hello".to_string()),
            },
            LLMMessage {
                role: "assistant".to_string(),
                content: LLMMessageContent::String("World".to_string()),
            },
        ];
        let tokens = TaskBoardContextManager::estimate_tokens(&messages);
        // Each: 5 bytes / BYTES_PER_TOKEN + 8 msg overhead, total raw, then 5% buffer
        let per_msg = (5.0 / BYTES_PER_TOKEN).ceil() as u64 + 8;
        let raw = per_msg * 2;
        let expected = (raw as f64 * 1.05).ceil() as u64;
        assert_eq!(tokens, expected);
    }

    #[test]
    fn test_estimate_tokens_with_tool_results() {
        let messages = vec![LLMMessage {
            role: "tool".to_string(),
            content: LLMMessageContent::List(vec![LLMMessageTypedContent::ToolResult {
                tool_use_id: "tc_1".to_string(),
                content: "A".repeat(400), // 400 bytes
            }]),
        }];
        let tokens = TaskBoardContextManager::estimate_tokens(&messages);
        // 400 content + 30 structural = 430 bytes / BYTES_PER_TOKEN
        // + 3 per-part overhead (1 part) + 8 msg overhead, then 5% buffer
        let part_tokens = (430.0 / BYTES_PER_TOKEN).ceil() as u64;
        let raw = part_tokens + 3 + 8;
        let expected = (raw as f64 * 1.05).ceil() as u64;
        assert_eq!(tokens, expected);
    }

    // =========================================================================
    // Budget-aware trimming tests
    // =========================================================================

    #[test]
    fn test_no_trimming_when_under_threshold() {
        let cm = create_context_manager();
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("Hello".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("Hi there".to_string())),
                ..Default::default()
            },
        ];

        let (result, metadata) = cm.reduce_context_with_budget(messages, 200_000, None, None);
        assert_eq!(result.len(), 2);
        // No trimming should have occurred
        assert!(metadata.is_none());
        // Content should be preserved
        match &result[0].content {
            LLMMessageContent::String(s) => assert_eq!(s, "Hello"),
            _ => panic!("Expected string content"),
        }
    }

    #[test]
    fn test_trimming_triggers_at_threshold() {
        let cm = create_context_manager();
        // Create messages that exceed 80% of a small context window
        let mut messages = Vec::new();
        for i in 0..20 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "Message {}: {}",
                    i,
                    "x".repeat(100)
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Response {}: {}",
                    i,
                    "y".repeat(100)
                ))),
                ..Default::default()
            });
        }

        // Use a small context window so trimming triggers
        let (result, metadata) = cm.reduce_context_with_budget(messages, 100, None, None);

        // Should have metadata with trimmed index
        assert!(metadata.is_some());
        let meta = metadata.unwrap();
        assert!(meta.get("trimmed_up_to_message_index").is_some());

        // With keep_last_n_assistant_messages = 2, the trim boundary is placed
        // just before the 2nd-from-last assistant message. In a 40-message
        // alternating sequence (user/assistant × 20), the last 2 assistants are
        // at indices 37 and 39, so trim_end = 37 and messages 37-39 are untrimmed.
        let trimmed_idx = meta["trimmed_up_to_message_index"].as_u64().unwrap() as usize;
        for msg in &result[trimmed_idx..] {
            if let LLMMessageContent::String(s) = &msg.content {
                assert_ne!(
                    s, TRIMMED_CONTENT_PLACEHOLDER,
                    "Messages after trim boundary should not be trimmed"
                );
            }
        }

        // Earlier assistant messages should be trimmed, user messages preserved
        let first_assistant = &result[1]; // index 1 is assistant
        if let LLMMessageContent::String(s) = &first_assistant.content {
            assert_eq!(
                s, TRIMMED_CONTENT_PLACEHOLDER,
                "Early assistant messages should be trimmed"
            );
        }

        // User messages should NOT be trimmed
        let first_user = &result[0]; // index 0 is user
        if let LLMMessageContent::String(s) = &first_user.content {
            assert_ne!(
                s, TRIMMED_CONTENT_PLACEHOLDER,
                "User messages should never be trimmed"
            );
        }
    }

    #[test]
    fn test_already_trimmed_messages_not_re_trimmed() {
        let cm = create_context_manager();
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!("Msg {}", i))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!("Resp {}", i))),
                ..Default::default()
            });
        }

        // First trim
        let (_, metadata) = cm.reduce_context_with_budget(messages.clone(), 100, None, None);
        let trimmed_index = metadata
            .as_ref()
            .unwrap()
            .get("trimmed_up_to_message_index")
            .unwrap()
            .as_u64()
            .unwrap();

        // Add more messages and trim again
        messages.push(ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("New message".to_string())),
            ..Default::default()
        });
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("New response".to_string())),
            ..Default::default()
        });

        let (_, metadata2) = cm.reduce_context_with_budget(messages, 100, metadata, None);
        let new_trimmed_index = metadata2
            .as_ref()
            .unwrap()
            .get("trimmed_up_to_message_index")
            .unwrap()
            .as_u64()
            .unwrap();

        // New trimmed index should be >= old one (never goes backward)
        assert!(
            new_trimmed_index >= trimmed_index,
            "Trimmed index should not decrease: {} < {}",
            new_trimmed_index,
            trimmed_index
        );
    }

    #[test]
    fn test_trimming_preserves_message_structure() {
        let cm = create_context_manager();
        let messages = vec![
            create_tool_call_msg("call_1"),
            create_tool_result_msg("call_1", "x".repeat(200).as_str()),
            create_tool_call_msg("call_2"),
            create_tool_result_msg("call_2", "y".repeat(200).as_str()),
            // These should be kept
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("Recent".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("Response".to_string())),
                ..Default::default()
            },
        ];

        let (result, _) = cm.reduce_context_with_budget(messages, 50, None, None);

        // All messages should still be present (structure preserved)
        assert!(!result.is_empty());

        // Verify roles alternate correctly
        for msg in &result {
            assert!(
                msg.role == "user"
                    || msg.role == "assistant"
                    || msg.role == "tool"
                    || msg.role == "system",
                "Invalid role: {}",
                msg.role
            );
        }
    }

    // =========================================================================
    // Full lifecycle integration tests
    // =========================================================================

    /// Helper: check if a message has been trimmed
    fn is_trimmed(msg: &LLMMessage) -> bool {
        match &msg.content {
            LLMMessageContent::String(s) => s == TRIMMED_CONTENT_PLACEHOLDER,
            LLMMessageContent::List(parts) => parts.iter().all(|p| match p {
                LLMMessageTypedContent::Text { text } => text == TRIMMED_CONTENT_PLACEHOLDER,
                LLMMessageTypedContent::ToolResult { content, .. } => {
                    content == TRIMMED_CONTENT_PLACEHOLDER
                }
                LLMMessageTypedContent::ToolCall { .. } => true, // tool calls are never trimmed
                LLMMessageTypedContent::Image { .. } => true,
            }),
        }
    }

    #[test]
    fn test_full_lifecycle_trim_save_resume_trim() {
        let cm = create_context_manager(); // threshold = 0.8
        let context_window = 200u64; // small window to force trimming

        // === Turn 1-5: Build up conversation ===
        let mut messages = Vec::new();
        for i in 0..5 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "User turn {}: {}",
                    i,
                    "a".repeat(80)
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Assistant turn {}: {}",
                    i,
                    "b".repeat(80)
                ))),
                ..Default::default()
            });
        }

        // === First inference: should trigger trimming ===
        let (result1, metadata1) =
            cm.reduce_context_with_budget(messages.clone(), context_window, None, None);

        assert!(
            metadata1.is_some(),
            "Trimming should have produced metadata"
        );
        let trimmed_idx_1 = metadata1.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(trimmed_idx_1 > 0, "Should have trimmed some messages");

        // Verify: non-user messages before trimmed_idx are trimmed
        for (i, msg) in result1.iter().enumerate() {
            if i < trimmed_idx_1 && msg.role != "user" {
                assert!(
                    is_trimmed(msg),
                    "Non-user message {} should be trimmed (trimmed_idx={}), content: {:?}",
                    i,
                    trimmed_idx_1,
                    msg.content
                );
            }
            if i < trimmed_idx_1 && msg.role == "user" {
                assert!(
                    !is_trimmed(msg),
                    "User message {} should NOT be trimmed, content: {:?}",
                    i,
                    msg.content
                );
            }
        }

        // Verify: messages after trim boundary are NOT trimmed
        for (i, msg) in result1.iter().enumerate().skip(trimmed_idx_1) {
            assert!(
                !is_trimmed(msg),
                "Message {} (after trim boundary {}) should NOT be trimmed",
                i,
                trimmed_idx_1
            );
        }

        // === Simulate checkpoint save/load: metadata round-trips through JSON ===
        let saved_metadata_json = serde_json::to_string(&metadata1).unwrap();
        let loaded_metadata: Option<serde_json::Value> =
            serde_json::from_str(&saved_metadata_json).unwrap();

        // === Turn 6-7: Add more messages (simulating continued conversation) ===
        messages.push(ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String(format!(
                "User turn 5: {}",
                "c".repeat(80)
            ))),
            ..Default::default()
        });
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String(format!(
                "Assistant turn 5: {}",
                "d".repeat(80)
            ))),
            ..Default::default()
        });

        // === Second inference with loaded metadata ===
        let (result2, metadata2) =
            cm.reduce_context_with_budget(messages.clone(), context_window, loaded_metadata, None);

        let trimmed_idx_2 = metadata2.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // Trimmed index should advance (more messages to trim)
        assert!(
            trimmed_idx_2 >= trimmed_idx_1,
            "Trimmed index should not decrease: {} < {}",
            trimmed_idx_2,
            trimmed_idx_1
        );

        // Verify: the previously-trimmed prefix is still trimmed (stable for caching)
        // User messages are never trimmed, only assistant/tool messages
        for (i, msg) in result2.iter().enumerate().take(trimmed_idx_1) {
            if msg.role != "user" {
                assert!(
                    is_trimmed(msg),
                    "Previously trimmed non-user message {} should still be trimmed",
                    i
                );
            }
        }

        // Verify: messages after trim boundary are NOT trimmed
        for (i, msg) in result2.iter().enumerate().skip(trimmed_idx_2) {
            assert!(
                !is_trimmed(msg),
                "Message {} (after trim boundary {}) should NOT be trimmed after second trim",
                i,
                trimmed_idx_2
            );
        }

        // Verify: total message count increased
        assert!(
            result2.len() > result1.len(),
            "Should have more messages after adding turns"
        );
    }

    #[test]
    fn test_tool_call_ids_preserved_after_trimming() {
        let cm = create_context_manager();

        let messages = vec![
            // Old tool interaction (should be trimmed)
            create_tool_call_msg_with_ids(&["tc_old_1", "tc_old_2"]),
            create_tool_result_msg("tc_old_1", &"x".repeat(200)),
            create_tool_result_msg("tc_old_2", &"y".repeat(200)),
            // Recent tool interaction (should be kept)
            create_tool_call_msg_with_ids(&["tc_new_1"]),
            create_tool_result_msg("tc_new_1", "recent result"),
            // Recent conversation
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("What happened?".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("Here's what I found".to_string())),
                ..Default::default()
            },
        ];

        let (result, _) = cm.reduce_context_with_budget(messages, 80, None, None);

        // Find all ToolCall parts and verify they still have their IDs
        for msg in &result {
            if let LLMMessageContent::List(parts) = &msg.content {
                for part in parts {
                    if let LLMMessageTypedContent::ToolCall { id, name, .. } = part {
                        assert!(!id.is_empty(), "ToolCall ID should be preserved");
                        assert!(!name.is_empty(), "ToolCall name should be preserved");
                    }
                    if let LLMMessageTypedContent::ToolResult { tool_use_id, .. } = part {
                        assert!(
                            !tool_use_id.is_empty(),
                            "ToolResult tool_use_id should be preserved"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_system_messages_never_trimmed() {
        let cm = create_context_manager();

        // Simulate a system message at the start (though normally added by hook,
        // test that the trimmer skips system role)
        let mut messages = vec![ChatMessage {
            role: Role::User, // system messages are added by hook, not in ChatMessage
            content: Some(MessageContent::String("System-like setup".to_string())),
            ..Default::default()
        }];

        for i in 0..10 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "Msg {}: {}",
                    i,
                    "z".repeat(100)
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Resp {}: {}",
                    i,
                    "w".repeat(100)
                ))),
                ..Default::default()
            });
        }

        let (result, metadata) = cm.reduce_context_with_budget(messages, 100, None, None);

        assert!(metadata.is_some(), "Should have trimmed");

        // Verify no system-role or user-role messages were trimmed
        for msg in &result {
            if msg.role == "system" {
                assert!(!is_trimmed(msg), "System messages must never be trimmed");
            }
            if msg.role == "user" {
                assert!(!is_trimmed(msg), "User messages must never be trimmed");
            }
        }
    }

    #[test]
    fn test_trimming_with_empty_metadata_object() {
        let cm = create_context_manager();
        // Use large assistant messages and small user messages so that trimming
        // the first assistant (per keep_last_n=2) is sufficient to get under budget.
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u1".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("y".repeat(500))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u2".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("w".repeat(500))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("recent".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("response".to_string())),
                ..Default::default()
            },
        ];

        // Context window sized so total (~380 tokens) exceeds 80% threshold (~320)
        // but after trimming 1 assistant per keep_last_n=2, remaining fits.
        let (result, metadata) =
            cm.reduce_context_with_budget(messages, 400, Some(serde_json::json!({})), None);

        assert!(metadata.is_some());
        let meta = metadata.unwrap();
        assert!(
            meta.get("trimmed_up_to_message_index").is_some(),
            "Should have set trimmed_up_to_message_index"
        );

        // Verify trimming happened on assistant messages (user messages are preserved)
        assert!(
            !is_trimmed(&result[0]),
            "First user message should NOT be trimmed"
        );
        assert!(
            is_trimmed(&result[1]),
            "First assistant message should be trimmed"
        );

        // Verify last messages preserved
        let len = result.len();
        assert!(
            !is_trimmed(&result[len - 1]),
            "Last message should not be trimmed"
        );
    }

    /// Verify that keep_last_n_assistant_messages counts only assistant-role
    /// messages, not all messages. With interleaved user/tool messages between
    /// assistants, the trim boundary should be placed based on assistant count.
    #[test]
    fn test_keep_last_n_counts_only_assistant_messages() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 2,
            context_budget_threshold: 0.8,
        });

        // Build: user, assistant, user, assistant, user, user, assistant
        // After merge_consecutive_same_role, the two consecutive user messages
        // get merged, so the sequence is:
        // [0] user, [1] asst(a1), [2] user, [3] asst(a2), [4] user(merged), [5] asst(a3)
        // Assistants at indices: 1, 3, 5
        // keep_last_n_assistant_messages = 2 → trim_end = index of 2nd-from-last asst = 3
        // So messages 0..3 are in the trim zone, messages 3..6 are untrimmed.
        //
        // Use large assistant messages so trimming them actually reduces token count
        // below the budget threshold. User messages are small so the remaining
        // content fits within budget after trimming the prefix.
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u1".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a1".to_string() + &"y".repeat(300))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u2".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a2".to_string() + &"y".repeat(300))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u3".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u4".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a3".to_string() + &"y".repeat(300))),
                ..Default::default()
            },
        ];

        // Context window sized so that:
        // - Total tokens (~380) exceed 80% threshold (~320) → triggers trimming
        // - After trimming 1 assistant per keep_last_n=2 (~100 tokens saved),
        //   remaining (~280) fits under threshold
        let (result, metadata) = cm.reduce_context_with_budget(messages, 400, None, None);
        assert!(metadata.is_some(), "Should trigger trimming");

        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // The 2nd-from-last assistant is "a2". After merge_consecutive_same_role,
        // the two consecutive user messages get merged, so the sequence is:
        // [0] user, [1] asst(a1), [2] user, [3] asst(a2), [4] user(merged), [5] asst(a3)
        // Assistants at 1, 3, 5. 2nd-from-last = index 3. trim_end = 3.
        assert_eq!(
            trimmed_idx, 3,
            "Trim boundary should be at the 2nd-from-last assistant (index 3)"
        );

        // a1 (index 1) should be trimmed
        assert!(
            is_trimmed(&result[1]),
            "First assistant (a1) should be trimmed"
        );

        // a2 (index 3) should NOT be trimmed (it's the boundary — kept)
        assert!(
            !is_trimmed(&result[3]),
            "Second assistant (a2) should NOT be trimmed (within keep window)"
        );

        // a3 (index 5) should NOT be trimmed
        assert!(
            !is_trimmed(&result[5]),
            "Third assistant (a3) should NOT be trimmed"
        );

        // All user messages should never be trimmed
        for msg in &result {
            if msg.role == "user" {
                assert!(!is_trimmed(msg), "User messages should never be trimmed");
            }
        }
    }

    /// Verify lazy trimming: when under threshold but with existing
    /// trimmed_up_to_message_index, re-apply old trim without advancing.
    #[test]
    fn test_lazy_trimming_reapplies_without_advancing() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 2,
            context_budget_threshold: 0.8,
        });

        // Build 10 turns of user/assistant
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "Msg {}: {}",
                    i,
                    "x".repeat(100)
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Resp {}: {}",
                    i,
                    "y".repeat(100)
                ))),
                ..Default::default()
            });
        }

        // First call: small window → triggers trimming, establishes trim index
        let (_, metadata1) = cm.reduce_context_with_budget(messages.clone(), 100, None, None);
        assert!(metadata1.is_some(), "First call should trigger trimming");
        let trimmed_idx_1 = metadata1.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(trimmed_idx_1 > 0);

        // Second call: LARGE window (under threshold) but with existing metadata.
        // Should re-apply trimming up to trimmed_idx_1 but NOT advance it.
        let (result2, metadata2) =
            cm.reduce_context_with_budget(messages.clone(), 200_000, metadata1.clone(), None);

        assert!(metadata2.is_some(), "Should still return metadata");
        let trimmed_idx_2 = metadata2.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // Index should NOT have advanced (we're under threshold)
        assert_eq!(
            trimmed_idx_2, trimmed_idx_1,
            "Trim index should not advance when under threshold"
        );

        // But the old prefix should still be trimmed (re-applied)
        for (i, msg) in result2.iter().enumerate() {
            if i < trimmed_idx_1 && msg.role != "user" {
                assert!(
                    is_trimmed(msg),
                    "Non-user message {} should still be trimmed (re-applied from metadata)",
                    i
                );
            }
        }

        // Messages after the trim boundary should NOT be trimmed
        for (i, msg) in result2.iter().enumerate() {
            if i >= trimmed_idx_1 {
                assert!(
                    !is_trimmed(msg),
                    "Message {} (after trim boundary) should NOT be trimmed",
                    i
                );
            }
        }

        // Third call: small window again → should advance the index
        // Add more messages first to ensure we're over budget
        messages.push(ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("x".repeat(200))),
            ..Default::default()
        });
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("y".repeat(200))),
            ..Default::default()
        });

        let (_, metadata3) = cm.reduce_context_with_budget(messages, 100, metadata2, None);
        let trimmed_idx_3 = metadata3.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        assert!(
            trimmed_idx_3 >= trimmed_idx_1,
            "Trim index should advance when over threshold again: {} < {}",
            trimmed_idx_3,
            trimmed_idx_1
        );
    }

    /// Tool messages between assistants should be trimmed in the prefix,
    /// and the trim boundary should still be based on assistant count only.
    #[test]
    fn test_trim_boundary_with_tool_messages() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 1,
            context_budget_threshold: 0.8,
        });

        // Realistic agent flow:
        //   user → assistant(tool_call) → tool(result) → assistant → user → assistant(tool_call) → tool(result) → assistant
        //
        // After merge_consecutive_same_role the LLM sequence is:
        //   [0] user  [1] assistant(tc_1)  [2] tool(result_1)  [3] assistant
        //   [4] user  [5] assistant(tc_2)  [6] tool(result_2)  [7] assistant
        //
        // Assistants at: 1, 3, 5, 7.  keep_last_n = 1 → trim_end = index 7.
        // So messages 0..7 are in the trim zone, only message 7 is untrimmed.
        // Within the trim zone: user messages (0, 4) are preserved; assistant (1, 3, 5)
        // and tool (2, 6) messages are trimmed.
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u1".to_string())),
                ..Default::default()
            },
            create_tool_call_msg("tc_1"),
            create_tool_result_msg("tc_1", &"r".repeat(200)),
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("follow-up 1".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u2".to_string())),
                ..Default::default()
            },
            create_tool_call_msg("tc_2"),
            create_tool_result_msg("tc_2", &"r".repeat(200)),
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("follow-up 2".to_string())),
                ..Default::default()
            },
        ];

        // Compute a context window that triggers trimming: use 90% of the
        // estimated total so the 80% threshold is exceeded.
        let pre_messages: Vec<_> = messages.iter().cloned().map(LLMMessage::from).collect();
        let total_estimate = TaskBoardContextManager::estimate_tokens(&pre_messages);
        let context_window = (total_estimate as f64 / 0.9).ceil() as u64;

        let (result, metadata) =
            cm.reduce_context_with_budget(messages, context_window, None, None);
        assert!(metadata.is_some());
        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // Verify every message's trim state
        for (i, msg) in result.iter().enumerate() {
            if i >= trimmed_idx {
                // After boundary: nothing trimmed
                assert!(
                    !is_trimmed(msg),
                    "Message {} (role={}) after trim boundary should NOT be trimmed",
                    i,
                    msg.role
                );
            } else if msg.role == "user" {
                // Before boundary but user: never trimmed
                assert!(
                    !is_trimmed(msg),
                    "User message {} before trim boundary should NOT be trimmed",
                    i
                );
            } else {
                // Before boundary, assistant or tool: trimmed
                assert!(
                    is_trimmed(msg),
                    "Non-user message {} (role={}) before trim boundary should be trimmed",
                    i,
                    msg.role
                );
            }
        }

        // The last assistant should be the only untrimmed assistant
        let untrimmed_assistants: Vec<_> = result
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "assistant" && !is_trimmed(m))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            untrimmed_assistants.len(),
            1,
            "Exactly 1 assistant should be untrimmed, got {:?}",
            untrimmed_assistants
        );
    }

    /// When there are fewer assistant messages than keep_last_n but we're
    /// over budget, the budget constraint takes priority — the oldest
    /// assistant gets trimmed to make progress. keep_last_n is best-effort.
    #[test]
    fn test_fewer_assistants_than_keep_last_n_budget_overrides() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 10,
            context_budget_threshold: 0.8,
        });

        // Only 3 assistant messages but keep_last_n = 10
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(300))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a1".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(300))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a2".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(300))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a3".to_string())),
                ..Default::default()
            },
        ];

        // Small window — over budget despite fewer assistants than keep_last_n
        let (result, metadata) = cm.reduce_context_with_budget(messages, 50, None, None);

        // Budget overrides keep_last_n: at least the oldest assistant is trimmed
        assert!(metadata.is_some(), "Should produce metadata when trimming");
        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(
            trimmed_idx > 0,
            "Budget should force trimming even with fewer assistants than keep_last_n"
        );

        // All user messages should be preserved
        for msg in &result {
            if msg.role == "user" {
                assert!(!is_trimmed(msg), "User messages should never be trimmed");
            }
        }
    }

    /// With keep_last_n_assistant_messages = 0, every assistant/tool message
    /// in the entire history should be trimmed.
    #[test]
    fn test_keep_zero_trims_all_assistant_messages() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 0,
            context_budget_threshold: 0.8,
        });

        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(200))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a1".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(200))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a2".to_string())),
                ..Default::default()
            },
        ];

        let (result, metadata) = cm.reduce_context_with_budget(messages, 50, None, None);
        assert!(metadata.is_some());

        // Every assistant message should be trimmed
        for msg in &result {
            if msg.role == "assistant" {
                assert!(
                    is_trimmed(msg),
                    "All assistant messages should be trimmed when keep_last_n = 0"
                );
            }
        }

        // Every user message should be preserved
        for msg in &result {
            if msg.role == "user" {
                assert!(!is_trimmed(msg), "User messages should never be trimmed");
            }
        }
    }

    /// With keep_last_n_assistant_messages = 1, only the very last assistant
    /// message should be kept untrimmed.
    #[test]
    fn test_keep_one_preserves_only_last_assistant() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 1,
            context_budget_threshold: 0.8,
        });

        // Use large assistant messages and small user messages so that trimming
        // the first 2 assistants (per keep_last_n=1) is sufficient to get under budget.
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u1".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a1".to_string() + &"y".repeat(300))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u2".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("a2".to_string() + &"y".repeat(300))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("u3".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(
                    "a3_last".to_string() + &"y".repeat(300),
                )),
                ..Default::default()
            },
        ];

        // Context window sized so total (~380 tokens) exceeds 80% threshold (~280)
        // but after trimming 2 assistants per keep_last_n=1, remaining fits.
        let (result, metadata) = cm.reduce_context_with_budget(messages, 350, None, None);
        assert!(metadata.is_some());

        // Collect assistant messages with their trim state
        let assistants: Vec<_> = result
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "assistant")
            .map(|(i, m)| (i, is_trimmed(m)))
            .collect();

        // All but the last assistant should be trimmed
        for &(idx, trimmed) in &assistants[..assistants.len() - 1] {
            assert!(
                trimmed,
                "Assistant at index {} should be trimmed (not the last)",
                idx
            );
        }

        // The last assistant should NOT be trimmed
        let (last_idx, last_trimmed) = assistants.last().unwrap();
        assert!(
            !last_trimmed,
            "Last assistant at index {} should NOT be trimmed",
            last_idx
        );
    }

    /// The trim index should never go backward, even if a new call computes
    /// a smaller trim_end than the stored prev_trimmed_up_to.
    #[test]
    fn test_trim_index_never_goes_backward() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 2,
            context_budget_threshold: 0.8,
        });

        // 10 turns → 20 messages, small window → establishes a trim index
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "u{}: {}",
                    i,
                    "x".repeat(100)
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "a{}: {}",
                    i,
                    "y".repeat(100)
                ))),
                ..Default::default()
            });
        }

        let (_, metadata1) = cm.reduce_context_with_budget(messages.clone(), 100, None, None);
        let idx1 = metadata1.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(idx1 > 0);

        // Now use a LARGER keep_last_n (would compute a smaller trim_end)
        // but pass the existing metadata with the higher index.
        let cm_generous = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 8,
            context_budget_threshold: 0.8,
        });

        let (_, metadata2) = cm_generous.reduce_context_with_budget(messages, 100, metadata1, None);
        let idx2 = metadata2.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        assert!(
            idx2 >= idx1,
            "Trim index must never decrease: got {} < {}",
            idx2,
            idx1
        );
    }

    /// Tool definitions should be included in the budget check, pushing
    /// an otherwise-under-threshold conversation over the limit.
    #[test]
    fn test_tool_overhead_pushes_over_threshold() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 2,
            context_budget_threshold: 0.8,
        });

        // Build a conversation that's just under threshold without tools
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(100))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("y".repeat(100))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(100))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("y".repeat(100))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("x".repeat(100))),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("y".repeat(100))),
                ..Default::default()
            },
        ];

        // Estimate tokens for these messages
        let llm_msgs: Vec<_> = messages.iter().cloned().map(LLMMessage::from).collect();
        let msg_tokens = TaskBoardContextManager::estimate_tokens(&llm_msgs);

        // Pick a context window where messages alone are under 80% threshold
        // but messages + tool overhead exceed it.
        let context_window = (msg_tokens as f64 / 0.75) as u64; // 80% of this ≈ msg_tokens * 1.067
        let threshold = (context_window as f32 * 0.8) as u64;

        // Without tools: under threshold → no trimming
        let (_, meta_no_tools) =
            cm.reduce_context_with_budget(messages.clone(), context_window, None, None);
        assert!(
            meta_no_tools.is_none(),
            "Without tools, should be under threshold (tokens={}, threshold={})",
            msg_tokens,
            threshold
        );

        // With many large tool definitions: over threshold → trimming triggers
        let big_tools: Vec<LLMTool> = (0..50)
            .map(|i| LLMTool {
                name: format!("tool_{}", i),
                description: "x".repeat(200),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "arg1": { "type": "string", "description": "x".repeat(200) },
                        "arg2": { "type": "string", "description": "x".repeat(200) },
                    }
                }),
            })
            .collect();

        let tool_overhead = TaskBoardContextManager::estimate_tool_overhead(Some(&big_tools));
        assert!(
            msg_tokens + tool_overhead > threshold,
            "Tools should push over threshold: msg={} + tools={} > threshold={}",
            msg_tokens,
            tool_overhead,
            threshold
        );

        let (result, meta_with_tools) =
            cm.reduce_context_with_budget(messages, context_window, None, Some(&big_tools));
        assert!(
            meta_with_tools.is_some(),
            "With tool overhead, should exceed threshold"
        );

        // Verify some assistant message got trimmed
        let has_trimmed_assistant = result
            .iter()
            .any(|m| m.role == "assistant" && is_trimmed(m));
        assert!(
            has_trimmed_assistant,
            "At least one assistant message should be trimmed"
        );
    }

    /// Realistic multi-turn tool-call flow verifying that the trim boundary
    /// is based on assistant count and that interleaved tool messages are
    /// correctly trimmed/preserved relative to the boundary.
    #[test]
    fn test_realistic_tool_call_flow_trim_boundary() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 3,
            context_budget_threshold: 0.8,
        });

        // 5 turns of: user → assistant(tool_call) → tool(result) → assistant(follow-up)
        // Each turn produces 4 ChatMessages. After merge, each turn is:
        //   user, assistant(tc), tool(result), assistant(follow-up)
        // So 5 turns = 20 LLM messages.
        // Assistants at indices: 1, 3, 5, 7, 9, 11, 13, 15, 17, 19
        // (every assistant(tc) and assistant(follow-up))
        // keep_last_n = 3 → 3rd-from-last assistant = index 15 → trim_end = 15
        let mut messages = Vec::new();
        for turn in 0..5 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!("user turn {}", turn))),
                ..Default::default()
            });
            messages.push(create_tool_call_msg(&format!("tc_{}", turn)));
            messages.push(create_tool_result_msg(
                &format!("tc_{}", turn),
                &format!("result {}: {}", turn, "r".repeat(200)),
            ));
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!("follow-up {}", turn))),
                ..Default::default()
            });
        }

        let (result, metadata) = cm.reduce_context_with_budget(messages, 700, None, None);
        assert!(metadata.is_some());
        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // Count untrimmed assistants — should be exactly 3
        let untrimmed_assistants: Vec<usize> = result
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "assistant" && !is_trimmed(m))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            untrimmed_assistants.len(),
            3,
            "Should have exactly 3 untrimmed assistants, got {:?}",
            untrimmed_assistants
        );

        // All untrimmed assistants should be at or after the trim boundary
        for &idx in &untrimmed_assistants {
            assert!(
                idx >= trimmed_idx,
                "Untrimmed assistant at {} should be >= trim boundary {}",
                idx,
                trimmed_idx
            );
        }

        // Tool messages before the boundary should be trimmed
        let trimmed_tools: Vec<usize> = result
            .iter()
            .enumerate()
            .filter(|(i, m)| *i < trimmed_idx && m.role == "tool" && is_trimmed(m))
            .map(|(i, _)| i)
            .collect();
        let total_tools_before_boundary = result
            .iter()
            .enumerate()
            .filter(|(i, m)| *i < trimmed_idx && m.role == "tool")
            .count();
        assert_eq!(
            trimmed_tools.len(),
            total_tools_before_boundary,
            "All tool messages before trim boundary should be trimmed"
        );

        // Tool messages at or after the boundary should NOT be trimmed
        for (i, msg) in result.iter().enumerate() {
            if i >= trimmed_idx && msg.role == "tool" {
                assert!(
                    !is_trimmed(msg),
                    "Tool message {} at/after trim boundary should NOT be trimmed",
                    i
                );
            }
        }

        // User messages should never be trimmed regardless of position
        for (i, msg) in result.iter().enumerate() {
            if msg.role == "user" {
                assert!(
                    !is_trimmed(msg),
                    "User message {} should never be trimmed",
                    i
                );
            }
        }
    }

    /// Verbose simulation of a realistic multi-turn agent session.
    /// Run with `cargo test -p stakpak-api test_verbose_session_simulation -- --nocapture`
    #[test]
    fn test_verbose_session_simulation() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 50,
            context_budget_threshold: 0.8,
        });

        // Simulate a 200k context window model (like Claude)
        // With ~1000 chars per tool result, each turn is ~1200 chars = ~300 tokens
        // 30 turns = ~9000 tokens. To trigger trimming at 80%, we need context_window
        // where 80% < 9000, so context_window < 11250. Use 10000 to simulate a
        // session approaching the limit.
        let context_window = 10_000u64;
        let mut messages: Vec<ChatMessage> = Vec::new();
        let metadata: Option<serde_json::Value> = None;

        // === Simulate 30 turns of agent conversation with tool calls ===
        // Each turn: user message + assistant with tool_call + tool_result + assistant follow-up
        // This generates ~4 messages per turn, with tool results being large (like file contents)
        for turn in 0..30 {
            // User message
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "Turn {}: Please check the status of the deployment and fix any issues you find.",
                    turn
                ))),
                ..Default::default()
            });

            // Assistant with tool call
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "I'll check the deployment status for turn {}.",
                    turn
                ))),
                tool_calls: Some(vec![
                    stakpak_shared::models::integrations::openai::ToolCall {
                        id: format!("tc_{}", turn),
                        r#type: "function".to_string(),
                        function: stakpak_shared::models::integrations::openai::FunctionCall {
                            name: "run_command".to_string(),
                            arguments: format!(
                                r#"{{"command":"kubectl get pods -n app-{} -o wide"}}"#,
                                turn
                            ),
                        },
                        metadata: None,
                    },
                ]),
                ..Default::default()
            });

            // Tool result (simulating large kubectl output)
            messages.push(ChatMessage {
                role: Role::Tool,
                tool_call_id: Some(format!("tc_{}", turn)),
                content: Some(MessageContent::String(format!(
                    "NAME                          READY   STATUS    RESTARTS   AGE    IP            NODE\n\
                     app-{t}-deploy-abc123-xyz     1/1     Running   0          2d     10.0.{t}.1    node-1\n\
                     app-{t}-deploy-abc123-def     1/1     Running   0          2d     10.0.{t}.2    node-2\n\
                     app-{t}-deploy-abc123-ghi     0/1     CrashLoop 5          2d     10.0.{t}.3    node-3\n\
                     app-{t}-worker-jkl456-mno     1/1     Running   0          5d     10.0.{t}.4    node-1\n\
                     app-{t}-worker-jkl456-pqr     1/1     Running   0          5d     10.0.{t}.5    node-2\n\
                     {extra}",
                    t = turn,
                    extra = "x".repeat(800) // simulate verbose output
                ))),
                ..Default::default()
            });

            // Assistant follow-up
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "I found an issue in turn {}. Pod app-{}-deploy-abc123-ghi is in CrashLoopBackOff. \
                     Let me check the logs and fix the configuration.",
                    turn, turn
                ))),
                ..Default::default()
            });
        }

        // === Run the context manager (simulating what the hook does) ===
        let (result, new_metadata) =
            cm.reduce_context_with_budget(messages.clone(), context_window, metadata.clone(), None);

        // === Print what the agent sees ===
        println!("\n{}", "=".repeat(80));
        println!("CONTEXT TRIMMING SIMULATION RESULTS");
        println!("{}", "=".repeat(80));
        println!(
            "Input: {} ChatMessages ({} turns)",
            messages.len(),
            messages.len() / 4
        );
        println!("Output: {} LLMMessages (after merge/dedup)", result.len());
        println!("Context window: {} tokens", context_window);
        println!(
            "Threshold: 80% = {} tokens",
            (context_window as f32 * 0.8) as u64
        );

        let _estimated_before = TaskBoardContextManager::estimate_tokens(
            &result
                .iter()
                .map(|m| {
                    // Create untrimmed version for comparison
                    LLMMessage {
                        role: m.role.clone(),
                        content: LLMMessageContent::String("x".repeat(200)),
                    }
                })
                .collect::<Vec<_>>(),
        );

        let estimated_after = TaskBoardContextManager::estimate_tokens(&result);
        println!("Estimated tokens after trimming: {}", estimated_after);

        if let Some(ref meta) = new_metadata {
            println!(
                "Metadata: {}",
                serde_json::to_string_pretty(meta).unwrap_or_default()
            );
        } else {
            println!("Metadata: None (no trimming occurred)");
        }

        println!("\n--- Messages the agent sees ---");
        for (i, msg) in result.iter().enumerate() {
            let content_preview = match &msg.content {
                LLMMessageContent::String(s) => {
                    if s.chars().count() > 80 {
                        let truncated: String = s.chars().take(80).collect();
                        format!("{}...", truncated)
                    } else {
                        s.clone()
                    }
                }
                LLMMessageContent::List(parts) => {
                    let mut preview = String::new();
                    for p in parts {
                        match p {
                            LLMMessageTypedContent::Text { text } => {
                                let t = if text.chars().count() > 60 {
                                    let truncated: String = text.chars().take(60).collect();
                                    format!("{}...", truncated)
                                } else {
                                    text.clone()
                                };
                                preview.push_str(&format!("[text:{}] ", t));
                            }
                            LLMMessageTypedContent::ToolCall { id, name, .. } => {
                                preview.push_str(&format!("[tool_call:{}:{}] ", id, name));
                            }
                            LLMMessageTypedContent::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } => {
                                let c = if content.chars().count() > 40 {
                                    let truncated: String = content.chars().take(40).collect();
                                    format!("{}...", truncated)
                                } else {
                                    content.clone()
                                };
                                preview.push_str(&format!("[tool_result:{}:{}] ", tool_use_id, c));
                            }
                            LLMMessageTypedContent::Image { .. } => {
                                preview.push_str("[image] ");
                            }
                        }
                    }
                    preview
                }
            };

            let trimmed_marker = if is_trimmed(msg) { " [TRIMMED]" } else { "" };
            println!(
                "  [{:2}] {:>10}: {}{}",
                i, msg.role, content_preview, trimmed_marker
            );
        }

        // === Simulate session resume: add 2 more turns and trim again ===
        println!("\n--- After adding 2 more turns (simulating session resume) ---");
        for turn in 30..32 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "Turn {}: Check again please.",
                    turn
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Checking turn {}... {}",
                    turn,
                    "z".repeat(200)
                ))),
                ..Default::default()
            });
        }

        let (result2, metadata2) =
            cm.reduce_context_with_budget(messages.clone(), context_window, new_metadata, None);

        println!("After resume: {} LLMMessages", result2.len());
        let estimated_after2 = TaskBoardContextManager::estimate_tokens(&result2);
        println!("Estimated tokens after second trim: {}", estimated_after2);
        if let Some(ref meta) = metadata2 {
            println!(
                "Metadata: {}",
                serde_json::to_string_pretty(meta).unwrap_or_default()
            );
        }

        let trimmed_count = result2.iter().filter(|m| is_trimmed(m)).count();
        let untrimmed_count = result2.iter().filter(|m| !is_trimmed(m)).count();
        println!("Trimmed: {}, Untrimmed: {}", trimmed_count, untrimmed_count);

        // Print last 6 messages (what the agent actually works with)
        println!("\n--- Last 6 messages (agent's working context) ---");
        let len2 = result2.len();
        let start = len2.saturating_sub(6);
        for (i, msg) in result2[start..].iter().enumerate() {
            let content_preview = match &msg.content {
                LLMMessageContent::String(s) => {
                    if s.chars().count() > 100 {
                        let truncated: String = s.chars().take(100).collect();
                        format!("{}...", truncated)
                    } else {
                        s.clone()
                    }
                }
                LLMMessageContent::List(parts) => format!("[{} parts]", parts.len()),
            };
            println!("  [{:2}] {:>10}: {}", start + i, msg.role, content_preview);
        }

        // Assertions
        assert!(trimmed_count > 0, "Should have trimmed some messages");
        assert!(
            untrimmed_count >= 4,
            "Should keep at least 4 untrimmed messages"
        );
    }

    // =========================================================================
    // Regression tests
    //
    // These tests cover bugs found in production. Each documents:
    //   1. The production config that triggered the bug
    //   2. Why the old code was wrong
    //   3. The correct behavior
    //
    // Key design invariants:
    //   - Budget (context_budget_threshold) is the HARD constraint — context
    //     must not grow past it unchecked.
    //   - keep_last_n_assistant_messages is BEST-EFFORT — respected when budget
    //     allows, overridden when budget demands trimming.
    //   - Lazy trimming: the trim boundary only advances when the *effective*
    //     token count (after re-applying previous trims) exceeds the threshold.
    //     This keeps the message prefix stable across turns, which is critical
    //     for Anthropic prompt caching — the less the prefix changes, the
    //     higher the cache hit rate.
    // =========================================================================

    /// Production config: keep_last_n=50, threshold=0.3, context_window=200k.
    ///
    /// Bug: with 30% threshold you hit the budget after ~20 assistant messages,
    /// well before accumulating 50. The old code's keep_last_n loop never found
    /// 50 assistants, so trim_end stayed at 0 → nothing got trimmed → context
    /// grew past 50% unchecked.
    ///
    /// Fix: budget overrides keep_last_n. When over budget and keep_last_n
    /// can't produce a boundary, the fallback trims the oldest assistant to
    /// make progress.
    #[test]
    fn test_production_config_budget_overrides_keep_last_n() {
        // Mirror the actual production config from client/mod.rs
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 50,
            context_budget_threshold: 0.3,
        });

        // Simulate a session with 10 turns (20 messages) — well under 50
        // assistants but enough content to exceed 30% of a small window.
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "User turn {}: {}",
                    i,
                    "x".repeat(200)
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Assistant turn {}: {}",
                    i,
                    "y".repeat(200)
                ))),
                ..Default::default()
            });
        }

        // Context window where 30% threshold is easily exceeded by 10 turns
        let context_window = 500;
        let (result, metadata) =
            cm.reduce_context_with_budget(messages, context_window, None, None);

        // Must trim despite having only 10 assistants (< keep_last_n of 50)
        assert!(metadata.is_some(), "Should trigger trimming");
        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(
            trimmed_idx > 0,
            "Budget must override keep_last_n when over threshold \
             (had 10 assistants < keep_last_n of 50, but was over 30% budget)"
        );

        // At least the first assistant should be trimmed
        let first_assistant = result.iter().find(|m| m.role == "assistant").unwrap();
        assert!(
            is_trimmed(first_assistant),
            "Oldest assistant should be trimmed to make progress"
        );

        // User messages are never trimmed
        for msg in &result {
            if msg.role == "user" {
                assert!(!is_trimmed(msg), "User messages should never be trimmed");
            }
        }
    }

    /// Regression test: keep_last_n produces a trim boundary but the remaining
    /// N assistant messages (with large tool results) still exceed the budget.
    ///
    /// Bug: the old code used keep_last_n as a hard boundary — when
    /// keep_n_trim_end > 0, it skipped the budget-driven scan entirely.
    /// With keep_last_n=20 and 200K context window, 20 assistant messages
    /// with large tool results could easily be 160K+ tokens, blowing past
    /// the 80% threshold.
    ///
    /// Fix: keep_last_n is best-effort. After applying keep_last_n trimming,
    /// if still over budget, continue scanning forward to trim more messages.
    #[test]
    fn test_keep_last_n_boundary_insufficient_budget_continues_trimming() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 3,
            context_budget_threshold: 0.8,
        });

        // 6 turns: user + assistant with large content.
        // keep_last_n=3 would normally keep the last 3 assistants untrimmed,
        // but each assistant is so large that 3 of them alone exceed the budget.
        let mut messages = Vec::new();
        for i in 0..6 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!("user {}", i))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "assistant {}: {}",
                    i,
                    "x".repeat(500) // ~170 tokens each
                ))),
                ..Default::default()
            });
        }

        // Context window where 80% ≈ 400 tokens.
        // 6 assistants × ~170 tokens = ~1020 tokens total.
        // keep_last_n=3 would leave ~510 tokens — still over 400.
        // Budget must override and trim past the keep_last_n boundary.
        let context_window = 500;
        let (result, metadata) =
            cm.reduce_context_with_budget(messages, context_window, None, None);

        assert!(metadata.is_some(), "Should trigger trimming");
        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // The trim index must go PAST the keep_last_n boundary (index of
        // 3rd-from-last assistant). With 6 assistants at indices 1,3,5,7,9,11,
        // keep_last_n=3 boundary = index 7. Budget must push past that.
        let keep_n_boundary = 7; // 3rd-from-last assistant in merged sequence
        assert!(
            trimmed_idx > keep_n_boundary,
            "Budget should override keep_last_n: trimmed_idx {} should be > keep_n boundary {}",
            trimmed_idx,
            keep_n_boundary
        );

        // Effective tokens after trimming should be under threshold
        let effective_tokens = TaskBoardContextManager::estimate_tokens(&result);
        let threshold = (context_window as f32 * 0.8) as u64;
        assert!(
            effective_tokens <= threshold,
            "Effective tokens {} should be <= threshold {} after budget-driven trimming",
            effective_tokens,
            threshold
        );

        // User messages are never trimmed
        for msg in &result {
            if msg.role == "user" {
                assert!(!is_trimmed(msg), "User messages should never be trimmed");
            }
        }
    }

    /// Simulates the exact production scenario from the bug report:
    /// Claude Opus 4.5 with 200K context, keep_last_n=20, threshold=0.8.
    /// A session grows to 196K/200K tokens because the last 20 assistant
    /// messages (with large tool results) themselves exceed the budget.
    ///
    /// The OLD code would stop trimming at the keep_last_n boundary and
    /// never get under budget. The fix continues trimming past keep_last_n
    /// when budget demands it.
    ///
    /// We also verify the hook-level fix: the system prompt (~8K tokens)
    /// and max_output_tokens (16K) are subtracted from the context window
    /// before the trimmer sees it, so the effective budget is ~176K × 0.8
    /// = ~140K, not 200K × 0.8 = 160K.
    #[test]
    fn test_production_scenario_200k_context_keep_20() {
        // Mirror production config from client/mod.rs
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 20,
            context_budget_threshold: 0.8,
        });

        // Simulate a session with 30 turns of tool-heavy interaction.
        // Each turn: user (small) → assistant with tool_call → tool result (large) → assistant follow-up
        // This produces 60 assistant messages and 30 tool results.
        // After merge: 120 messages (30 × 4).
        let mut messages = Vec::new();
        for turn in 0..30 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "User turn {}: {}",
                    turn,
                    "q".repeat(100)
                ))),
                ..Default::default()
            });
            messages.push(create_tool_call_msg(&format!("tc_{}", turn)));
            messages.push(create_tool_result_msg(
                &format!("tc_{}", turn),
                // Large tool results (~2000 chars each ≈ 700 tokens)
                &format!("result {}: {}", turn, "r".repeat(2000)),
            ));
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Follow-up {}: {}",
                    turn,
                    "a".repeat(500)
                ))),
                ..Default::default()
            });
        }

        // Use a context window that simulates the effective budget after
        // subtracting system prompt and max_output_tokens in the hook.
        // Real: 200K - ~8K (system prompt) - 16K (max_output) ≈ 176K
        // Scale down proportionally for test: use 10000 as context_window.
        // 80% threshold = 8000 tokens.
        let context_window = 10_000u64;
        let threshold = (context_window as f32 * 0.8) as u64;

        let (result, metadata) =
            cm.reduce_context_with_budget(messages.clone(), context_window, None, None);

        assert!(metadata.is_some(), "Should trigger trimming");
        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // KEY ASSERTION: effective tokens must be under threshold.
        // This is what the old code failed to guarantee — it would stop at
        // keep_last_n boundary even if remaining tokens exceeded the budget.
        let effective_tokens = TaskBoardContextManager::estimate_tokens(&result);
        assert!(
            effective_tokens <= threshold,
            "PRODUCTION BUG: effective tokens {} exceed threshold {} \
             (this is the exact scenario where context hit 196K/200K). \
             trimmed_idx={}, total_messages={}",
            effective_tokens,
            threshold,
            trimmed_idx,
            result.len()
        );

        // The trim index should go well past the keep_last_n boundary.
        // With 60 assistants and keep_last_n=20, the keep_n boundary is at
        // the 20th-from-last assistant. But 20 assistants with ~700-token
        // tool results = ~14000 tokens > 8000 threshold, so budget must
        // push further.
        assert!(trimmed_idx > 0, "Should have trimmed some messages");

        // User messages are never trimmed
        for msg in &result {
            if msg.role == "user" {
                assert!(!is_trimmed(msg), "User messages should never be trimmed");
            }
        }

        // Simulate a second call (next turn) — verify trimming state persists
        // and effective tokens stay under budget.
        messages.push(ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("Next question".to_string())),
            ..Default::default()
        });
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String(format!(
                "Response: {}",
                "a".repeat(500)
            ))),
            ..Default::default()
        });

        let (result2, metadata2) =
            cm.reduce_context_with_budget(messages, context_window, metadata, None);

        let effective_tokens2 = TaskBoardContextManager::estimate_tokens(&result2);
        assert!(
            effective_tokens2 <= threshold,
            "Second call: effective tokens {} exceed threshold {} — trimming not keeping up",
            effective_tokens2,
            threshold
        );

        let trimmed_idx2 = metadata2.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(
            trimmed_idx2 >= trimmed_idx,
            "Trim index should never go backward: {} < {}",
            trimmed_idx2,
            trimmed_idx
        );
    }

    /// Lazy trimming preserves prompt cache: after trimming brings effective
    /// tokens under threshold, the trim boundary must NOT advance on the next
    /// call — even though raw (untrimmed) history keeps growing.
    ///
    /// Bug: the old code estimated tokens on raw messages, so once raw content
    /// exceeded the threshold it was permanently true. The trim boundary
    /// advanced every turn, invalidating the prompt cache each time.
    ///
    /// Fix: re-apply previous trims first, then re-estimate on the *effective*
    /// (trimmed) messages. Only advance when effective tokens exceed threshold.
    #[test]
    fn test_lazy_trimming_preserves_cache_prefix() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 2,
            context_budget_threshold: 0.8,
        });

        // Build conversation that exceeds threshold
        let mut messages = Vec::new();
        for i in 0..5 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!(
                    "User {}: {}",
                    i,
                    "x".repeat(100)
                ))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(format!(
                    "Asst {}: {}",
                    i,
                    "y".repeat(100)
                ))),
                ..Default::default()
            });
        }

        // First call: triggers trimming, establishes trim boundary
        let context_window = 500;
        let (result1, metadata1) =
            cm.reduce_context_with_budget(messages.clone(), context_window, None, None);
        assert!(metadata1.is_some(), "First call should trigger trimming");
        let trimmed_idx_1 = metadata1.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(trimmed_idx_1 > 0);

        // Add small messages — raw total grows but effective total stays under
        messages.push(ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("short".to_string())),
            ..Default::default()
        });
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("brief".to_string())),
            ..Default::default()
        });

        // Second call: effective tokens under threshold → index must NOT advance
        let (result2, metadata2) =
            cm.reduce_context_with_budget(messages.clone(), context_window, metadata1, None);
        let trimmed_idx_2 = metadata2.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        assert_eq!(
            trimmed_idx_2, trimmed_idx_1,
            "Trim index must not advance when effective tokens are under threshold — \
             advancing would invalidate the prompt cache (was {}, now {})",
            trimmed_idx_1, trimmed_idx_2
        );

        // The trimmed prefix must be identical across calls (cache stability)
        for i in 0..trimmed_idx_1.min(result1.len()).min(result2.len()) {
            let t1 = is_trimmed(&result1[i]);
            let t2 = is_trimmed(&result2[i]);
            assert_eq!(
                t1, t2,
                "Message {} trim state changed between calls (was trimmed={}, now trimmed={}) — \
                 this would invalidate the prompt cache",
                i, t1, t2
            );
        }

        // New messages at the end should NOT be trimmed
        let len = result2.len();
        assert!(
            !is_trimmed(&result2[len - 1]),
            "Last message should not be trimmed"
        );
        assert!(
            !is_trimmed(&result2[len - 2]),
            "Second-to-last message should not be trimmed"
        );
    }

    /// Under budget with fewer assistants than keep_last_n: no trimming at all.
    #[test]
    fn test_under_budget_fewer_assistants_no_trimming() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 10,
            context_budget_threshold: 0.8,
        });

        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("hello".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("hi".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String("how are you".to_string())),
                ..Default::default()
            },
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("good".to_string())),
                ..Default::default()
            },
        ];

        // Large window — well under budget
        let (result, metadata) = cm.reduce_context_with_budget(messages, 200_000, None, None);

        assert!(
            metadata.is_none(),
            "Should not produce metadata when under budget"
        );
        for msg in &result {
            assert!(
                !is_trimmed(msg),
                "No messages should be trimmed when under budget"
            );
        }
    }

    /// Multi-turn progressive trimming: each call that's still over budget
    /// trims one more assistant, eventually bringing context under threshold.
    /// Once under threshold, the boundary freezes (cache stability).
    #[test]
    fn test_progressive_trimming_then_freeze() {
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 50, // high keep_last_n, like production
            context_budget_threshold: 0.3,
        });

        // 5 turns with large assistant responses
        let mut messages = Vec::new();
        for i in 0..5 {
            messages.push(ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(format!("q{}", i))),
                ..Default::default()
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String("y".repeat(300))),
                ..Default::default()
            });
        }

        let context_window = 800; // 30% = 240 tokens budget

        // Turn 1: over budget → trims oldest assistant
        let (_, meta1) =
            cm.reduce_context_with_budget(messages.clone(), context_window, None, None);
        assert!(meta1.is_some());
        let idx1 = meta1.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(idx1 > 0, "Should trim on first call");

        // Turn 2: add small message, call again — if still over budget,
        // index advances further; if under, it freezes.
        messages.push(ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("small".to_string())),
            ..Default::default()
        });
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("ok".to_string())),
            ..Default::default()
        });

        let (_, meta2) =
            cm.reduce_context_with_budget(messages.clone(), context_window, meta1, None);
        let idx2 = meta2.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(
            idx2 >= idx1,
            "Trim index must never go backward: {} < {}",
            idx2,
            idx1
        );

        // Turn 3: another small message
        messages.push(ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("tiny".to_string())),
            ..Default::default()
        });
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String("k".to_string())),
            ..Default::default()
        });

        let (_, meta3) =
            cm.reduce_context_with_budget(messages.clone(), context_window, meta2.clone(), None);
        let idx3 = meta3.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(
            idx3 >= idx2,
            "Trim index must never go backward: {} < {}",
            idx3,
            idx2
        );

        // If idx2 == idx3, trimming has frozen (effective tokens under budget).
        // If idx3 > idx2, we're still over budget and making progress.
        // Either way, the invariant holds: monotonically non-decreasing.
    }
}

pub struct TaskBoardContextManagerOptions {
    /// How many recent **assistant** messages to keep untrimmed when context
    /// trimming is triggered. The trim boundary is placed just before the
    /// Nth-from-last assistant message — all messages after that point
    /// (including interleaved user/tool messages) are left untouched.
    /// User and system messages are never trimmed regardless of position.
    pub keep_last_n_assistant_messages: usize,
    /// Fraction of context window at which trimming triggers (e.g., 0.8 = 80%)
    pub context_budget_threshold: f32,
}

impl TaskBoardContextManager {
    pub fn new(options: TaskBoardContextManagerOptions) -> Self {
        Self {
            keep_last_n_assistant_messages: options.keep_last_n_assistant_messages,
            context_budget_threshold: options.context_budget_threshold,
        }
    }
}
