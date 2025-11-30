use crate::commands::agent::run::tui::send_input_event;
use rmcp::model::CallToolResult;
use stakpak_api::AgentProvider;
use stakpak_api::models::AgentOutput;
use stakpak_shared::models::integrations::{
    mcp::CallToolResultExt,
    openai::{ChatMessage, MessageContent, Role, ToolCall, ToolCallResult},
};
use stakpak_tui::{InputEvent, LoadingOperation};
use uuid::Uuid;

pub async fn get_checkpoint_messages(
    client: &dyn AgentProvider,
    checkpoint_id: &String,
) -> Result<Vec<ChatMessage>, String> {
    let checkpoint_uuid = Uuid::parse_str(checkpoint_id).map_err(|_| {
        format!(
            "Invalid checkpoint ID '{}' - must be a valid UUID",
            checkpoint_id
        )
    })?;

    let checkpoint = client
        .get_agent_checkpoint(checkpoint_uuid)
        .await
        .map_err(|e| e.to_string())?;
    let checkpoint_output: AgentOutput = checkpoint.output;

    Ok(get_messages_from_checkpoint_output(&checkpoint_output))
}

pub fn get_messages_from_checkpoint_output(output: &AgentOutput) -> Vec<ChatMessage> {
    let AgentOutput::PabloV1 { messages, .. } = output;

    messages.clone()
}

pub async fn extract_checkpoint_messages_and_tool_calls(
    checkpoint_id: &String,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    messages: Vec<ChatMessage>,
) -> Result<(Vec<ChatMessage>, Vec<ToolCall>), String> {
    let mut checkpoint_messages = messages.clone();
    // Append checkpoint_id to the last assistant message if present
    if let Some(last_message) = checkpoint_messages
        .iter_mut()
        .rev()
        .find(|message| message.role != Role::User && message.role != Role::Tool)
        && last_message.role == Role::Assistant
    {
        last_message.content = Some(MessageContent::String(format!(
            "{}\n<checkpoint_id>{}</checkpoint_id>",
            last_message
                .content
                .as_ref()
                .unwrap_or(&MessageContent::String(String::new())),
            checkpoint_id
        )));
    }

    for message in &*checkpoint_messages {
        match message.role {
            Role::Assistant => {
                if let Some(content) = &message.content {
                    let _ = input_tx
                        .send(InputEvent::StreamAssistantMessage(
                            Uuid::new_v4(),
                            content.to_string(),
                        ))
                        .await;
                }
            }
            Role::User => {
                if let Some(content) = &message.content {
                    let _ = input_tx
                        .send(InputEvent::AddUserMessage(content.to_string()))
                        .await;
                }
            }
            Role::Tool => {
                let tool_call = checkpoint_messages
                    .iter()
                    .find(|checkpoint_message| {
                        checkpoint_message
                            .tool_calls
                            .as_ref()
                            .is_some_and(|tool_calls| {
                                message.tool_call_id.as_ref().is_some_and(|tool_call_id| {
                                    tool_calls
                                        .iter()
                                        .any(|tool_call| tool_call.id == *tool_call_id)
                                })
                            })
                    })
                    .and_then(|chat_message| {
                        chat_message.tool_calls.as_ref().and_then(|tool_calls| {
                            message.tool_call_id.as_ref().and_then(|tool_call_id| {
                                tool_calls
                                    .iter()
                                    .find(|tool_call| tool_call.id == *tool_call_id)
                            })
                        })
                    });

                if let Some(tool_call) = tool_call {
                    let _ = send_input_event(
                        input_tx,
                        InputEvent::ToolResult(ToolCallResult {
                            call: tool_call.clone(),
                            result: message
                                .content
                                .as_ref()
                                .unwrap_or(&MessageContent::String(String::new()))
                                .to_string(),
                            status: CallToolResult::get_status_from_chat_message(message),
                        }),
                    )
                    .await;
                }
            }
            _ => {}
        }
    }

    let tool_calls = checkpoint_messages
        .last()
        .filter(|msg| msg.role == Role::Assistant)
        .and_then(|msg| msg.tool_calls.as_ref());

    Ok((
        checkpoint_messages.clone(),
        tool_calls.map(|t| t.to_vec()).unwrap_or_default(),
    ))
}

pub fn prepare_checkpoint_messages_and_tool_calls(
    checkpoint_id: &String,
    messages: Vec<ChatMessage>,
) -> (Vec<ChatMessage>, Vec<ToolCall>) {
    let mut checkpoint_messages = messages;

    // Extract tool_calls before modifying messages
    let tool_calls = checkpoint_messages
        .last()
        .filter(|msg| msg.role == Role::Assistant)
        .and_then(|msg| msg.tool_calls.as_ref())
        .map(|t| t.to_vec())
        .unwrap_or_default();

    if let Ok(checkpoint_uuid) = Uuid::parse_str(checkpoint_id) {
        if let Some(last_message) = checkpoint_messages
            .iter_mut()
            .rev()
            .find(|message| message.role != Role::User && message.role != Role::Tool)
            .filter(|message| message.role == Role::Assistant)
            && let Some(content) = last_message.content.as_ref()
            && content.extract_checkpoint_id().is_none()
        {
            last_message.content = Some(content.inject_checkpoint_id(checkpoint_uuid));
        }
    } else if let Some(last_message) = checkpoint_messages
        .iter_mut()
        .rev()
        .find(|message| message.role != Role::User && message.role != Role::Tool)
        .filter(|message| message.role == Role::Assistant)
    {
        last_message.content = Some(MessageContent::String(format!(
            "{}\n<checkpoint_id>{}</checkpoint_id>",
            last_message
                .content
                .as_ref()
                .map(std::string::ToString::to_string)
                .unwrap_or_default(),
            checkpoint_id
        )));
    }

    (checkpoint_messages, tool_calls)
}

pub fn extract_checkpoint_id_from_messages(messages: &[ChatMessage]) -> Option<String> {
    messages
        .last()
        .and_then(|msg| msg.content.as_ref())
        .as_ref()
        .and_then(|content| match content {
            MessageContent::String(text) => {
                if let Some(start) = text.find("<checkpoint_id>") {
                    if let Some(end) = text.find("</checkpoint_id>") {
                        let start_pos = start + "<checkpoint_id>".len();
                        Some(text[start_pos..end].to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            MessageContent::Array(items) => {
                for item in items {
                    if let Some(text) = &item.text
                        && let Some(start) = text.find("<checkpoint_id>")
                        && let Some(end) = text.find("</checkpoint_id>")
                    {
                        let start_pos = start + "<checkpoint_id>".len();
                        return Some(text[start_pos..end].to_string());
                    }
                }
                None
            }
        })
}

/// Resumes a session from a checkpoint, loading messages and tool calls
pub async fn resume_session_from_checkpoint(
    client: &dyn AgentProvider,
    session_id: &str,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<(Vec<ChatMessage>, Vec<ToolCall>, Uuid), String> {
    let session_uuid = Uuid::parse_str(session_id).map_err(|e| e.to_string())?;

    match client
        .get_agent_session_latest_checkpoint(session_uuid)
        .await
    {
        Ok(checkpoint) => {
            let (chat_messages, tool_calls) = extract_checkpoint_messages_and_tool_calls(
                &checkpoint.checkpoint.id.to_string(),
                input_tx,
                get_messages_from_checkpoint_output(&checkpoint.output),
            )
            .await?;

            Ok((chat_messages, tool_calls, checkpoint.session.id))
        }
        Err(e) => {
            send_input_event(
                input_tx,
                InputEvent::EndLoadingOperation(LoadingOperation::CheckpointResume),
            )
            .await?;
            send_input_event(input_tx, InputEvent::Error(e)).await?;
            Err("Failed to get session checkpoint".to_string())
        }
    }
}
