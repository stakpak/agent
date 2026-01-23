use super::common::{HistoryItem, HistoryProcessingOptions, messages_to_history};
use once_cell::sync::Lazy;
use regex::Regex;
use stakpak_shared::models::{
    integrations::openai::{ChatMessage, Role},
    llm::{LLMMessage, LLMMessageContent},
};
use std::collections::HashMap;

pub struct ScratchpadContextManager {
    options: HistoryProcessingOptions,
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

        let history = messages_to_history(&messages, &self.options);
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
            options: HistoryProcessingOptions {
                history_action_message_size_limit: options.history_action_message_size_limit,
                history_action_message_keep_last_n: options.history_action_message_keep_last_n,
                history_action_result_keep_last_n: options.history_action_result_keep_last_n,
                truncation_hint: "consult the scratchpad instead".to_string(),
            },
        }
    }

    fn history_to_text(
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
