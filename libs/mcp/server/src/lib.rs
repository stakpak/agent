use anyhow::Result;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

use stakpak_api::ClientConfig;
use tools::Tools;

pub mod tools;

pub struct MCPServerConfig {
    pub api: ClientConfig,
    pub bind_address: String,
    pub redact_secrets: bool,
}

/// npx @modelcontextprotocol/inspector cargo run mcp
pub async fn start_server(
    config: MCPServerConfig,
    shutdown_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<()> {
    if config.redact_secrets {
        // Initialize gitleaks configuration in a background task to avoid blocking server startup
        tokio::spawn(async {
            match std::panic::catch_unwind(stakpak_shared::secrets::initialize_gitleaks_config) {
                Ok(_rule_count) => {
                    // Gitleaks rules initialized successfully
                }
                Err(_) => {
                    // Failed to initialize, will initialize on first use
                }
            }
        });
    }

    // Create an instance of our counter router
    let service = StreamableHttpService::new(
        move || Tools::new(config.api.clone(), config.redact_secrets),
        LocalSessionManager::default().into(),
        Default::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let tcp_listener = tokio::net::TcpListener::bind(config.bind_address.clone()).await?;
    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async move {
            if let Some(mut shutdown_rx) = shutdown_rx {
                let _ = shutdown_rx.recv().await;
            } else {
                #[allow(clippy::unwrap_used)]
                tokio::signal::ctrl_c().await.unwrap();
            }
        })
        .await?;

    Ok(())
}
