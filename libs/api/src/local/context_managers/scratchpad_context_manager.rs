use once_cell::sync::Lazy;
use regex::Regex;
use stakpak_shared::models::{
    integrations::openai::{ChatMessage, Role},
    llm::{LLMMessage, LLMMessageContent},
};
use std::{collections::HashMap, fmt::Display};

pub struct ScratchpadContextManager {
    history_action_message_size_limit: usize,
    history_action_message_keep_last_n: usize,
    history_action_result_keep_last_n: usize,
}

impl super::ContextManager for ScratchpadContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        let mut scratchpad = HashMap::new();

        for message in messages.iter() {
            if let Some(content) = message.content.clone()
                && let Some(scratchpad_content) = extract_scratchpad(&content.to_string())
            {
                scratchpad.extend(scratchpad_content);
            }
        }

        let history = self.messages_to_history(&messages);
        let context_content = self.history_to_text(&history, &scratchpad);

        vec![LLMMessage {
            role: Role::User.to_string(),
            content: LLMMessageContent::String(context_content),
        }]
    }
}

pub struct ScratchpadContextManagerOptions {
    pub history_action_message_size_limit: usize,
    pub history_action_message_keep_last_n: usize,
    pub history_action_result_keep_last_n: usize,
}

impl ScratchpadContextManager {
    pub fn new(options: ScratchpadContextManagerOptions) -> Self {
        Self {
            history_action_message_size_limit: options.history_action_message_size_limit,
            history_action_message_keep_last_n: options.history_action_message_keep_last_n,
            history_action_result_keep_last_n: options.history_action_result_keep_last_n,
        }
    }

    fn messages_to_history(&self, messages: &[ChatMessage]) -> Vec<HistoryItem> {
        let mut history_items: Vec<HistoryItem> = Vec::new();
        let mut index = 0;

        for message in messages.iter() {
            match &message.role {
                Role::Assistant | Role::User if message.tool_calls.is_none() => {
                    // clean content from checkpoint_id tag
                    let content = remove_xml_tag(
                        "checkpoint_id",
                        &message.content.clone().unwrap_or_default().to_string(),
                    );
                    history_items.push(HistoryItem {
                        index,
                        content: HistoryItemContent::Message {
                            role: message.role.clone(),
                            content,
                        },
                    });
                    index += 1;
                }
                Role::Assistant | Role::User if message.tool_calls.is_some() => {
                    // clean content from checkpoint_id tag
                    let content = message
                        .content
                        .clone()
                        .map(|c| remove_xml_tag("checkpoint_id", &c.to_string()));
                    for tool_call in message.tool_calls.clone().unwrap_or_default() {
                        history_items.push(HistoryItem {
                            index,
                            content: HistoryItemContent::Action {
                                role: message.role.clone(),
                                id: tool_call.id.clone(),
                                name: tool_call.function.name.clone(),
                                status: HistoryItemActionStatus::Pending,
                                message: content.clone(),
                                arguments: serde_json::from_str(&tool_call.function.arguments)
                                    .unwrap_or_default(),
                                result: None,
                            },
                        });
                        index += 1;
                    }
                }
                Role::Tool => {
                    // Find the corresponding tool call item and update it with the result
                    if let Some(tool_call_id) = &message.tool_call_id {
                        // Look for the matching tool call in history items
                        if let Some(history_item) = history_items.iter_mut().find(|item| {
                            if let HistoryItemContent::Action { id, .. } = &item.content {
                                *id == *tool_call_id
                            } else {
                                false
                            }
                        }) {
                            // Update the tool call with the result
                            if let HistoryItemContent::Action { status, result, .. } =
                                &mut history_item.content
                            {
                                let result_content =
                                    message.content.clone().unwrap_or_default().to_string();
                                *result = serde_json::from_str(&result_content)
                                    .unwrap_or(Some(serde_json::Value::String(result_content)));

                                if let Some(result) = result
                                    && result.as_str().unwrap_or_default() == "TOOL_CALL_CANCELLED"
                                {
                                    *status = HistoryItemActionStatus::Aborted;
                                    continue;
                                }
                                *status = HistoryItemActionStatus::Completed;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // keep the full last message of a tool call action to prevent scratchpad update verbosity
        drop_older_action_messages(
            &mut history_items,
            self.history_action_message_size_limit,
            self.history_action_message_keep_last_n,
        );
        // replace older action results with a placeholder to conserve context
        drop_older_action_results(&mut history_items, self.history_action_result_keep_last_n);

        history_items
    }

    pub fn history_to_text(
        &self,
        history: &[HistoryItem],
        scratchpad: &HashMap<String, String>,
    ) -> String {
        let mut content = String::new();

        content.push_str(&format!(
            r#"
<scratchpad>
{}
</scratchpad>"#,
            scratchpad
                // .as_object()
                // .unwrap_or(&serde_json::Map::new())
                .iter()
                .map(|(key, value)| format!(
                    r#"
<{}>
{}
</{}>
"#,
                    key,
                    value, // .as_str()
                    // .unwrap_or(serde_yaml::to_string(&value).unwrap_or_default().trim())
                    key
                ))
                .collect::<Vec<String>>()
                .join("\n")
        ));

        content.push_str(&format!(
            r#"
<history>
{}
</history>"#,
            history
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        ));

        content.trim().to_string()
    }
}

fn remove_xml_tag(tag_name: &str, content: &str) -> String {
    #[allow(clippy::unwrap_used)]
    let xml_tag_regex =
        Regex::new(format!("<{}>(?s)(.*?)</{}>", tag_name, tag_name).as_str()).unwrap();
    xml_tag_regex.replace_all(content, "").trim().to_string()
}

fn get_threshold_idx(history_items: &[HistoryItem], keep_last_n: usize) -> Option<usize> {
    let action_indices: Vec<usize> = history_items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.content.is_action())
        .map(|(idx, _)| idx)
        .collect();
    let keep_from_action_idx = action_indices.len().saturating_sub(keep_last_n);
    action_indices.get(keep_from_action_idx).copied()
}

fn drop_older_action_results(history_items: &mut [HistoryItem], keep_last_n: usize) {
    let threshold_idx = get_threshold_idx(history_items, keep_last_n);

    for (idx, history_item) in history_items.iter_mut().enumerate() {
        let should_drop = history_item.content.is_action()
            && threshold_idx.is_none_or(|threshold| idx < threshold);
        if should_drop && let HistoryItemContent::Action { result, .. } = &mut history_item.content
        {
            *result = Some(serde_json::Value::String(
                    "[This result was truncated from history to conserve space, consult the scratchpad instead]".to_string(),
                ));
        }
    }
}

fn drop_older_action_messages(
    history_items: &mut [HistoryItem],
    message_size_limit: usize,
    keep_last_n: usize,
) {
    let threshold_idx = get_threshold_idx(history_items, keep_last_n);

    for (idx, history_item) in history_items.iter_mut().enumerate() {
        let should_drop = history_item.content.is_action()
            && threshold_idx.is_none_or(|threshold| idx < threshold);
        if should_drop
            && let HistoryItemContent::Action { message, .. } = &mut history_item.content
            && let Some(msg) = message
            && msg.chars().count() > message_size_limit
        {
            *message = None;
        }
    }
}

fn extract_scratchpad(content: &str) -> Option<HashMap<String, String>> {
    static SCRATCHPAD_RE: Lazy<Option<Regex>> =
        Lazy::new(|| match Regex::new(r"<scratchpad>(?s)(.*?)</scratchpad>") {
            Ok(re) => Some(re),
            Err(e) => {
                println!("Failed to create scratchpad regex: {}", e);
                None
            }
        });

    static XML_TAG_RE: Lazy<Option<Regex>> = Lazy::new(|| {
        match Regex::new(r"<([a-zA-Z_][a-zA-Z0-9_-]*?)>(?s)(.*?)</([a-zA-Z_][a-zA-Z0-9_-]*?)>") {
            Ok(re) => Some(re),
            Err(e) => {
                println!("Failed to create XML tag regex: {}", e);
                None
            }
        }
    });

    // First extract the scratchpad content
    let scratchpad_content = match SCRATCHPAD_RE.as_ref() {
        Some(re) => re
            .captures(content)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().trim().to_string())?,
        None => return None,
    };

    // Then extract all XML tags within the scratchpad content
    let mut result = HashMap::new();

    if let Some(xml_re) = XML_TAG_RE.as_ref() {
        for cap in xml_re.captures_iter(&scratchpad_content) {
            if let (Some(opening_tag), Some(tag_content), Some(closing_tag)) =
                (cap.get(1), cap.get(2), cap.get(3))
            {
                let opening_name = opening_tag.as_str();
                let closing_name = closing_tag.as_str();

                // Only include if opening and closing tags match
                if opening_name == closing_name {
                    result.insert(
                        opening_name.to_string(),
                        tag_content.as_str().trim().to_string(),
                    );
                }
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_action_item(index: usize, message: Option<String>) -> HistoryItem {
        HistoryItem {
            index,
            content: HistoryItemContent::Action {
                role: Role::Assistant,
                id: format!("action_{}", index),
                name: "test_action".to_string(),
                status: HistoryItemActionStatus::Completed,
                message,
                arguments: serde_json::json!({}),
                result: None,
            },
        }
    }

    fn create_message_item(index: usize) -> HistoryItem {
        HistoryItem {
            index,
            content: HistoryItemContent::Message {
                role: Role::User,
                content: "test message".to_string(),
            },
        }
    }

    fn get_action_message(history_item: &HistoryItem) -> Option<&String> {
        if let HistoryItemContent::Action { message, .. } = &history_item.content {
            message.as_ref()
        } else {
            None
        }
    }

    #[test]
    fn test_drop_older_action_messages_n_zero_drops_all_large() {
        // Test n = 0: should drop all action messages larger than size limit
        let mut history_items = vec![
            create_action_item(0, Some("small".to_string())), // 5 chars, below limit
            create_action_item(1, Some("x".repeat(100))),     // 100 chars, above limit
            create_action_item(2, Some("y".repeat(50))),      // 50 chars, above limit
            create_message_item(3),                           // Not an action, should be untouched
            create_action_item(4, Some("z".repeat(10))),      // 10 chars, below limit
        ];

        drop_older_action_messages(&mut history_items, 20, 0);

        // All action messages above size limit (20) should be dropped
        assert_eq!(
            get_action_message(&history_items[0]),
            Some(&"small".to_string())
        );
        assert_eq!(get_action_message(&history_items[1]), None); // Dropped (100 > 20)
        assert_eq!(get_action_message(&history_items[2]), None); // Dropped (50 > 20)
        assert_eq!(get_action_message(&history_items[4]), Some(&"z".repeat(10)));
        // Kept (10 <= 20)
    }

    #[test]
    fn test_drop_older_action_messages_keep_last_n() {
        // Test keeping last 2 actions
        // The function only drops messages that exceed size_limit, and only for actions NOT in the last n
        let mut history_items = vec![
            create_action_item(0, Some("x".repeat(100))), // Should be dropped (old, > limit)
            create_action_item(1, Some("y".repeat(100))), // Should be dropped (old, > limit)
            create_message_item(2),                       // Not an action
            create_action_item(3, Some("z".repeat(100))), // Should be kept (last 2, even if > limit)
            create_action_item(4, Some("w".repeat(10))),  // Should be kept (last 2, <= limit)
        ];

        drop_older_action_messages(&mut history_items, 20, 2);

        // First two actions should be dropped (not in last 2, and > limit)
        assert_eq!(get_action_message(&history_items[0]), None);
        assert_eq!(get_action_message(&history_items[1]), None);
        // Last two actions should be kept (in last 2, so not dropped even if > limit)
        assert_eq!(
            get_action_message(&history_items[3]),
            Some(&"z".repeat(100))
        ); // Kept (in last 2)
        assert_eq!(get_action_message(&history_items[4]), Some(&"w".repeat(10)));
        // Kept (in last 2)
    }

    #[test]
    fn test_drop_older_action_messages_off_by_one() {
        // Test off-by-one: keep exactly n actions
        let mut history_items = vec![
            create_action_item(0, Some("a".repeat(100))), // Should be dropped (not in last 3)
            create_action_item(1, Some("b".repeat(100))), // Should be dropped (not in last 3)
            create_action_item(2, Some("c".repeat(100))), // Should be kept (in last 3)
            create_action_item(3, Some("d".repeat(100))), // Should be kept (in last 3)
            create_action_item(4, Some("e".repeat(100))), // Should be kept (in last 3)
        ];

        drop_older_action_messages(&mut history_items, 20, 3);

        // First two should be dropped
        assert_eq!(get_action_message(&history_items[0]), None);
        assert_eq!(get_action_message(&history_items[1]), None);
        // Last three should be kept
        assert_eq!(
            get_action_message(&history_items[2]),
            Some(&"c".repeat(100))
        );
        assert_eq!(
            get_action_message(&history_items[3]),
            Some(&"d".repeat(100))
        );
        assert_eq!(
            get_action_message(&history_items[4]),
            Some(&"e".repeat(100))
        );
    }

    #[test]
    fn test_drop_older_action_messages_all_below_limit() {
        // Test that actions below size limit are never dropped
        let mut history_items = vec![
            create_action_item(0, Some("a".repeat(10))), // Below limit
            create_action_item(1, Some("b".repeat(10))), // Below limit
            create_action_item(2, Some("c".repeat(10))), // Below limit
        ];

        drop_older_action_messages(&mut history_items, 20, 1);

        // All should be kept since they're all below limit
        assert_eq!(get_action_message(&history_items[0]), Some(&"a".repeat(10)));
        assert_eq!(get_action_message(&history_items[1]), Some(&"b".repeat(10)));
        assert_eq!(get_action_message(&history_items[2]), Some(&"c".repeat(10)));
    }

    #[test]
    fn test_drop_older_action_messages_empty_history() {
        // Test with empty history
        let mut history_items = vec![];

        drop_older_action_messages(&mut history_items, 20, 5);

        assert!(history_items.is_empty());
    }

    #[test]
    fn test_drop_older_action_messages_no_actions() {
        // Test with no action items
        let mut history_items = vec![
            create_message_item(0),
            create_message_item(1),
            create_message_item(2),
        ];

        let original_len = history_items.len();
        drop_older_action_messages(&mut history_items, 20, 5);

        // Nothing should change - length should be the same and all should still be messages
        assert_eq!(history_items.len(), original_len);
        assert!(!history_items[0].content.is_action());
        assert!(!history_items[1].content.is_action());
        assert!(!history_items[2].content.is_action());
    }

    #[test]
    fn test_drop_older_action_messages_keep_more_than_exist() {
        // Test keeping more actions than exist
        // When keep_last_n > number of actions, all actions are in the "last n", so none are dropped
        let mut history_items = vec![
            create_action_item(0, Some("x".repeat(100))), // Should be kept (all actions are in "last 10")
            create_action_item(1, Some("y".repeat(10))), // Should be kept (all actions are in "last 10")
        ];

        drop_older_action_messages(&mut history_items, 20, 10); // Keep 10, but only 2 exist

        // Both should be kept since all actions are in the "last 10"
        assert_eq!(
            get_action_message(&history_items[0]),
            Some(&"x".repeat(100))
        );
        assert_eq!(get_action_message(&history_items[1]), Some(&"y".repeat(10)));
    }

    #[test]
    fn test_drop_older_action_messages_mixed_with_messages() {
        // Test with actions mixed with regular messages
        let mut history_items = vec![
            create_action_item(0, Some("x".repeat(100))), // Should be dropped
            create_message_item(1),                       // Should be untouched
            create_action_item(2, Some("y".repeat(100))), // Should be dropped
            create_message_item(3),                       // Should be untouched
            create_action_item(4, Some("z".repeat(10))),  // Should be kept (last 1, small)
        ];

        drop_older_action_messages(&mut history_items, 20, 1);

        assert_eq!(get_action_message(&history_items[0]), None);
        assert_eq!(get_action_message(&history_items[2]), None);
        assert_eq!(get_action_message(&history_items[4]), Some(&"z".repeat(10)));
        // Messages should be untouched
        assert!(!history_items[1].content.is_action());
        assert!(!history_items[3].content.is_action());
    }

    #[test]
    fn test_hashmap_extend_overrides_old_values() {
        // Test that HashMap::extend overrides older values with newer values when keys already exist
        let mut scratchpad = HashMap::new();

        // Initial scratchpad state
        scratchpad.insert("key1".to_string(), "old_value1".to_string());
        scratchpad.insert("key2".to_string(), "old_value2".to_string());
        scratchpad.insert("key3".to_string(), "unchanged".to_string());

        // New content with overlapping keys
        let mut new_content = HashMap::new();
        new_content.insert("key1".to_string(), "new_value1".to_string());
        new_content.insert("key2".to_string(), "new_value2".to_string());
        new_content.insert("key4".to_string(), "added_value".to_string());

        // Extend the scratchpad (this is what happens in the reduce_context method)
        scratchpad.extend(new_content);

        // Verify that older values were overridden
        assert_eq!(scratchpad.get("key1"), Some(&"new_value1".to_string()));
        assert_eq!(scratchpad.get("key2"), Some(&"new_value2".to_string()));
        // Verify that unchanged keys remain
        assert_eq!(scratchpad.get("key3"), Some(&"unchanged".to_string()));
        // Verify that new keys were added
        assert_eq!(scratchpad.get("key4"), Some(&"added_value".to_string()));
        // Verify total count
        assert_eq!(scratchpad.len(), 4);
    }
}

pub struct HistoryItem {
    pub index: usize,
    pub content: HistoryItemContent,
}

impl Display for HistoryItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.content {
            HistoryItemContent::Message { role, content } => {
                write!(
                    f,
                    "<message index=\"{}\" role=\"{}\">\n{}\n</message>",
                    self.index,
                    role,
                    content.trim()
                )?;
            }
            HistoryItemContent::Action {
                role,
                id: _,
                name,
                status,
                message,
                arguments,
                result,
            } => {
                write!(
                    f,
                    "<action index=\"{}\" role=\"{}\" name=\"{}\" status=\"{}\">",
                    self.index, role, name, status
                )?;

                if let Some(message) = message {
                    write!(f, "\n{}", message.trim())?;
                }
                write!(
                    f,
                    "\n<arguments>\n{}\n</arguments>",
                    serde_yaml::to_string(&arguments).unwrap_or_default().trim()
                )?;
                if let Some(result) = result {
                    let result_str = if let serde_json::Value::String(s) = result {
                        s.trim().to_string()
                    } else {
                        serde_yaml::to_string(&result)
                            .unwrap_or_default()
                            .trim()
                            .to_string()
                    };
                    if !result_str.is_empty() {
                        write!(f, "\n<result>\n{result_str}\n</result>")?;
                    }
                }
                write!(f, "\n</action>")?;
            }
        };

        Ok(())
    }
}

pub enum HistoryItemContent {
    Message {
        role: Role,
        content: String,
    },
    Action {
        role: Role,
        id: String,
        name: String,
        status: HistoryItemActionStatus,
        message: Option<String>,
        arguments: serde_json::Value,
        result: Option<serde_json::Value>,
    },
}

impl HistoryItemContent {
    pub fn is_action(&self) -> bool {
        matches!(self, HistoryItemContent::Action { .. })
    }
}

pub enum HistoryItemActionStatus {
    Pending,
    Completed,
    Aborted,
}

impl std::fmt::Display for HistoryItemActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Completed => write!(f, "completed"),
            Self::Aborted => write!(f, "aborted"),
        }
    }
}
