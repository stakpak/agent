use crate::commands::agent::run::checkpoint::{
    get_checkpoint_messages, prepare_checkpoint_messages_and_tool_calls,
};
use serde::{Deserialize, Serialize};
use stakpak_api::AgentProvider;
use stakpak_shared::models::integrations::openai::{AgentModel, ChatMessage, MessageContent, Role};

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

pub enum RecoveryResult {
    ModelSwitched(usize),
}

pub async fn handle_revert_to_checkpoint(
    client: &dyn AgentProvider,
    checkpoint_id: &String,
    messages: &mut Vec<ChatMessage>,
) -> Result<(), String> {
    let checkpoint_messages =
        get_checkpoint_messages(client, checkpoint_id).await?;
    let (chat_messages, _) = prepare_checkpoint_messages_and_tool_calls(
        checkpoint_id,
        checkpoint_messages,
    );
    *messages = chat_messages;
    Ok(())
}

pub fn handle_truncate(messages: &mut Vec<ChatMessage>, index: usize) {
    if index < messages.len() {
        messages.truncate(index);
    }
}

pub fn handle_remove_tools(messages: &mut Vec<ChatMessage>, tool_ids: &[String]) {
    clean_state_from_tool_failures(messages, tool_ids);
}

pub fn handle_append(messages: &mut Vec<ChatMessage>, role: Role, content: MessageContent) {
    messages.push(ChatMessage {
        role,
        content: Some(content),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
    });
}

pub fn handle_change_model(current_model: &mut AgentModel, config: &ModelConfig) -> RecoveryResult {
    *current_model = AgentModel::from(config.model.clone());
    RecoveryResult::ModelSwitched(5)
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
