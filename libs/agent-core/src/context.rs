use crate::types::ContextConfig;
use stakai::{ContentPart, Message, MessageContent, Model, Role, Tool};
use std::collections::{HashMap, HashSet};

const TRUNCATED_ASSISTANT_PLACEHOLDER: &str = "[assistant message truncated]";

/// Pluggable strategy for reducing context before each inference turn.
pub trait ContextReducer: Send + Sync {
    fn reduce(
        &self,
        messages: Vec<Message>,
        model: &Model,
        max_output_tokens: u32,
        tools: &[Tool],
        metadata: &mut serde_json::Value,
    ) -> Vec<Message>;
}

#[derive(Debug, Clone)]
pub struct DefaultContextReducer {
    config: ContextConfig,
}

impl DefaultContextReducer {
    pub fn new(config: ContextConfig) -> Self {
        Self { config }
    }
}

impl Default for DefaultContextReducer {
    fn default() -> Self {
        Self::new(ContextConfig::default())
    }
}

impl ContextReducer for DefaultContextReducer {
    fn reduce(
        &self,
        messages: Vec<Message>,
        _model: &Model,
        _max_output_tokens: u32,
        _tools: &[Tool],
        _metadata: &mut serde_json::Value,
    ) -> Vec<Message> {
        reduce_context(messages, &self.config)
    }
}

pub fn reduce_context(messages: Vec<Message>, config: &ContextConfig) -> Vec<Message> {
    let messages = dedup_tool_results(messages);
    let messages = merge_consecutive_same_role(messages);
    let messages = truncate_old_tool_results(messages, config.keep_last_messages);
    let messages = truncate_old_assistant_messages(messages, config.keep_last_messages);
    let messages = strip_dangling_tool_calls(messages);
    remove_orphaned_tool_results(messages)
}

pub fn dedup_tool_results(mut messages: Vec<Message>) -> Vec<Message> {
    let mut last_positions: HashMap<String, (usize, usize)> = HashMap::new();

    for (message_idx, message) in messages.iter().enumerate() {
        if let MessageContent::Parts(parts) = &message.content {
            for (part_idx, part) in parts.iter().enumerate() {
                if let ContentPart::ToolResult { tool_call_id, .. } = part {
                    last_positions.insert(tool_call_id.clone(), (message_idx, part_idx));
                }
            }
        }
    }

    for (message_idx, message) in messages.iter_mut().enumerate() {
        if let MessageContent::Parts(parts) = &mut message.content {
            let mut part_idx = 0usize;
            parts.retain(|part| {
                let should_keep = match part {
                    ContentPart::ToolResult { tool_call_id, .. } => last_positions
                        .get(tool_call_id)
                        .is_some_and(|(last_message_idx, last_part_idx)| {
                            *last_message_idx == message_idx && *last_part_idx == part_idx
                        }),
                    _ => true,
                };
                part_idx += 1;
                should_keep
            });
            normalize_message_content(message);
        }
    }

    remove_empty_messages(messages)
}

pub fn merge_consecutive_same_role(messages: Vec<Message>) -> Vec<Message> {
    let mut merged: Vec<Message> = Vec::with_capacity(messages.len());

    for message in messages {
        let Some(previous) = merged.last_mut() else {
            merged.push(message);
            continue;
        };

        if previous.role == message.role {
            let mut previous_parts = message_parts(previous).unwrap_or_default();
            previous_parts.extend(message_parts(&message).unwrap_or_default());
            previous.content = MessageContent::Parts(previous_parts);
            normalize_message_content(previous);
        } else {
            merged.push(message);
        }
    }

    remove_empty_messages(merged)
}

pub fn truncate_old_tool_results(messages: Vec<Message>, keep_last_n: usize) -> Vec<Message> {
    if keep_last_n == usize::MAX {
        return messages;
    }

    let mut positions: Vec<(usize, usize, String)> = Vec::new();

    for (message_idx, message) in messages.iter().enumerate() {
        if let MessageContent::Parts(parts) = &message.content {
            for (part_idx, part) in parts.iter().enumerate() {
                if let ContentPart::ToolResult { tool_call_id, .. } = part {
                    positions.push((message_idx, part_idx, tool_call_id.clone()));
                }
            }
        }
    }

    if positions.len() <= keep_last_n {
        return messages;
    }

    let keep_from = positions.len().saturating_sub(keep_last_n);
    let keep_set: HashSet<(usize, usize)> = positions
        .into_iter()
        .skip(keep_from)
        .map(|(message_idx, part_idx, _)| (message_idx, part_idx))
        .collect();

    let mut truncated = messages;
    for (message_idx, message) in truncated.iter_mut().enumerate() {
        if let MessageContent::Parts(parts) = &mut message.content {
            let mut part_idx = 0usize;
            parts.retain(|part| {
                let keep = match part {
                    ContentPart::ToolResult { .. } => keep_set.contains(&(message_idx, part_idx)),
                    _ => true,
                };
                part_idx += 1;
                keep
            });
            normalize_message_content(message);
        }
    }

    remove_empty_messages(truncated)
}

pub fn truncate_old_assistant_messages(
    mut messages: Vec<Message>,
    keep_last_n: usize,
) -> Vec<Message> {
    if keep_last_n == usize::MAX {
        return messages;
    }

    let assistant_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, message)| {
            if message.role == Role::Assistant {
                Some(idx)
            } else {
                None
            }
        })
        .collect();

    if assistant_indices.len() <= keep_last_n {
        return messages;
    }

    let keep_start = assistant_indices.len().saturating_sub(keep_last_n);
    let keep_indices: HashSet<usize> = assistant_indices.into_iter().skip(keep_start).collect();

    for (idx, message) in messages.iter_mut().enumerate() {
        if message.role != Role::Assistant || keep_indices.contains(&idx) {
            continue;
        }

        match &mut message.content {
            MessageContent::Text(text) => {
                if !text.is_empty() {
                    *text = TRUNCATED_ASSISTANT_PLACEHOLDER.to_string();
                }
            }
            MessageContent::Parts(parts) => {
                parts.retain(|part| matches!(part, ContentPart::ToolCall { .. }));

                if parts.is_empty() {
                    message.content =
                        MessageContent::Text(TRUNCATED_ASSISTANT_PLACEHOLDER.to_string());
                }
            }
        }
    }

    remove_empty_messages(messages)
}

pub fn strip_dangling_tool_calls(mut messages: Vec<Message>) -> Vec<Message> {
    for idx in 0..messages.len() {
        let tool_call_ids: Vec<String> = match &messages[idx].content {
            MessageContent::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::ToolCall { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect(),
            MessageContent::Text(_) => Vec::new(),
        };

        if tool_call_ids.is_empty() {
            continue;
        }

        let next_results: HashSet<String> = messages
            .get(idx + 1)
            .and_then(|message| match &message.content {
                MessageContent::Parts(parts) => Some(
                    parts
                        .iter()
                        .filter_map(|part| match part {
                            ContentPart::ToolResult { tool_call_id, .. } => {
                                Some(tool_call_id.clone())
                            }
                            _ => None,
                        })
                        .collect::<HashSet<_>>(),
                ),
                MessageContent::Text(_) => None,
            })
            .unwrap_or_default();

        let has_immediate_results = !next_results.is_empty()
            && tool_call_ids
                .iter()
                .all(|tool_call_id| next_results.contains(tool_call_id));

        if has_immediate_results {
            continue;
        }

        if let MessageContent::Parts(parts) = &mut messages[idx].content {
            parts.retain(|part| !matches!(part, ContentPart::ToolCall { .. }));
            normalize_message_content(&mut messages[idx]);
        }
    }

    remove_empty_messages(messages)
}

pub fn remove_orphaned_tool_results(mut messages: Vec<Message>) -> Vec<Message> {
    let mut seen_tool_calls: HashSet<String> = HashSet::new();

    for message in &mut messages {
        if let MessageContent::Parts(parts) = &mut message.content {
            for part in parts.iter() {
                if let ContentPart::ToolCall { id, .. } = part {
                    seen_tool_calls.insert(id.clone());
                }
            }

            parts.retain(|part| match part {
                ContentPart::ToolResult { tool_call_id, .. } => {
                    seen_tool_calls.contains(tool_call_id)
                }
                _ => true,
            });

            normalize_message_content(message);
        }
    }

    remove_empty_messages(messages)
}

fn message_parts(message: &Message) -> Option<Vec<ContentPart>> {
    match &message.content {
        MessageContent::Text(text) => {
            if text.is_empty() {
                None
            } else {
                Some(vec![ContentPart::text(text.clone())])
            }
        }
        MessageContent::Parts(parts) => Some(parts.clone()),
    }
}

fn normalize_message_content(message: &mut Message) {
    match &message.content {
        MessageContent::Parts(parts) if parts.is_empty() => {
            message.content = MessageContent::Text(String::new());
        }
        _ => {}
    }
}

fn remove_empty_messages(messages: Vec<Message>) -> Vec<Message> {
    messages
        .into_iter()
        .filter(|message| match &message.content {
            MessageContent::Text(text) => !text.is_empty(),
            MessageContent::Parts(parts) => !parts.is_empty(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_call_message(id: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: MessageContent::Parts(vec![ContentPart::ToolCall {
                id: id.to_string(),
                name: "stakpak__view".to_string(),
                arguments: json!({"path":"README.md"}),
                provider_options: None,
                metadata: None,
            }]),
            name: None,
            provider_options: None,
        }
    }

    fn tool_result_message(id: &str, value: &str) -> Message {
        Message {
            role: Role::Tool,
            content: MessageContent::Parts(vec![ContentPart::ToolResult {
                tool_call_id: id.to_string(),
                content: json!(value),
                provider_options: None,
            }]),
            name: None,
            provider_options: None,
        }
    }

    #[test]
    fn dedup_keeps_last_tool_result_per_tool_call_id() {
        let reduced = dedup_tool_results(vec![
            tool_call_message("tc_1"),
            tool_result_message("tc_1", "old"),
            tool_result_message("tc_1", "new"),
        ]);

        assert_eq!(reduced.len(), 2);

        let last = &reduced[1];
        assert_eq!(last.role, Role::Tool);
        if let MessageContent::Parts(parts) = &last.content {
            assert_eq!(parts.len(), 1);
            assert!(matches!(
                &parts[0],
                ContentPart::ToolResult { content, .. } if content == &json!("new")
            ));
        } else {
            panic!("expected parts content for tool message");
        }
    }

    #[test]
    fn merge_consecutive_same_role_merges_tool_messages() {
        let merged = merge_consecutive_same_role(vec![
            tool_call_message("tc_1"),
            tool_result_message("tc_1", "result_1"),
            tool_result_message("tc_2", "result_2"),
        ]);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].role, Role::Tool);

        if let MessageContent::Parts(parts) = &merged[1].content {
            let tool_results = parts
                .iter()
                .filter(|part| matches!(part, ContentPart::ToolResult { .. }))
                .count();
            assert_eq!(tool_results, 2);
        } else {
            panic!("expected merged tool parts");
        }
    }

    #[test]
    fn remove_orphaned_tool_results_removes_missing_references() {
        let reduced = remove_orphaned_tool_results(vec![
            tool_result_message("tc_missing", "orphan"),
            tool_call_message("tc_1"),
            tool_result_message("tc_1", "ok"),
        ]);

        assert_eq!(reduced.len(), 2);
        assert_eq!(reduced[0].role, Role::Assistant);
        assert_eq!(reduced[1].role, Role::Tool);
    }

    #[test]
    fn truncate_old_assistant_messages_keeps_recent_context() {
        let messages = vec![
            Message::new(Role::Assistant, "older"),
            Message::new(Role::Assistant, "newer"),
            Message::new(Role::Assistant, "latest"),
        ];

        let truncated = truncate_old_assistant_messages(messages, 2);

        assert_eq!(truncated.len(), 3);
        assert_eq!(
            truncated[0].text(),
            Some(TRUNCATED_ASSISTANT_PLACEHOLDER.to_string())
        );
        assert_eq!(truncated[1].text(), Some("newer".to_string()));
        assert_eq!(truncated[2].text(), Some("latest".to_string()));
    }

    #[test]
    fn strip_dangling_tool_calls_removes_unresolved_tool_uses() {
        let assistant_with_tool_call = Message {
            role: Role::Assistant,
            content: MessageContent::Parts(vec![
                ContentPart::text("let me check"),
                ContentPart::tool_call("tc_1", "stakpak__view", json!({"path":"README.md"})),
            ]),
            name: None,
            provider_options: None,
        };

        let reduced = reduce_context(
            vec![
                assistant_with_tool_call,
                Message::new(Role::User, "new user prompt"),
                tool_result_message("tc_1", "late result"),
            ],
            &ContextConfig::default(),
        );

        // Tool call was removed because the immediate next message did not include tool_result.
        // The orphaned late tool_result is removed by remove_orphaned_tool_results().
        assert_eq!(reduced.len(), 2);
        assert_eq!(reduced[0].role, Role::Assistant);
        assert_eq!(reduced[1].role, Role::User);

        if let MessageContent::Parts(parts) = &reduced[0].content {
            assert!(
                parts
                    .iter()
                    .all(|part| !matches!(part, ContentPart::ToolCall { .. }))
            );
        } else {
            panic!("expected assistant message parts");
        }
    }

    #[test]
    fn full_reduce_pipeline_runs_in_expected_order() {
        let config = ContextConfig {
            keep_last_messages: 2,
        };

        let reduced = reduce_context(
            vec![
                tool_call_message("tc_1"),
                tool_result_message("tc_1", "old"),
                tool_result_message("tc_1", "new"),
                Message::new(Role::Assistant, "analysis"),
            ],
            &config,
        );

        // assistant tool call + last deduped tool result + assistant analysis
        assert_eq!(reduced.len(), 3);
    }
}
