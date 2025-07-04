use anyhow::Result;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

use stakpak_api::ClientConfig;

pub mod local_tools;
pub mod remote_tools;
pub mod secret_manager;
pub mod tool_container;

pub use tool_container::ToolContainer;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ToolMode {
    /// Only local tools (no API key required)
    LocalOnly,
    /// Only remote tools (requires API key)
    RemoteOnly,
    /// Both local and remote tools (requires API key)
    Combined,
}

impl std::fmt::Display for ToolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ToolMode::LocalOnly => "local",
            ToolMode::RemoteOnly => "remote",
            ToolMode::Combined => "combined",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for ToolMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local" => Ok(ToolMode::LocalOnly),
            "remote" => Ok(ToolMode::RemoteOnly),
            "combined" => Ok(ToolMode::Combined),
            _ => Err(format!("Invalid tool mode: {}", s)),
        }
    }
}

pub struct MCPServerConfig {
    pub api: ClientConfig,
    pub bind_address: String,
    pub redact_secrets: bool,
    pub tool_mode: ToolMode,
}

/// Initialize gitleaks configuration if secret redaction is enabled
async fn init_gitleaks_if_needed(redact_secrets: bool) {
    if redact_secrets {
        tokio::spawn(async {
            match std::panic::catch_unwind(stakpak_shared::secrets::initialize_gitleaks_config) {
                Ok(_rule_count) => {}
                Err(_) => {
                    // Failed to initialize, will initialize on first use
                }
            }
        });
    }
}

// /// Initialize code index in background if needed for remote/combined modes
// async fn init_code_index_if_needed(tool_mode: &ToolMode, api_config: &ClientConfig) {
//     match tool_mode {
//         ToolMode::RemoteOnly | ToolMode::Combined => {
//             let api_config = api_config.clone();
//             tokio::spawn(async move {
//                 match get_or_build_local_code_index(&api_config, None).await {
//                     Ok(_) => if let Ok(_handle) = start_code_index_watcher(&api_config, None) {},
//                     Err(_) => {
//                         // Index will be built on first use if initialization fails
//                     }
//                 }
//             });
//         }
//         ToolMode::LocalOnly => {
//             // No code index needed for local-only mode
//         }
//     }
// }

/// Create graceful shutdown handler
async fn create_shutdown_handler(shutdown_rx: Option<tokio::sync::broadcast::Receiver<()>>) {
    if let Some(mut shutdown_rx) = shutdown_rx {
        let _ = shutdown_rx.recv().await;
    } else {
        tracing::info!("Setting up signal handlers for graceful shutdown");

        // Handle both SIGINT (Ctrl+C) and SIGTERM
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};

            let mut sigint = match signal(SignalKind::interrupt()) {
                Ok(signal) => signal,
                Err(_) => {
                    // Fall back to basic ctrl_c handler
                    match tokio::signal::ctrl_c().await {
                        Ok(()) => {
                            return;
                        }
                        Err(_) => {
                            tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;
                            return;
                        }
                    }
                }
            };

            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(_) => {
                    // Continue with just SIGINT
                    let _ = sigint.recv().await;
                    return;
                }
            };

            tokio::select! {
                _ = sigint.recv() => {
                }
                _ = sigterm.recv() => {
                }
            }
        }

        #[cfg(not(unix))]
        {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    tracing::info!("Received Ctrl+C signal, shutting down gracefully");
                }
                Err(e) => {
                    tracing::error!("Failed to listen for Ctrl+C signal: {}", e);
                    // Fall back to waiting indefinitely if signal handling fails
                    tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;
                }
            }
        }
    }
}

/// npx @modelcontextprotocol/inspector cargo run mcp
pub async fn start_server(
    config: MCPServerConfig,
    shutdown_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<()> {
    init_gitleaks_if_needed(config.redact_secrets).await;

    match config.tool_mode {
        ToolMode::LocalOnly => {
            let service = StreamableHttpService::new(
                move || {
                    Ok(ToolContainer::new(
                        config.api.clone(),
                        config.redact_secrets,
                        ToolContainer::tool_router_local(),
                    ))
                },
                LocalSessionManager::default().into(),
                Default::default(),
            );
            let router = axum::Router::new().nest_service("/mcp", service);
            let tcp_listener = tokio::net::TcpListener::bind(config.bind_address).await?;
            axum::serve(tcp_listener, router)
                .with_graceful_shutdown(create_shutdown_handler(shutdown_rx))
                .await?;
        }
        ToolMode::RemoteOnly => {
            let service = StreamableHttpService::new(
                move || {
                    Ok(ToolContainer::new(
                        config.api.clone(),
                        config.redact_secrets,
                        ToolContainer::tool_router_remote(),
                    ))
                },
                LocalSessionManager::default().into(),
                Default::default(),
            );
            let router = axum::Router::new().nest_service("/mcp", service);
            let tcp_listener = tokio::net::TcpListener::bind(config.bind_address).await?;
            axum::serve(tcp_listener, router)
                .with_graceful_shutdown(create_shutdown_handler(shutdown_rx))
                .await?;
        }
        ToolMode::Combined => {
            let service = StreamableHttpService::new(
                move || {
                    Ok(ToolContainer::new(
                        config.api.clone(),
                        config.redact_secrets,
                        ToolContainer::tool_router_local() + ToolContainer::tool_router_remote(),
                    ))
                },
                LocalSessionManager::default().into(),
                Default::default(),
            );
            let router = axum::Router::new().nest_service("/mcp", service);
            let tcp_listener = tokio::net::TcpListener::bind(config.bind_address).await?;
            axum::serve(tcp_listener, router)
                .with_graceful_shutdown(create_shutdown_handler(shutdown_rx))
                .await?;
        }
    }

    Ok(())
}

/// Start server with local tools only (no API key required)
pub async fn start_local_server(
    bind_address: String,
    redact_secrets: bool,
    shutdown_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<()> {
    start_server(
        MCPServerConfig {
            api: ClientConfig {
                api_key: None,
                api_endpoint: "".to_string(),
            },
            bind_address,
            redact_secrets,
            tool_mode: ToolMode::LocalOnly,
        },
        shutdown_rx,
    )
    .await
}

/// Start server with remote tools only (requires API key)
pub async fn start_remote_server(
    api_config: ClientConfig,
    bind_address: String,
    redact_secrets: bool,
    shutdown_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<()> {
    start_server(
        MCPServerConfig {
            api: api_config,
            bind_address,
            redact_secrets,
            tool_mode: ToolMode::RemoteOnly,
        },
        shutdown_rx,
    )
    .await
}

/// Start server with combined tools (requires API key)
pub async fn start_combined_server(
    api_config: ClientConfig,
    bind_address: String,
    redact_secrets: bool,
    shutdown_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<()> {
    start_server(
        MCPServerConfig {
            api: api_config,
            bind_address,
            redact_secrets,
            tool_mode: ToolMode::Combined,
        },
        shutdown_rx,
    )
    .await
}
