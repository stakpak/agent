use rmcp::model::{
    CallToolRequestParam, CallToolResult, CancelledNotification, CancelledNotificationParam,
    ServerResult,
};
use stakpak_api::AgentProvider;
use stakpak_api::models::AgentSession;
use stakpak_mcp_client::ClientManager;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_tui::SessionInfo;
use uuid::Uuid;

pub async fn list_sessions(client: &dyn AgentProvider) -> Result<Vec<SessionInfo>, String> {
    let sessions: Vec<AgentSession> = client.list_agent_sessions().await?;

    let mut session_infos: Vec<(SessionInfo, chrono::DateTime<chrono::Utc>)> = sessions
        .into_iter()
        .map(|s| {
            let mut checkpoints = s.checkpoints.clone();
            checkpoints.sort_by_key(|c| c.updated_at);

            // Get the last checkpoint's updated_at for sorting
            let last_checkpoint_timestamp = checkpoints
                .last()
                .map(|c| c.updated_at)
                .unwrap_or(s.updated_at);

            let session_info = SessionInfo {
                id: s.id.to_string(),
                title: s.title,
                updated_at: s.updated_at.to_string(),
                checkpoints: checkpoints.iter().map(|c| c.id.to_string()).collect(),
            };

            (session_info, last_checkpoint_timestamp)
        })
        .collect();

    session_infos.sort_by_key(|(_, timestamp)| *timestamp);
    session_infos.reverse(); // Reverse to get most recent sessions at the top

    let session_infos: Vec<SessionInfo> = session_infos
        .into_iter()
        .map(|(mut session_info, last_checkpoint_timestamp)| {
            session_info.updated_at = last_checkpoint_timestamp.to_string();
            session_info
        })
        .collect();
    Ok(session_infos)
}

pub async fn run_tool_call(
    client_manager: &ClientManager,
    tools_map: &std::collections::HashMap<String, Vec<rmcp::model::Tool>>,
    tool_call: &ToolCall,
    cancel_rx: Option<tokio::sync::broadcast::Receiver<()>>,
    session_id: Option<Uuid>,
) -> Result<Option<CallToolResult>, String> {
    let tool_name = &tool_call.function.name;
    let client_name = tools_map
        .iter()
        .find(|(_, tools)| tools.iter().any(|tool| tool.name == *tool_name))
        .map(|(name, _)| name.clone());

    if let Some(client_name) = client_name {
        let handle = client_manager
            .call_tool(
                &client_name,
                CallToolRequestParam {
                    name: tool_name.clone().into(),
                    arguments: Some(
                        serde_json::from_str(&tool_call.function.arguments)
                            .map_err(|e| e.to_string())?,
                    ),
                },
                session_id,
            )
            .await?;

        let peer_for_cancel = handle.peer.clone();
        let request_id = handle.id.clone();

        if let Some(mut cancel_rx) = cancel_rx {
            tokio::select! {
                result = handle.await_response() => {
                    let result = match result.map_err(|e| e.to_string())? {
                        ServerResult::CallToolResult(result) => result,
                        _ => return Err("Unexpected response".to_string()),
                    };
                    return Ok(Some(result))
                },
                _ = cancel_rx.recv() => {
                    let notification = CancelledNotification {
                        params: CancelledNotificationParam {
                            request_id,
                            reason: Some("user cancel".to_string()),
                        },
                        method: rmcp::model::CancelledNotificationMethod,
                        extensions: Default::default(),
                    };
                    let _ = peer_for_cancel.send_notification(notification.into()).await;
                    return Ok(Some(CallToolResult::cancel(None)));
                }
            }
        } else {
            let result = match handle.await_response().await.map_err(|e| e.to_string())? {
                ServerResult::CallToolResult(result) => result,
                _ => return Err("Unexpected response".to_string()),
            };
            return Ok(Some(result));
        }
    }

    Ok(None)
}
