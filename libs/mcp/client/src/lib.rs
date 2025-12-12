use anyhow::Result;
use rmcp::{
    RoleClient,
    model::{CallToolRequestParam, ClientRequest, Meta, Request, Tool},
    service::{PeerRequestOptions, RequestHandle, RunningService},
};
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

mod local;

pub use local::LocalClientHandler;

pub type McpClient = RunningService<RoleClient, LocalClientHandler>;

/// Connect to the MCP proxy via stdio
pub async fn connect(progress_tx: Option<Sender<ToolCallResultProgress>>) -> Result<McpClient> {
    local::connect(progress_tx).await
}

/// Get all available tools from the MCP client
pub async fn get_tools(client: &McpClient) -> Result<Vec<Tool>> {
    let tools = client.list_tools(Default::default()).await?;
    Ok(tools.tools)
}

/// Call a tool on the MCP client
pub async fn call_tool(
    client: &McpClient,
    params: CallToolRequestParam,
    session_id: Option<Uuid>,
) -> Result<RequestHandle<RoleClient>, String> {
    let mut meta_map = serde_json::Map::new();
    if let Some(session_id) = session_id {
        meta_map.insert("session_id".to_string(), serde_json::json!(session_id));
    }
    let options = PeerRequestOptions {
        meta: Some(Meta(meta_map)),
        ..Default::default()
    };
    client
        .send_cancellable_request(
            ClientRequest::CallToolRequest(Request::new(params)),
            options,
        )
        .await
        .map_err(|e| e.to_string())
}
