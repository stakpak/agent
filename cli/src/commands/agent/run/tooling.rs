use rmcp::model::{
    CallToolRequestParam, CallToolResult, CancelledNotification, CancelledNotificationParam,
    Content, ServerResult,
};
use stakpak_api::AgentProvider;
use stakpak_api::storage::ListSessionsQuery;
use stakpak_mcp_client::McpClient;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
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
    tool_call: &stakai::ToolCall,
    cancel_rx: Option<tokio::sync::broadcast::Receiver<()>>,
    session_id: Option<Uuid>,
    model_id: Option<String>,
    model_provider: Option<String>,
) -> Result<Option<CallToolResult>, String> {
    let tool_name = &tool_call.name;
    let tool_exists = tools.iter().any(|tool| tool.name == *tool_name);

    if tool_exists {
        let arguments = match mcp_tool_arguments(tool_call) {
            Ok(arguments) => arguments,
            Err(error) => return Ok(Some(error)),
        };

        // Call tool and handle errors gracefully
        let metadata = Some({
            let mut meta = serde_json::Map::new();
            if let Some(session_id) = session_id {
                let session_id_str = session_id.to_string();
                // Legacy key — consumed by the local stakpak MCP server (ctx.meta.get("session_id"))
                meta.insert(
                    "session_id".to_string(),
                    serde_json::Value::String(session_id_str.clone()),
                );
                // MCP-spec-compliant reverse-DNS key — consumed by the AAP MCP server
                meta.insert(
                    "dev.stakpak/session-id".to_string(),
                    serde_json::Value::String(session_id_str),
                );
            }
            if let Some(model_id) = model_id {
                meta.insert("model_id".to_string(), serde_json::Value::String(model_id));
            }
            if let Some(model_provider) = model_provider {
                meta.insert(
                    "model_provider".to_string(),
                    serde_json::Value::String(model_provider),
                );
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

fn mcp_tool_arguments(
    tool_call: &stakai::ToolCall,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, CallToolResult> {
    match &tool_call.arguments {
        serde_json::Value::Object(arguments) => Ok(Some(arguments.clone())),
        serde_json::Value::Null => Ok(None),
        other => Err(CallToolResult::error(vec![
            Content::text("INVALID_ARGUMENTS"),
            Content::text(format!(
                "Tool '{}' arguments must be a JSON object, got {}",
                tool_call.name,
                json_type_name(other)
            )),
        ])),
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tool_arguments_rejects_string_arguments() {
        let tool_call = stakai::ToolCall {
            id: "tc_1".to_string(),
            name: "test_tool".to_string(),
            arguments: serde_json::Value::String(r#"{"path":"README.md"}"#.to_string()),
            metadata: None,
        };

        let error = mcp_tool_arguments(&tool_call).expect_err("string arguments should be invalid");
        let content: Vec<&str> = error
            .content
            .iter()
            .filter_map(|content| content.raw.as_text().map(|text| text.text.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(error.is_error, Some(true));
        assert_eq!(content.first(), Some(&"INVALID_ARGUMENTS"));
        assert!(
            content
                .get(1)
                .is_some_and(|message| message.contains("must be a JSON object"))
        );
    }
}
