use anyhow::Result;
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParam, ClientRequest, Meta, Request, Tool},
    service::{PeerRequestOptions, RequestHandle, RunningService},
    transport::StreamableHttpClientTransport,
    transport::streamable_http_client::StreamableHttpClientTransportConfig,
};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

mod local;

pub use local::LocalClientHandler;

pub type McpClient = RunningService<RoleClient, LocalClientHandler>;

/// Connect to the MCP proxy via stdio (legacy method)
pub async fn connect(progress_tx: Option<Sender<ToolCallResultProgress>>) -> Result<McpClient> {
    local::connect(progress_tx).await
}

/// Connect to an MCP server via HTTPS with optional mTLS
pub async fn connect_https(
    url: &str,
    certificate_chain: Option<Arc<CertificateChain>>,
    progress_tx: Option<Sender<ToolCallResultProgress>>,
) -> Result<McpClient> {
    let mut client_builder = reqwest::Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(std::time::Duration::from_secs(60));

    // Configure TLS: use mTLS cert chain if provided, otherwise use
    // platform-verified TLS so the OS CA store is trusted.
    if let Some(cert_chain) = certificate_chain {
        let tls_config = cert_chain.create_client_config()?;
        client_builder = client_builder.use_preconfigured_tls(tls_config);
    } else {
        let arc_crypto_provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        if let Ok(tls_config) = rustls::ClientConfig::builder_with_provider(arc_crypto_provider)
            .with_safe_default_protocol_versions()
            .map(|builder| {
                rustls_platform_verifier::BuilderVerifierExt::with_platform_verifier(builder)
                    .with_no_client_auth()
            })
        {
            client_builder = client_builder.use_preconfigured_tls(tls_config);
        }
    }

    let http_client = client_builder.build()?;

    let config = StreamableHttpClientTransportConfig::with_uri(url);
    let transport =
        StreamableHttpClientTransport::<reqwest::Client>::with_client(http_client, config);

    let client_handler = LocalClientHandler::new(progress_tx);
    let client: McpClient = client_handler.serve(transport).await?;

    Ok(client)
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
