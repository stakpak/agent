use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use tools::Tools;

pub mod tools;
/// npx @modelcontextprotocol/inspector cargo run mcp
pub async fn start_server() -> Result<()> {
    // Create an instance of our counter router
    let service = Tools::new().serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.waiting().await?;
    Ok(())
}
