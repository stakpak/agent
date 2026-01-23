use regex::Regex;
use stakpak_shared::models::{
    integrations::openai::{ChatMessage, Role},
    llm::{LLMMessage, LLMMessageContent},
};
use std::fmt::Display;

pub struct TaskBoardContextManager {
    history_action_message_size_limit: usize,
    history_action_message_keep_last_n: usize,
    history_action_result_keep_last_n: usize,
}

impl super::ContextManager for TaskBoardContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        let history = self.messages_to_history(&messages);
        let context_content = self.history_to_text(&history);

        vec![LLMMessage {
            role: Role::User.to_string(),
            content: LLMMessageContent::String(context_content),
        }]
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

        // keep the full last message of a tool call action to prevent verbosity
        drop_older_action_messages(
            &mut history_items,
            self.history_action_message_size_limit,
            self.history_action_message_keep_last_n,
        );
        // replace older action results with a placeholder to conserve context
        drop_older_action_results(&mut history_items, self.history_action_result_keep_last_n);

        history_items
    }

    pub fn history_to_text(&self, history: &[HistoryItem]) -> String {
        format!(
            "<history>\n{}\n</history>",
            history
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<String>>()
                .join("\n"),
        )
        .trim()
        .to_string()
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
                    "[This result was truncated from history to conserve space]".to_string(),
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
