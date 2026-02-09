use std::collections::HashMap;

use super::common::HistoryProcessingOptions;
use stakpak_shared::models::{
    integrations::openai::{ChatMessage, MessageContent},
    llm::{LLMMessage, LLMMessageContent, LLMMessageTypedContent},
};

pub struct TaskBoardContextManager {
    _options: HistoryProcessingOptions,
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
    use stakpak_shared::models::integrations::openai::{
        FunctionCall, MessageContent, Role, ToolCall,
    };
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
}

pub struct TaskBoardContextManagerOptions {
    pub history_action_message_size_limit: usize,
    pub history_action_message_keep_last_n: usize,
    pub history_action_result_keep_last_n: usize,
}

impl TaskBoardContextManager {
    pub fn new(options: TaskBoardContextManagerOptions) -> Self {
        Self {
            _options: HistoryProcessingOptions {
                history_action_message_size_limit: options.history_action_message_size_limit,
                history_action_message_keep_last_n: options.history_action_message_keep_last_n,
                history_action_result_keep_last_n: options.history_action_result_keep_last_n,
                truncation_hint: "consult the task board cards instead".to_string(),
            },
        }
    }
}
