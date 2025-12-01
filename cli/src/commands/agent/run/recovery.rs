use crate::commands::agent::run::checkpoint::{
    get_checkpoint_messages, prepare_checkpoint_messages_and_tool_calls,
};
use serde::{Deserialize, Serialize};
use stakpak_api::AgentProvider;
use stakpak_api::models::{RecoveryMode, RecoveryOption};
use stakpak_shared::models::integrations::openai::{AgentModel, ChatMessage, MessageContent, Role};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOperation {
    Append,      // Append a new message (e.g., redirection message)
    Truncate,    // Truncate messages after a certain point
    RemoveTools, // Remove tool calls and their results
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAction {
    pub message_index: usize,
    pub role: Option<Role>,
    pub content: Option<MessageContent>,
    pub failed_tool_call_ids_to_remove: Option<Vec<String>>,
    pub recovery_operation: RecoveryOperation,
}

pub enum RecoveryResult {
    Applied,
    ModelSwitched(usize),
}

pub async fn handle_recovery_action(
    client: &dyn AgentProvider,
    option: &RecoveryOption,
    messages: &mut Vec<ChatMessage>,
    current_model: &mut AgentModel,
) -> Result<RecoveryResult, String> {
    // 1. Handle Revert/Checkpoint loading first if needed
    if let Some(checkpoint_id) = option.revert_to_checkpoint {
        let checkpoint_messages =
            get_checkpoint_messages(client, &checkpoint_id.to_string()).await?;
        let (chat_messages, _) = prepare_checkpoint_messages_and_tool_calls(
            &checkpoint_id.to_string(),
            checkpoint_messages,
        );
        *messages = chat_messages;
    }

    // 2. Parse and apply state edits
    if let Ok(actions) = serde_json::from_value::<Vec<RecoveryAction>>(option.state_edits.clone()) {
        for action in actions {
            match action.recovery_operation {
                RecoveryOperation::Truncate => {
                    if action.message_index < messages.len() {
                        messages.truncate(action.message_index);
                    }
                }
                RecoveryOperation::RemoveTools => {
                    if let Some(tool_ids) = &action.failed_tool_call_ids_to_remove {
                        clean_state_from_tool_failures(messages, tool_ids);
                    }
                }
                RecoveryOperation::Append => {
                    // Skip User role messages - TUI handles adding the recovery message
                    if let (Some(role), Some(content)) = (action.role, action.content)
                        && role != Role::User
                    {
                        messages.push(ChatMessage {
                            role,
                            content: Some(content),
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                            usage: None,
                        });
                    }
                }
            }
        }
    }

    // 3. Handle Model Change
    if option.mode == RecoveryMode::ModelChange {
        // Always switch to Recovery model for ModelChange mode
        let new_model = AgentModel::Recovery;
        *current_model = new_model.clone();
        return Ok(RecoveryResult::ModelSwitched(5));
    }

    Ok(RecoveryResult::Applied)
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
