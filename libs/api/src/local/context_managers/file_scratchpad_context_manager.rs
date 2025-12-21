use regex::Regex;
use stakpak_shared::models::{
    integrations::openai::{ChatMessage, Role},
    llm::{LLMMessage, LLMMessageContent},
};
use std::{fmt::Display, fs, path::PathBuf};

/// A context manager where the agent edits scratchpad/todo files directly using file tools.
///
/// Key differences from ScratchpadContextManager:
/// - Agent uses str_replace/view/create tools to edit .stakpak/scratchpad.md and .stakpak/todo.md
/// - Context manager reads these files and injects their content into the context
/// - No XML tag parsing from agent responses - files are the source of truth
/// - System prompt instructs agent to use file tools instead of <scratchpad> XML tags
pub struct FileScratchpadContextManager {
    history_action_message_size_limit: usize,
    history_action_message_keep_last_n: usize,
    history_action_result_keep_last_n: usize,
    scratchpad_file_path: PathBuf,
    todo_file_path: PathBuf,
}

pub struct FileScratchpadContextManagerOptions {
    pub history_action_message_size_limit: usize,
    pub history_action_message_keep_last_n: usize,
    pub history_action_result_keep_last_n: usize,
    pub scratchpad_file_path: PathBuf,
    pub todo_file_path: PathBuf,
}

impl super::ContextManager for FileScratchpadContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        let scratchpad_content = self.load_scratchpad();
        let todo_content = self.load_todo();

        let history = self.messages_to_history(&messages);
        let context_content = self.history_to_text(&history, &scratchpad_content, &todo_content);

        vec![LLMMessage {
            role: Role::User.to_string(),
            content: LLMMessageContent::String(context_content),
        }]
    }
}

impl FileScratchpadContextManager {
    pub fn new(options: FileScratchpadContextManagerOptions) -> Self {
        if let Some(parent) = options.scratchpad_file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Some(parent) = options.todo_file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        Self {
            history_action_message_size_limit: options.history_action_message_size_limit,
            history_action_message_keep_last_n: options.history_action_message_keep_last_n,
            history_action_result_keep_last_n: options.history_action_result_keep_last_n,
            scratchpad_file_path: options.scratchpad_file_path,
            todo_file_path: options.todo_file_path,
        }
    }

    pub fn get_scratchpad_path(&self) -> &PathBuf {
        &self.scratchpad_file_path
    }

    pub fn get_todo_path(&self) -> &PathBuf {
        &self.todo_file_path
    }

    fn load_scratchpad(&self) -> String {
        fs::read_to_string(&self.scratchpad_file_path).unwrap_or_default()
    }

    fn load_todo(&self) -> String {
        fs::read_to_string(&self.todo_file_path).unwrap_or_default()
    }

    fn messages_to_history(&self, messages: &[ChatMessage]) -> Vec<HistoryItem> {
        let mut history_items: Vec<HistoryItem> = Vec::new();
        let mut index = 0;

        for (message_index, message) in messages.iter().enumerate() {
            match &message.role {
                Role::Assistant | Role::User if message.tool_calls.is_none() => {
                    // clean content from checkpoint_id tag
                    let content = remove_xml_tag(
                        "checkpoint_id",
                        &message.content.clone().unwrap_or_default().to_string(),
                    );
                    history_items.push(HistoryItem {
                        index,
                        message_index,
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
                            message_index,
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

        // Filter out scratchpad/todo file operations from history
        drop_scratchpad_actions(
            &mut history_items,
            &self.scratchpad_file_path,
            &self.todo_file_path,
        );

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

    pub fn history_to_text(&self, history: &[HistoryItem], scratchpad: &str, todo: &str) -> String {
        let mut content = String::new();

        if !scratchpad.trim().is_empty() {
            content.push_str(&format!(
                r#"
<scratchpad>
{}
</scratchpad>"#,
                scratchpad.trim()
            ));
        }

        if !todo.trim().is_empty() {
            content.push_str(&format!(
                r#"
<todo>
{}
</todo>"#,
                todo.trim()
            ));
        }

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

/// Filter out tool calls that target scratchpad or todo files from older history.
/// Keeps the most recent scratchpad/todo actions so the agent knows what it just did.
fn drop_scratchpad_actions(
    history_items: &mut Vec<HistoryItem>,
    scratchpad_path: &PathBuf,
    todo_path: &PathBuf,
) {
    let scratchpad_str = scratchpad_path.to_string_lossy();
    let todo_str = todo_path.to_string_lossy();

    let is_scratchpad_action = |item: &HistoryItem| -> bool {
        if let HistoryItemContent::Action {
            name, arguments, ..
        } = &item.content
        {
            let file_tools = ["view", "str_replace", "create", "remove"];
            if !file_tools.contains(&name.as_str()) {
                return false;
            }

            if let Some(path) = arguments.get("path").and_then(|p| p.as_str()) {
                if path.contains(scratchpad_str.as_ref()) || path.contains(todo_str.as_ref()) {
                    return true;
                }
            }
        }
        false
    };

    // Find the highest message_index in history (represents the most recent message)
    let max_message_index = history_items
        .iter()
        .map(|item| item.message_index)
        .max()
        .unwrap_or(0);

    // Keep scratchpad actions from the last message (same message_index as max),
    // filter out older ones
    history_items.retain(|item| {
        if is_scratchpad_action(item) {
            item.message_index == max_message_index
        } else {
            true
        }
    });
}

pub struct HistoryItem {
    pub index: usize,
    /// The index of the original message this history item came from
    pub message_index: usize,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> (FileScratchpadContextManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let scratchpad_path = temp_dir.path().join("scratchpad.md");
        let todo_path = temp_dir.path().join("todo.md");

        let manager = FileScratchpadContextManager::new(FileScratchpadContextManagerOptions {
            history_action_message_size_limit: 100,
            history_action_message_keep_last_n: 1,
            history_action_result_keep_last_n: 50,
            scratchpad_file_path: scratchpad_path,
            todo_file_path: todo_path,
        });

        (manager, temp_dir)
    }

    #[test]
    fn test_load_empty_files() {
        let (manager, _temp_dir) = create_test_manager();

        // Without creating files, load should return empty strings
        let scratchpad = manager.load_scratchpad();
        let todo = manager.load_todo();

        assert!(scratchpad.is_empty());
        assert!(todo.is_empty());
    }

    #[test]
    fn test_load_existing_files() {
        let (manager, _temp_dir) = create_test_manager();

        // Write content to files (simulating agent file edits)
        fs::write(
            &manager.scratchpad_file_path,
            "# My Notes\nSome important info",
        )
        .unwrap();
        fs::write(&manager.todo_file_path, "- [x] Task 1\n- [ ] Task 2").unwrap();

        // Load and verify
        let scratchpad = manager.load_scratchpad();
        let todo = manager.load_todo();

        assert!(scratchpad.contains("My Notes"));
        assert!(scratchpad.contains("important info"));
        assert!(todo.contains("Task 1"));
        assert!(todo.contains("Task 2"));
    }

    #[test]
    fn test_history_to_text_with_content() {
        let (manager, _temp_dir) = create_test_manager();

        let history = vec![];
        let scratchpad = "# Notes\nKey info here";
        let todo = "- [ ] Do something";

        let result = manager.history_to_text(&history, scratchpad, todo);

        assert!(result.contains("<scratchpad>"));
        assert!(result.contains("Key info here"));
        assert!(result.contains("<todo>"));
        assert!(result.contains("Do something"));
        assert!(result.contains("<history>"));
    }

    #[test]
    fn test_history_to_text_empty_scratchpad() {
        let (manager, _temp_dir) = create_test_manager();

        let history = vec![];
        let scratchpad = "";
        let todo = "";

        let result = manager.history_to_text(&history, scratchpad, todo);

        // Should not include empty scratchpad/todo sections
        assert!(!result.contains("<scratchpad>"));
        assert!(!result.contains("<todo>"));
        assert!(result.contains("<history>"));
    }

    #[test]
    fn test_paths_accessible() {
        let (manager, temp_dir) = create_test_manager();

        // Verify paths are accessible for system prompt generation
        assert_eq!(
            &manager.scratchpad_file_path,
            &temp_dir.path().join("scratchpad.md")
        );
        assert_eq!(&manager.todo_file_path, &temp_dir.path().join("todo.md"));
    }
}
