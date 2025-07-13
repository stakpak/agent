use rmcp::model::{CallToolRequestParam, CallToolResult, ServerResult};
use stakpak_api::Client;
use stakpak_api::models::AgentSession;
use stakpak_mcp_client::ClientManager;
use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_tui::SessionInfo;
use tokio::sync::oneshot;

pub async fn list_sessions(client: &Client) -> Result<Vec<SessionInfo>, String> {
    let sessions: Vec<AgentSession> = client.list_agent_sessions().await?;
    let session_infos: Vec<SessionInfo> = sessions
        .into_iter()
        .map(|s| {
            let mut checkpoints = s.checkpoints.clone();
            checkpoints.sort_by_key(|c| c.created_at);
            SessionInfo {
                id: s.id.to_string(),
                title: s.title,
                updated_at: s.updated_at.to_string(),
                checkpoints: checkpoints.iter().map(|c| c.id.to_string()).collect(),
            }
        })
        .collect();
    Ok(session_infos)
}

pub async fn run_tool_call(
    client_manager: &ClientManager,
    tools_map: &std::collections::HashMap<String, Vec<rmcp::model::Tool>>,
    tool_call: &ToolCall,
    cancel_rx: Option<oneshot::Receiver<()>>,
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
            )
            .await?;

        if let Some(cancel_rx) = cancel_rx {
            tokio::select! {
                result = handle.await_response() => {
                    let result = match result.map_err(|e| e.to_string())? {
                        ServerResult::CallToolResult(result) => result,
                        _ => return Err("Unexpected response".to_string()),
                    };
                    return Ok(Some(result));
                }
                _ = cancel_rx => {
                    return Err("Tool call was cancelled".to_string());
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
