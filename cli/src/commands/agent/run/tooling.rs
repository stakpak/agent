use rmcp::model::{
    CallToolRequestParam, CallToolResult, CancelledNotification, CancelledNotificationParam,
    ServerResult,
};
use stakpak_api::AgentProvider;
use stakpak_api::storage::ListSessionsQuery;
use stakpak_mcp_client::McpClient;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_tui::SessionInfo;
use uuid::Uuid;

pub async fn list_sessions(client: &dyn AgentProvider) -> Result<Vec<SessionInfo>, String> {
    let result = client
        .list_sessions(&ListSessionsQuery::new())
        .await
        .map_err(|e| e.to_string())?;

    let session_infos: Vec<SessionInfo> = result
        .sessions
        .into_iter()
        .map(|s| SessionInfo {
            id: s.id.to_string(),
            title: s.title,
            updated_at: s.updated_at.to_string(),
            checkpoints: s
                .active_checkpoint_id
                .map(|id| vec![id.to_string()])
                .unwrap_or_default(),
        })
        .collect();

    Ok(session_infos)
}

pub async fn run_tool_call(
    mcp_client: &McpClient,
    tools: &[rmcp::model::Tool],
    tool_call: &ToolCall,
    cancel_rx: Option<tokio::sync::broadcast::Receiver<()>>,
    session_id: Option<Uuid>,
    model_id: Option<String>,
) -> Result<Option<CallToolResult>, String> {
    let tool_name = &tool_call.function.name;
    let tool_id = &tool_call.id;
    tracing::debug!(
        tool = %tool_name,
        tool_call_id = %tool_id,
        "tool_run_start (agent executing tool)"
    );
    let tool_exists = tools.iter().any(|tool| tool.name == *tool_name);

    if tool_exists {
        // Parse arguments safely
        let arguments = match serde_json::from_str(&tool_call.function.arguments) {
            Ok(args) => Some(args),
            Err(e) => {
                let error_msg = format!("Failed to parse tool arguments as JSON: {}", e);
                log::error!("{}", error_msg);
                tracing::debug!(
                    tool = %tool_name,
                    tool_call_id = %tool_id,
                    error = %error_msg,
                    "tool_run_failed (parse error, result still sent to client)"
                );
                return Ok(Some(CallToolResult::error(vec![
                    rmcp::model::Content::text("INVALID_ARGUMENTS"),
                    rmcp::model::Content::text(error_msg),
                ])));
            }
        };

        // Call tool and handle errors gracefully
        let metadata = Some({
            let mut meta = serde_json::Map::new();
            if let Some(session_id) = session_id {
                meta.insert(
                    "session_id".to_string(),
                    serde_json::Value::String(session_id.to_string()),
                );
            }
            if let Some(model_id) = model_id {
                meta.insert("model_id".to_string(), serde_json::Value::String(model_id));
            }
            meta
        });
        let handle = match stakpak_mcp_client::call_tool(
            mcp_client,
            CallToolRequestParam {
                name: tool_name.clone().into(),
                arguments,
            },
            metadata,
        )
        .await
        {
            Ok(handle) => handle,
            Err(e) => {
                let error_msg = format!("Failed to call MCP tool '{}': {}", tool_name, e);
                log::error!("{}", error_msg);
                tracing::debug!(
                    tool = %tool_name,
                    tool_call_id = %tool_id,
                    error = %error_msg,
                    "tool_run_failed (MCP call error, result still sent to client)"
                );
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
                            match server_result {
                                ServerResult::CallToolResult(result) => {
                                    let status = result.get_status();
                                    tracing::debug!(
                                        tool = %tool_name,
                                        tool_call_id = %tool_id,
                                        status = ?status,
                                        "tool_run_ok (result will be pushed to messages and sent to client)"
                                    );
                                    return Ok(Some(result));
                                },
                                _ => {
                                    let error_msg = "Unexpected response type from MCP server".to_string();
                                    log::error!("{}", error_msg);
                                    tracing::debug!(
                                        tool = %tool_name,
                                        tool_call_id = %tool_id,
                                        "tool_run_failed (unexpected response, result still sent to client)"
                                    );
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
                            tracing::debug!(
                                tool = %tool_name,
                                tool_call_id = %tool_id,
                                error = %error_msg,
                                "tool_run_failed (MCP error, result still sent to client)"
                            );
                            return Ok(Some(CallToolResult::error(vec![
                                rmcp::model::Content::text("MCP_ERROR"),
                                rmcp::model::Content::text(error_msg),
                            ])));
                        }
                    }
                },
                _ = cancel_rx.recv() => {
                    tracing::debug!(
                        tool = %tool_name,
                        tool_call_id = %tool_id,
                        "tool_run_cancelled (user cancel, cancel result sent to client)"
                    );
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
                        let status = result.get_status();
                        tracing::debug!(
                            tool = %tool_name,
                            tool_call_id = %tool_id,
                            status = ?status,
                            "tool_run_ok (result will be pushed to messages and sent to client)"
                        );
                        return Ok(Some(result));
                    }
                    _ => {
                        let error_msg = "Unexpected response type from MCP server".to_string();
                        log::error!("{}", error_msg);
                        tracing::debug!(
                            tool = %tool_name,
                            tool_call_id = %tool_id,
                            "tool_run_failed (unexpected response, result still sent to client)"
                        );
                        return Ok(Some(CallToolResult::error(vec![
                            rmcp::model::Content::text("UNEXPECTED_RESPONSE"),
                            rmcp::model::Content::text(error_msg),
                        ])));
                    }
                },
                Err(e) => {
                    let error_msg = format!("MCP tool execution error: {}", e);
                    log::error!("{}", error_msg);
                    tracing::debug!(
                        tool = %tool_name,
                        tool_call_id = %tool_id,
                        error = %error_msg,
                        "tool_run_failed (MCP error, result still sent to client)"
                    );
                    return Ok(Some(CallToolResult::error(vec![
                        rmcp::model::Content::text("MCP_ERROR"),
                        rmcp::model::Content::text(error_msg),
                    ])));
                }
            }
        }
    }

    tracing::debug!(
        tool = %tool_name,
        tool_call_id = %tool_id,
        "tool_run_skipped (tool not in list, no result sent)"
    );
    Ok(None)
}
