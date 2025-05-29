use anyhow::Result;
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{ClientCapabilities, ClientInfo, Implementation},
    service::RunningService,
    transport::StreamableHttpClientTransport,
};
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
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
        if let Some(progress_tx) = self.progress_tx.clone() {
            if let Some(message) = progress.message {
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
    }

    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "Stakpak Client".to_string(),
                version: "0.0.1".to_string(),
            },
        }
    }
}

pub async fn local_client(
    host: Option<String>,
    progress_tx: Option<Sender<ToolCallResultProgress>>,
) -> Result<RunningService<RoleClient, LocalClientHandler>> {
    let transport = StreamableHttpClientTransport::from_uri(format!(
        "{}/mcp",
        host.unwrap_or("http://0.0.0.0:65535".to_string())
    ));

    let client_handler = LocalClientHandler { progress_tx };
    let client: RunningService<RoleClient, LocalClientHandler> =
        client_handler.serve(transport).await?;

    // let client: RunningService<RoleClient, rmcp::model::InitializeRequestParam> =
    //     client_info.serve(transport).await.inspect_err(|e| {
    //         tracing::error!("client error: {:?}", e);
    //     })?;

    Ok(client)
}
