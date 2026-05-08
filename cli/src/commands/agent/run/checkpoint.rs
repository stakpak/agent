use crate::commands::agent::run::tui::send_input_event;
use crate::utils::agent_context::strip_injected_context_blocks;
use stakai::{ContentPart, Message, MessageContent, Role, ToolCall};
use stakpak_api::AgentProvider;
use stakpak_shared::models::agent_runtime::{ToolCallResult, ToolCallResultStatus};
use stakpak_tui::{InputEvent, LoadingOperation};
use uuid::Uuid;

pub async fn get_checkpoint_messages(
    client: &dyn AgentProvider,
    checkpoint_id: &str,
) -> Result<(Vec<Message>, Option<serde_json::Value>), String> {
    let checkpoint_uuid = Uuid::parse_str(checkpoint_id).map_err(|_| {
        format!(
            "Invalid checkpoint ID '{}' - must be a valid UUID",
            checkpoint_id
        )
    })?;

    let checkpoint = client
        .get_checkpoint(checkpoint_uuid)
        .await
        .map_err(|e| e.to_string())?;

    Ok((checkpoint.state.messages, checkpoint.state.metadata))
}

pub async fn extract_checkpoint_messages_and_tool_calls(
    checkpoint_id: &str,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
    messages: Vec<Message>,
) -> Result<(Vec<Message>, Vec<ToolCall>), String> {
    let mut checkpoint_messages = messages;
    // Append checkpoint_id to the last assistant message if present
    if let Some(last_message) = checkpoint_messages
        .iter_mut()
        .rev()
        .find(|message| message.role != Role::User && message.role != Role::Tool)
        && last_message.role == Role::Assistant
    {
        last_message.content = MessageContent::Text(format!(
            "{}\n<checkpoint_id>{}</checkpoint_id>",
            last_message.content.text().unwrap_or_default(),
            checkpoint_id
        ));
    }

    for message in &checkpoint_messages {
        match message.role {
            Role::Assistant => {
                if let Some(content) = message.content.text() {
                    let _ = input_tx
                        .send(InputEvent::StreamAssistantMessage(Uuid::new_v4(), content))
                        .await;
                }
            }
            Role::User => {
                if let Some(content) = message.content.text() {
                    let content = strip_injected_context_blocks(&content);
                    let _ = input_tx.send(InputEvent::AddUserMessage(content)).await;
                }
            }
            Role::Tool => {
                for part in message.parts() {
                    let ContentPart::ToolResult {
                        tool_call_id,
                        content,
                        ..
                    } = part
                    else {
                        continue;
                    };
                    let tool_call = checkpoint_messages
                        .iter()
                        .flat_map(|checkpoint_message| checkpoint_message.parts())
                        .find_map(|part| match part {
                            ContentPart::ToolCall {
                                id,
                                name,
                                arguments,
                                metadata,
                                ..
                            } if id == tool_call_id => Some(ToolCall {
                                id,
                                name,
                                arguments,
                                metadata,
                            }),
                            _ => None,
                        });
                    let Some(tool_call) = tool_call else {
                        continue;
                    };
                    let _ = send_input_event(
                        input_tx,
                        InputEvent::ToolResult(ToolCallResult {
                            call: tool_call,
                            result: content.to_string(),
                            status: ToolCallResultStatus::Success,
                        }),
                    )
                    .await;
                }
            }
            _ => {}
        }
    }

    // Find the last assistant message that has tool_calls
    let tool_calls = checkpoint_messages
        .iter()
        .rev()
        .find(|msg| {
            msg.role == Role::Assistant
                && msg
                    .parts()
                    .iter()
                    .any(|part| matches!(part, ContentPart::ToolCall { .. }))
        })
        .map(|msg| {
            msg.parts()
                .into_iter()
                .filter_map(|part| match part {
                    ContentPart::ToolCall {
                        id,
                        name,
                        arguments,
                        metadata,
                        ..
                    } => Some(ToolCall {
                        id,
                        name,
                        arguments,
                        metadata,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });

    // Filter out tool calls that already have results (Role::Tool messages)
    let executed_tool_ids: std::collections::HashSet<String> = checkpoint_messages
        .iter()
        .filter(|msg| msg.role == Role::Tool)
        .flat_map(|msg| msg.parts())
        .filter_map(|part| match part {
            ContentPart::ToolResult { tool_call_id, .. } => Some(tool_call_id),
            _ => None,
        })
        .collect();

    let pending_tool_calls: Vec<ToolCall> = tool_calls
        .map(|tcs| {
            tcs.into_iter()
                .filter(|tc| !executed_tool_ids.contains(&tc.id))
                .collect()
        })
        .unwrap_or_default();

    Ok((checkpoint_messages, pending_tool_calls))
}

pub fn extract_checkpoint_id_from_messages(messages: &[Message]) -> Option<String> {
    messages
        .last()
        .and_then(|msg| msg.content.text())
        .and_then(|text| {
            let start = text.find("<checkpoint_id>")?;
            let end = text.find("</checkpoint_id>")?;
            let start_pos = start + "<checkpoint_id>".len();
            Some(text[start_pos..end].to_string())
        })
}

/// Resumes a session from a checkpoint, loading messages and tool calls
pub async fn resume_session_from_checkpoint(
    client: &dyn AgentProvider,
    session_id: &str,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<(Vec<Message>, Vec<ToolCall>, Uuid, Option<serde_json::Value>), String> {
    let session_uuid = Uuid::parse_str(session_id).map_err(|e| e.to_string())?;

    match client.get_active_checkpoint(session_uuid).await {
        Ok(checkpoint) => {
            let metadata = checkpoint.state.metadata.clone();
            let (chat_messages, tool_calls) = extract_checkpoint_messages_and_tool_calls(
                &checkpoint.id.to_string(),
                input_tx,
                checkpoint.state.messages,
            )
            .await?;

            Ok((chat_messages, tool_calls, checkpoint.session_id, metadata))
        }
        Err(e) => {
            send_input_event(
                input_tx,
                InputEvent::EndLoadingOperation(LoadingOperation::CheckpointResume),
            )
            .await?;
            send_input_event(input_tx, InputEvent::Error(e.to_string())).await?;
            Err("Failed to get session checkpoint".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn extract_checkpoint_messages_replays_user_text_without_injected_context_blocks() {
        let (input_tx, mut input_rx) = mpsc::channel(8);
        let messages = vec![Message::new(
            Role::User,
            "fix tests\n<available_skills>\n# Available Skills:\n- very long\n</available_skills>\n<agents_md>\nrepo instructions\n</agents_md>"
                .to_string(),
        )];

        let (checkpoint_messages, pending_tool_calls) = extract_checkpoint_messages_and_tool_calls(
            "checkpoint-id",
            &input_tx,
            messages.clone(),
        )
        .await
        .expect("checkpoint extraction");

        assert_eq!(checkpoint_messages.len(), 1);
        assert!(
            checkpoint_messages[0]
                .content
                .text()
                .expect("checkpoint text")
                .contains("<available_skills>")
        );
        assert!(pending_tool_calls.is_empty());

        match input_rx.recv().await.expect("input event") {
            InputEvent::AddUserMessage(content) => assert_eq!(content, "fix tests"),
            event => panic!("unexpected input event: {event:?}"),
        }
    }
}
