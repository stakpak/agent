use crate::commands::agent::code::tui::send_input_event;
use stakpak_api::Client;
use stakpak_api::models::AgentOutput;
use stakpak_shared::models::integrations::openai::{
    ChatMessage, MessageContent, Role, ToolCallResult,
};
use stakpak_tui::InputEvent;
use uuid::Uuid;

pub async fn get_checkpoint_messages(
    client: &Client,
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
    if let AgentOutput::PabloV1 { messages, .. } = output {
        return messages.clone();
    }
    vec![]
}

pub async fn process_checkpoint_messages(
    checkpoint_id: &String,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    messages: Vec<ChatMessage>,
) -> Result<Vec<ChatMessage>, String> {
    let mut checkpoint_messages = messages.clone();
    // Append checkpoint_id to the last assistant message if present
    if let Some(last_message) = checkpoint_messages
        .iter_mut()
        .rev()
        .find(|message| message.role != Role::User && message.role != Role::Tool)
    {
        if last_message.role == Role::Assistant {
            last_message.content = Some(MessageContent::String(format!(
                "{}\n<checkpoint_id>{}</checkpoint_id>",
                last_message
                    .content
                    .as_ref()
                    .unwrap_or(&MessageContent::String(String::new())),
                checkpoint_id
            )));
        }
    }

    for message in &*checkpoint_messages {
        match message.role {
            Role::Assistant | Role::User => {
                if let Some(content) = &message.content {
                    let _ = input_tx
                        .send(InputEvent::InputSubmittedWith(content.to_string()))
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
                        }),
                    )
                    .await;
                }
            }
            _ => {}
        }
    }

    // NOTE: tools_queue logic is handled in the main run loop, not here.
    Ok(checkpoint_messages.clone())
}
