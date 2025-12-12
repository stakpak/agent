use anyhow::Result;
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{ClientCapabilities, ClientInfo, Implementation},
    service::RunningService,
    transport::TokioChildProcess,
};
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;

#[derive(Clone)]
pub struct LocalClientHandler {
    progress_tx: Option<Sender<ToolCallResultProgress>>,
}

impl ClientHandler for LocalClientHandler {
    async fn on_progress(
        &self,
        progress: rmcp::model::ProgressNotificationParam,
        _ctx: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        if let Some(progress_tx) = self.progress_tx.clone()
            && let Some(message) = progress.message
        {
            match serde_json::from_str::<ToolCallResultProgress>(&message) {
                Ok(tool_call_progress) => {
                    let _ = progress_tx.send(tool_call_progress).await;
                }
                Err(e) => {
                    tracing::warn!("Failed to deserialize ToolCallProgress: {}", e);
                }
            }
        }
    }

    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "stakpak-mcp-client".to_string(),
                version: "0.0.1".to_string(),
                title: Some("Stakpak MCP Client".to_string()),
                icons: Some(vec![]),
                website_url: Some("https://stakpak.dev".to_string()),
            },
        }
    }
}

pub async fn connect(
    progress_tx: Option<Sender<ToolCallResultProgress>>,
) -> Result<RunningService<RoleClient, LocalClientHandler>> {
    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("--").arg("mcp").arg("proxy");

    let proc = TokioChildProcess::new(cmd)?;
    let client_handler = LocalClientHandler { progress_tx };
    let client: RunningService<RoleClient, LocalClientHandler> = client_handler.serve(proc).await?;

    Ok(client)
}
