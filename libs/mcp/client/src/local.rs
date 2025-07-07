use anyhow::Result;
use reqwest::{
    Client,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{ClientCapabilities, ClientInfo, Implementation},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
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
    host: String,
    progress_tx: Option<Sender<ToolCallResultProgress>>,
    auth_token: Option<String>,
) -> Result<RunningService<RoleClient, LocalClientHandler>> {
    let http_client = if let Some(token) = auth_token {
        Client::builder().default_headers(HeaderMap::from_iter(vec![(
            HeaderName::from_static("authorization"),
            #[allow(clippy::expect_used)]
            HeaderValue::from_str(&format!("Bearer {}", token))
                .expect("Failed to create header value"),
        )]))
    } else {
        Client::builder()
    }
    .build()?;

    let transport = StreamableHttpClientTransport::with_client(
        http_client,
        StreamableHttpClientTransportConfig::with_uri(format!("{}/mcp", host)),
    );
    let client_handler = LocalClientHandler { progress_tx };
    let client: RunningService<RoleClient, LocalClientHandler> =
        client_handler.serve(transport).await?;

    Ok(client)
}
