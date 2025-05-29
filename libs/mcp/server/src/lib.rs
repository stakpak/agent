use anyhow::Result;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

use stakpak_api::ClientConfig;
use tools::Tools;

pub mod tools;

pub struct MCPServerConfig {
    pub api: ClientConfig,
}

const BIND_ADDRESS: &str = "0.0.0.0:65535";

/// npx @modelcontextprotocol/inspector cargo run mcp
pub async fn start_server(config: MCPServerConfig) -> Result<()> {
    // Create an instance of our counter router
    let service = StreamableHttpService::new(
        move || Tools::new(config.api.clone()),
        LocalSessionManager::default().into(),
        Default::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let tcp_listener = tokio::net::TcpListener::bind(BIND_ADDRESS).await?;
    axum::serve(tcp_listener, router).await.unwrap();
    Ok(())
}
