use std::collections::{HashMap, HashSet};

use super::common::HistoryProcessingOptions;
use stakpak_shared::models::{
    integrations::openai::{ChatMessage, MessageContent, Role},
    llm::{LLMMessage, LLMMessageContent, LLMMessageTypedContent},
};

pub struct TaskBoardContextManager {
    options: HistoryProcessingOptions,
}

impl super::ContextManager for TaskBoardContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        // Collect all tool call IDs in order of appearance
        let all_tool_call_ids: Vec<String> = messages
            .iter()
            .filter_map(|m| m.tool_calls.as_ref())
            .flatten()
            .map(|tc| tc.id.clone())
            .collect();

        // Tool calls whose associated assistant message "thought" content should be preserved
        let preserved_message_content_ids: HashSet<String> = all_tool_call_ids
            .iter()
            .rev()
            .take(self.options.history_action_message_keep_last_n)
            .cloned()
            .collect();

        // Tool calls whose associated tool result content should be preserved
        let preserved_result_content_ids: HashSet<String> = all_tool_call_ids
            .iter()
            .rev()
            .take(self.options.history_action_result_keep_last_n)
            .cloned()
            .collect();

        // Process each message: clean checkpoint_id tags and apply dropping logic
        let llm_messages: Vec<_> = messages
            .into_iter()
            .map(|mut message| {
                // Remove checkpoint_id XML tags from message content
                if let Some(content) = message.content {
                    message.content = Some(match content {
                        MessageContent::String(s) => MessageContent::String(
                            super::common::remove_xml_tag("checkpoint_id", &s),
                        ),
                        MessageContent::Array(parts) => MessageContent::Array(
                            parts
                                .into_iter()
                                .map(|mut part| {
                                    if let Some(text) = part.text {
                                        part.text = Some(super::common::remove_xml_tag(
                                            "checkpoint_id",
                                            &text,
                                        ));
                                    }
                                    part
                                })
                                .collect(),
                        ),
                    });
                }

                // Apply history dropping logic based on message role
                match message.role {
                    Role::Assistant => {
                        // For assistant messages with tool calls, check if we should drop the "thought" content
                        if let Some(tool_calls) = &message.tool_calls {
                            let all_tool_calls_are_old = tool_calls
                                .iter()
                                .all(|tc| !preserved_message_content_ids.contains(&tc.id));

                            // Only drop content if ALL tool calls in this message are old
                            // AND the content exceeds the size limit
                            if all_tool_calls_are_old {
                                let content_length = message
                                    .content
                                    .as_ref()
                                    .map(|c| c.to_string().len())
                                    .unwrap_or(0);

                                if content_length > self.options.history_action_message_size_limit {
                                    message.content = None;
                                }
                            }
                        }
                    }
                    Role::Tool => {
                        // For tool result messages, replace old results with truncation placeholder
                        if let Some(tool_call_id) = &message.tool_call_id {
                            let is_old_result = !preserved_result_content_ids.contains(tool_call_id);

                            if is_old_result {
                                let truncation_message = if self.options.truncation_hint.is_empty() {
                                    "[This result was truncated from history to conserve space]"
                                        .to_string()
                                } else {
                                    format!(
                                        "[This result was truncated from history to conserve space, {}]",
                                        self.options.truncation_hint
                                    )
                                };
                                message.content = Some(MessageContent::String(truncation_message));
                            }
                        }
                    }
                    _ => {}
                }

                message
            })
            .map(LLMMessage::from)
            .collect();

        // Post-process: merge consecutive same-role messages and deduplicate
        // tool_results. Providers like Anthropic require strictly alternating
        // user/assistant turns; multiple consecutive role=tool messages (each
        // converted to role=user by the provider layer) would violate this.
        let llm_messages = merge_consecutive_same_role(llm_messages);
        dedup_tool_results(llm_messages)
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
mod tests {
    use super::super::ContextManager;
    use super::*;
    use stakpak_shared::models::integrations::openai::{FunctionCall, MessageContent, ToolCall};
    use stakpak_shared::models::llm::{LLMMessageContent, LLMMessageTypedContent};

    fn create_context_manager() -> TaskBoardContextManager {
        TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            history_action_message_size_limit: 10,
            history_action_message_keep_last_n: 2, // Only keep last 2 actions
            history_action_result_keep_last_n: 2,
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
    fn test_reduce_context_drops_old_action_results() {
        let cm = create_context_manager();
        let messages = vec![
            create_tool_call_msg("call_1"),
            create_tool_result_msg("call_1", "Result 1"),
            create_tool_call_msg("call_2"),
            create_tool_result_msg("call_2", "Result 2"),
            create_tool_call_msg("call_3"),               // kept
            create_tool_result_msg("call_3", "Result 3"), // kept
        ];

        let reduced = cm.reduce_context(messages);

        // logic: total 3 actions. keep last 2. preserved: call_3, call_2.
        // call_1 is old.

        let result_1 = &reduced[1]; // tool result for call_1
        match &result_1.content {
            LLMMessageContent::List(parts) => {
                // should contain truncated text
                if let LLMMessageTypedContent::ToolResult { content, .. } = &parts[0] {
                    assert!(
                        content.contains("truncated"),
                        "Old result should be truncated"
                    );
                } else {
                    // fallback if it's text
                    if let LLMMessageTypedContent::Text { text } = &parts[0] {
                        assert!(text.contains("truncated"));
                    }
                }
            }
            LLMMessageContent::String(s) => {
                assert!(s.contains("truncated"), "Old result should be truncated");
            }
        }

        let result_3 = &reduced[5]; // tool result for call_3
        match &result_3.content {
            LLMMessageContent::List(parts) => {
                if let LLMMessageTypedContent::ToolResult { content, .. } = &parts[0] {
                    assert_eq!(content, "Result 3");
                }
            }
            LLMMessageContent::String(s) => {
                assert_eq!(s, "Result 3");
            }
        }
    }

    #[test]
    fn test_reduce_context_drops_old_action_message_content_if_large() {
        let cm = create_context_manager();
        // Message size limit is 10.
        let long_thought = "This is a very long thought that should be dropped";

        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(long_thought.to_string())),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(), // old
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
            create_tool_call_msg("call_2"), // preserved
            create_tool_call_msg("call_3"), // preserved
        ];

        let reduced = cm.reduce_context(messages);

        let msg_0 = &reduced[0];
        match &msg_0.content {
            LLMMessageContent::String(_) => {
                // Content was cleared. If LLMMessage content is String, it must be empty or just tool call text representation (depending on conversion)
                // But LLMMessage usually uses List for tool calls.
                // If From<ChatMessage> sees empty content string but tool calls, it creates List with just ToolCalls.
                panic!("Expected List content with tool call but no text");
            }
            LLMMessageContent::List(parts) => {
                // Should have ToolCall part.
                // Should NOT have Text part (or empty text).
                let has_text = parts
                    .iter()
                    .any(|p| matches!(p, LLMMessageTypedContent::Text { .. }));
                assert!(!has_text, "Large text content should be dropped");

                let has_tool = parts
                    .iter()
                    .any(|p| matches!(p, LLMMessageTypedContent::ToolCall { .. }));
                assert!(has_tool, "Tool call should be preserved");
            }
        }
    }

    #[test]
    fn test_reduce_context_mixed_limits() {
        // Keep last 1 result, but last 2 messages (thoughts).
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            history_action_message_size_limit: 10,
            history_action_message_keep_last_n: 2,
            history_action_result_keep_last_n: 1,
        });

        let long_thought = "This is a very long thought that should be dropped if not preserved"; // > 10 chars

        let messages = vec![
            // Action 1: Oldest. Both result and message should be dropped (if message > limit)
            // But wait, we keep last 2 messages. So if we have 3 actions:
            // Act 3 (new), Act 2 (msg kept, result dropped), Act 1 (msg dropped, result dropped).

            // Let's create 3 actions.
            // 1.
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(long_thought.to_string())),
                tool_calls: Some(vec![ToolCall {
                    id: "c1".to_string(),
                    r#type: "f".to_string(),
                    function: FunctionCall {
                        name: "t".to_string(),
                        arguments: "{}".to_string(),
                    },
                    metadata: None,
                }]),
                ..Default::default()
            },
            create_tool_result_msg("c1", "r1"),
            // 2. Message preserved (last 2), Result dropped (last 1)
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(long_thought.to_string())),
                tool_calls: Some(vec![ToolCall {
                    id: "c2".to_string(),
                    r#type: "f".to_string(),
                    function: FunctionCall {
                        name: "t".to_string(),
                        arguments: "{}".to_string(),
                    },
                    metadata: None,
                }]),
                ..Default::default()
            },
            create_tool_result_msg("c2", "r2"),
            // 3. Both preserved (last 1)
            create_tool_call_msg("c3"),
            create_tool_result_msg("c3", "r3"),
        ];

        let reduced = cm.reduce_context(messages);

        // c1 message (idx 0): Should be dropped (it's 3rd from last, limit 2)
        let msg_c1 = &reduced[0];
        if let LLMMessageContent::List(parts) = &msg_c1.content {
            assert!(
                !parts
                    .iter()
                    .any(|p| matches!(p, LLMMessageTypedContent::Text { .. })),
                "c1 text should be dropped"
            );
        } else {
            // If conversion resulted in no content parts other than tool calls, it might have made it into List or something else depending on impl.
            // But expected behavior is Text part is gone.
            match &msg_c1.content {
                LLMMessageContent::String(s) => assert!(s.is_empty()), // empty string is fine too
                _ => {}
            }
        }

        // c1 result (idx 1): Should be dropped (limit 1)
        let res_c1 = &reduced[1];
        if let LLMMessageContent::String(s) = &res_c1.content {
            assert!(s.contains("truncated"), "c1 result should be truncated");
        } else if let LLMMessageContent::List(parts) = &res_c1.content {
            // check inner
            if let LLMMessageTypedContent::ToolResult { content, .. } = &parts[0] {
                assert!(content.contains("truncated"));
            }
        }

        // c2 message (idx 2): Should be KEPT (limit 2, it's 2nd from last)
        let msg_c2 = &reduced[2];
        if let LLMMessageContent::List(parts) = &msg_c2.content {
            // Should contain text
            let has_text = parts
                .iter()
                .any(|p| matches!(p, LLMMessageTypedContent::Text { .. }));
            assert!(has_text, "c2 text should be preserved");
        } else if let LLMMessageContent::String(s) = &msg_c2.content {
            assert!(!s.is_empty(), "c2 text should be preserved");
        }

        // c2 result (idx 3): Should be DROPPED (limit 1)
        let res_c2 = &reduced[3];
        match &res_c2.content {
            LLMMessageContent::String(s) => {
                assert!(s.contains("truncated"))
            }
            LLMMessageContent::List(parts) => {
                if let LLMMessageTypedContent::ToolResult { content, .. } = &parts[0] {
                    assert!(content.contains("truncated"));
                }
            }
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
    fn test_reduce_context_preserves_small_old_action_messages() {
        let cm = create_context_manager();
        // Size limit is 10. "short" is 5 chars, which is below the limit.
        let short_thought = "short";

        let messages = vec![
            ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::String(short_thought.to_string())),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(), // old (3rd from last of 3 total, keep_last_n=2)
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
            create_tool_call_msg("call_2"), // preserved
            create_tool_call_msg("call_3"), // preserved
        ];

        let reduced = cm.reduce_context(messages);

        // Even though call_1 is "old", its content is below the size limit, so it should be preserved
        let msg_0 = &reduced[0];
        match &msg_0.content {
            LLMMessageContent::String(s) => {
                assert_eq!(s, short_thought, "Small content should be preserved");
            }
            LLMMessageContent::List(parts) => {
                let has_text = parts
                    .iter()
                    .any(|p| matches!(p, LLMMessageTypedContent::Text { .. }));
                assert!(
                    has_text,
                    "Small text content should be preserved even for old actions"
                );
            }
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
        // Tool role messages are passed through as-is; provider layer handles
        // format conversion (e.g., Anthropic expects user role with tool_result blocks)
        let cm = create_context_manager();
        let messages = vec![
            create_tool_call_msg("call_1"),
            create_tool_result_msg("call_1", "Result content"),
        ];

        let reduced = cm.reduce_context(messages);

        // The tool result should preserve the tool role - provider layer handles conversion
        let result_msg = &reduced[1];
        assert_eq!(
            result_msg.role, "tool",
            "Tool results should preserve tool role (provider layer handles format conversion)"
        );
    }
}

pub struct TaskBoardContextManagerOptions {
    pub history_action_message_size_limit: usize,
    pub history_action_message_keep_last_n: usize,
    pub history_action_result_keep_last_n: usize,
}

impl TaskBoardContextManager {
    pub fn new(options: TaskBoardContextManagerOptions) -> Self {
        Self {
            options: HistoryProcessingOptions {
                history_action_message_size_limit: options.history_action_message_size_limit,
                history_action_message_keep_last_n: options.history_action_message_keep_last_n,
                history_action_result_keep_last_n: options.history_action_result_keep_last_n,
                truncation_hint: "consult the task board cards instead".to_string(),
            },
        }
    }
}
