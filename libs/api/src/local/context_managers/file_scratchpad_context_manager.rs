use super::common::{HistoryItemActionStatus, HistoryItemContent, remove_xml_tag};
use std::{
    collections::HashSet,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
};
use uuid::Uuid;

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
    overwrite_if_different: bool,
    pub recovered_sessions: RwLock<HashSet<String>>,
}

pub struct FileScratchpadContextManagerOptions {
    pub history_action_message_size_limit: usize,
    pub history_action_message_keep_last_n: usize,
    pub history_action_result_keep_last_n: usize,
    pub scratchpad_file_path: PathBuf,
    pub todo_file_path: PathBuf,
    pub overwrite_if_different: bool,
}

impl super::ContextManager for FileScratchpadContextManager {
    fn reduce_context(&self, messages: Vec<stakai::Message>) -> Vec<stakai::Message> {
        self.reduce_context_with_session(messages, None)
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
            overwrite_if_different: options.overwrite_if_different,
            recovered_sessions: RwLock::new(HashSet::new()),
        }
    }

    pub fn get_scratchpad_path(&self, session_id: Option<Uuid>) -> PathBuf {
        self.resolve_path(&self.scratchpad_file_path, session_id)
    }

    pub fn get_todo_path(&self, session_id: Option<Uuid>) -> PathBuf {
        self.resolve_path(&self.todo_file_path, session_id)
    }

    fn load_file(&self, path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    fn resolve_path(&self, base_path: &Path, session_id: Option<Uuid>) -> PathBuf {
        if let Some(session_id) = session_id
            && let Some(parent) = base_path.parent()
        {
            // If there's a session ID, put files in a subdirectory named after the session ID
            // e.g. .stakpak/session/scratchpad.md -> .stakpak/session/<uuid>/scratchpad.md
            let session_dir = parent.join(session_id.to_string());
            return session_dir.join(base_path.file_name().unwrap_or_default());
        }
        base_path.to_path_buf()
    }

    pub fn reduce_context_with_session(
        &self,
        messages: Vec<stakai::Message>,
        session_id: Option<Uuid>,
    ) -> Vec<stakai::Message> {
        let session_key = session_id.map(|u| u.to_string()).unwrap_or_default();
        let should_recover = {
            let recovered = self.recovered_sessions.read().unwrap();
            !recovered.contains(&session_key)
        };

        if should_recover {
            self.recover_from_history(&messages, session_id);
            let mut recovered = self.recovered_sessions.write().unwrap();
            recovered.insert(session_key);
        }

        let scratchpad_content = self.load_file(&self.get_scratchpad_path(session_id));
        let todo_content = self.load_file(&self.get_todo_path(session_id));

        let history = self.messages_to_history(&messages, session_id);
        let context_content = self.history_to_text(&history, &scratchpad_content, &todo_content);

        vec![stakai::Message::new(stakai::Role::User, context_content)]
    }

    fn recover_from_history(&self, messages: &[stakai::Message], session_id: Option<Uuid>) {
        self.recover_file(messages, &self.get_scratchpad_path(session_id));
        self.recover_file(messages, &self.get_todo_path(session_id));
    }

    fn recover_file(&self, messages: &[stakai::Message], path: &Path) {
        // Try to reconstruct from tool calls
        let reconstructed_content = self.reconstruct_file_from_history(messages, path);

        if let Some(content) = reconstructed_content {
            let exists = path.exists();
            let current_content = if exists {
                fs::read_to_string(path).unwrap_or_default()
            } else {
                String::new()
            };

            let should_write = if !exists || current_content.trim().is_empty() {
                // File missing or empty -> recover
                true
            } else if self.overwrite_if_different {
                // File exists, check if content differs
                current_content.trim() != content.trim()
            } else {
                false
            };

            if should_write {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(path, content);
            }
        }
    }

    fn reconstruct_file_from_history(
        &self,
        messages: &[stakai::Message],
        path: &Path,
    ) -> Option<String> {
        let mut current_content: Option<String> = None;
        let path_str = path.to_string_lossy();

        for message in messages {
            for part in message.parts() {
                if let stakai::ContentPart::ToolCall {
                    name, arguments, ..
                } = part
                {
                    self.apply_tool_call(&name, &arguments, &path_str, &mut current_content);
                }
            }
        }

        current_content
    }

    fn apply_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        target_path: &str,
        current_content: &mut Option<String>,
    ) {
        if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
            if !path.contains(target_path) {
                return;
            }

            match tool_name {
                "create" => {
                    if let Some(file_text) = args.get("file_text").and_then(|t| t.as_str()) {
                        *current_content = Some(file_text.to_string());
                    }
                }
                "str_replace" => {
                    if let (Some(old_str), Some(new_str)) = (
                        args.get("old_str").and_then(|s| s.as_str()),
                        args.get("new_str").and_then(|s| s.as_str()),
                    ) && let Some(content) = current_content
                    {
                        let replace_all = args
                            .get("replace_all")
                            .and_then(|b| b.as_bool())
                            .unwrap_or(false);
                        if replace_all {
                            *content = content.replace(old_str, new_str);
                        } else {
                            *content = content.replacen(old_str, new_str, 1);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn messages_to_history(
        &self,
        messages: &[stakai::Message],
        session_id: Option<Uuid>,
    ) -> Vec<HistoryItem> {
        let mut history_items: Vec<HistoryItem> = Vec::new();
        let mut index = 0;

        for (message_index, message) in messages.iter().enumerate() {
            let parts = message.parts();
            let tool_calls: Vec<_> = parts
                .iter()
                .filter_map(|part| match part {
                    stakai::ContentPart::ToolCall {
                        id,
                        name,
                        arguments,
                        ..
                    } => Some((id.clone(), name.clone(), arguments.clone())),
                    _ => None,
                })
                .collect();

            match message.role {
                stakai::Role::Assistant | stakai::Role::User if tool_calls.is_empty() => {
                    let content =
                        remove_xml_tag("checkpoint_id", &message.text().unwrap_or_default());
                    history_items.push(HistoryItem {
                        index,
                        message_index,
                        content: HistoryItemContent::Message {
                            role: super::common::role_label(message.role).to_string(),
                            content,
                        },
                    });
                    index += 1;
                }
                stakai::Role::Assistant | stakai::Role::User => {
                    let content = message
                        .text()
                        .map(|text| remove_xml_tag("checkpoint_id", &text));
                    for (id, name, arguments) in tool_calls {
                        history_items.push(HistoryItem {
                            index,
                            message_index,
                            content: HistoryItemContent::Action {
                                role: super::common::role_label(message.role).to_string(),
                                id,
                                name,
                                status: HistoryItemActionStatus::Pending,
                                message: content.clone(),
                                arguments,
                                result: None,
                            },
                        });
                        index += 1;
                    }
                }
                stakai::Role::Tool => {
                    for part in parts {
                        let stakai::ContentPart::ToolResult {
                            tool_call_id,
                            content,
                            ..
                        } = part
                        else {
                            continue;
                        };
                        if let Some(history_item) = history_items.iter_mut().find(|item| {
                            if let HistoryItemContent::Action { id, .. } = &item.content {
                                *id == tool_call_id
                            } else {
                                false
                            }
                        }) && let HistoryItemContent::Action { status, result, .. } =
                            &mut history_item.content
                        {
                            *result = Some(content.clone());

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
                _ => {}
            }
        }

        // Filter out scratchpad/todo file operations from history
        drop_scratchpad_actions(
            &mut history_items,
            &self.get_scratchpad_path(session_id),
            &self.get_todo_path(session_id),
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
    scratchpad_path: &Path,
    todo_path: &Path,
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

            if let Some(path) = arguments.get("path").and_then(|p| p.as_str())
                && (path.contains(scratchpad_str.as_ref()) || path.contains(todo_str.as_ref()))
            {
                return true;
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

/// Extended HistoryItem with message_index for file scratchpad context manager
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
