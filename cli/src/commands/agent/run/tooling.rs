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
        // Parse arguments safely
        let arguments = match serde_json::from_str(&tool_call.function.arguments) {
            Ok(args) => Some(args),
            Err(e) => {
                let error_msg = format!("Failed to parse tool arguments as JSON: {}", e);
                log::error!("{}", error_msg);
                return Ok(Some(CallToolResult::error(vec![
                    rmcp::model::Content::text("INVALID_ARGUMENTS"),
                    rmcp::model::Content::text(error_msg),
                ])));
            }
        };

        // Call tool and handle errors gracefully
        let handle = match client_manager
            .call_tool(
                &client_name,
                CallToolRequestParam {
                    name: tool_name.clone().into(),
                    arguments,
                },
                session_id,
            )
            .await
        {
            Ok(handle) => handle,
            Err(e) => {
                let error_msg = format!("Failed to call MCP tool '{}': {}", tool_name, e);
                log::error!("{}", error_msg);
                return Ok(Some(CallToolResult::error(vec![
                    rmcp::model::Content::text("MCP_TOOL_CALL_ERROR"),
                    rmcp::model::Content::text(error_msg),
                ])));
            }
        };

        let peer_for_cancel = handle.peer.clone();
        let request_id = handle.id.clone();

        if let Some(mut cancel_rx) = cancel_rx {
            tokio::select! {
                result = handle.await_response() => {
                    match result {
                        Ok(server_result) => {
                            eprintln!("{:?}", server_result);
                            match server_result {
                                ServerResult::CallToolResult(result) => {
                                    return Ok(Some(result));
                                },
                                _ => {
                                    let error_msg = "Unexpected response type from MCP server".to_string();
                                    log::error!("{}", error_msg);
                                    return Ok(Some(CallToolResult::error(vec![
                                        rmcp::model::Content::text("UNEXPECTED_RESPONSE"),
                                        rmcp::model::Content::text(error_msg),
                                    ])));
                                }
                            }
                        },
                        Err(e) => {
                            let error_msg = format!("MCP tool execution error: {}", e);
                            log::error!("{}", error_msg);
                            // Return error result instead of panicking
                            return Ok(Some(CallToolResult::error(vec![
                                rmcp::model::Content::text("MCP_ERROR"),
                                rmcp::model::Content::text(error_msg),
                            ])));
                        }
                    }
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
            match handle.await_response().await {
                Ok(server_result) => match server_result {
                    ServerResult::CallToolResult(result) => {
                        return Ok(Some(result));
                    }
                    _ => {
                        let error_msg = "Unexpected response type from MCP server".to_string();
                        log::error!("{}", error_msg);
                        return Ok(Some(CallToolResult::error(vec![
                            rmcp::model::Content::text("UNEXPECTED_RESPONSE"),
                            rmcp::model::Content::text(error_msg),
                        ])));
                    }
                },
                Err(e) => {
                    let error_msg = format!("MCP tool execution error: {}", e);
                    log::error!("{}", error_msg);
                    // Return error result instead of panicking
                    return Ok(Some(CallToolResult::error(vec![
                        rmcp::model::Content::text("MCP_ERROR"),
                        rmcp::model::Content::text(error_msg),
                    ])));
                }
            }
        }
    }

    Ok(None)
}
