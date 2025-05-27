use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use stakpak_api::ClientConfig;
use tools::Tools;

pub mod tools;

pub struct MCPServerConfig {
    pub api: ClientConfig,
}

/// npx @modelcontextprotocol/inspector cargo run mcp
pub async fn start_server(config: MCPServerConfig) -> Result<()> {
    // Create an instance of our counter router
    let service = Tools::new(config.api)
        .serve(stdio())
        .await
        .inspect_err(|e| {
            tracing::error!("serving error: {:?}", e);
        })?;

    service.waiting().await?;
    Ok(())
}
