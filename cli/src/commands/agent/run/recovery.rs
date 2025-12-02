use crate::commands::agent::run::checkpoint::{
    get_checkpoint_messages, prepare_checkpoint_messages_and_tool_calls,
};
use serde::{Deserialize, Serialize};
use stakpak_api::AgentProvider;
use stakpak_shared::models::integrations::openai::{ChatMessage, MessageContent, Role};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOperation {
    Append,
    Truncate,
    RemoveTools,
    RevertToCheckpoint,
    ChangeModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAction {
    pub message_index: usize,
    pub role: Option<Role>,
    pub content: Option<MessageContent>,
    pub failed_tool_call_ids_to_remove: Option<Vec<String>>,
    pub recovery_operation: RecoveryOperation,
    pub revert_to_checkpoint: Option<String>,
    pub model_config: Option<ModelConfig>,
    pub explanation: Option<String>,
}

fn assistant_msg_with_checkpoint_id(checkpoint_id: &str) -> ChatMessage {
    let checkpoint_msg = format!("<checkpoint_id>{}</checkpoint_id>", checkpoint_id);
    ChatMessage {
        role: Role::Assistant,
        content: Some(MessageContent::String(checkpoint_msg)),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
    }
}

fn last_msg_checkpoint(messages: &[ChatMessage]) -> Option<&str> {
    messages.last().and_then(|msg| {
        if msg.role == Role::Assistant {
            msg.content.as_ref().and_then(|content| {
                if let MessageContent::String(content) = content {
                    if content.starts_with("<checkpoint_id>") {
                        content
                            .split_once('>')
                            .and_then(|(_, id)| id.split_once('<').map(|(_, id)| id))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        } else {
            None
        }
    })
}

pub async fn handle_revert_to_checkpoint(
    client: &dyn AgentProvider,
    checkpoint_id: &String,
    messages: &mut Vec<ChatMessage>,
) -> Result<(), String> {
    let checkpoint_messages = get_checkpoint_messages(client, checkpoint_id).await?;
    let (mut chat_messages, _) =
        prepare_checkpoint_messages_and_tool_calls(checkpoint_id, checkpoint_messages);

    // Check the NEW messages after revert, not the old ones
    let last_msg_checkpoit = last_msg_checkpoint(&chat_messages);
    if let Some(last_msg) = chat_messages.last()
        && last_msg.role == Role::User
        && let Some(checkpoint_id) = last_msg_checkpoit
    {
        chat_messages.push(assistant_msg_with_checkpoint_id(checkpoint_id));
    }

    *messages = chat_messages;
    Ok(())
}

pub fn handle_truncate(messages: &mut Vec<ChatMessage>, index: usize) {
    if index < messages.len() {
        messages.truncate(index);

        // After truncating, check if the new last message is a User message
        let last_msg_checkpoit = last_msg_checkpoint(messages);
        if let Some(last_msg) = messages.last()
            && last_msg.role == Role::User
            && let Some(checkpoint_id) = last_msg_checkpoit
        {
            messages.push(assistant_msg_with_checkpoint_id(checkpoint_id));
        }
    }
}

pub fn handle_remove_tools(messages: &mut Vec<ChatMessage>, tool_ids: &[String]) {
    clean_state_from_tool_failures(messages, tool_ids);
}

pub fn handle_append(
    messages: &mut Vec<ChatMessage>,
    role: Role,
    content: MessageContent,
    checkpoint_id: Option<String>,
) {
    // check if the last message is user role and if so add assistant message with the checkpoint_id if exists
    if let Some(last_msg) = messages.last()
        && last_msg.role == Role::User
        && let Some(checkpoint_id) = checkpoint_id
    {
        messages.push(assistant_msg_with_checkpoint_id(&checkpoint_id));
    }

    messages.push(ChatMessage {
        role,
        content: Some(content),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
    });
}

fn clean_state_from_tool_failures(messages: &mut Vec<ChatMessage>, tool_ids: &[String]) {
    messages.retain(|msg| {
        // Remove tool calls that match the IDs
        if let Some(tool_calls) = &msg.tool_calls
            && tool_calls.iter().any(|tc| tool_ids.contains(&tc.id))
        {
            return false;
        }
        // Remove tool results (messages with role Tool) that match the IDs
        if msg.role == Role::Tool
            && let Some(tool_call_id) = &msg.tool_call_id
            && tool_ids.contains(tool_call_id)
        {
            return false;
        }
        true
    });
}
