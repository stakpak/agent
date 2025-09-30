use anyhow::Result;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use std::hash::Hash;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::broadcast::Receiver};
pub use tool_container::ToolContainer;
use tracing::error;

use stakpak_api::ClientConfig;
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::subagent::SubagentConfigs;
use stakpak_shared::task_manager::TaskManager;

pub mod integrations;
pub mod local_tools;
pub mod remote_tools;
pub mod subagent_tools;
pub mod tool_container;

#[derive(Clone, Debug, Default)]
pub struct EnabledToolsConfig {
    pub slack: bool,
}

impl EnabledToolsConfig {
    pub fn with_slack(slack: bool) -> Self {
        Self { slack }
    }
}

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

#[derive(Clone)]
pub struct AuthConfig {
    pub token: Option<String>,
}

impl AuthConfig {
    pub async fn new(disabled: bool) -> Self {
        let token = if disabled {
            None
        } else {
            let token = stakpak_shared::utils::generate_password(64, true);
            Some(token)
        };

        Self { token }
    }
}

pub struct MCPServerConfig {
    pub api: ClientConfig,
    pub bind_address: String,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub enabled_tools: EnabledToolsConfig,
    pub tool_mode: ToolMode,
    pub subagent_configs: Option<SubagentConfigs>,
    pub certificate_chain: Arc<Option<CertificateChain>>,
}

/// Initialize gitleaks configuration if secret redaction is enabled
async fn init_gitleaks_if_needed(redact_secrets: bool, privacy_mode: bool) {
    if redact_secrets {
        tokio::spawn(async move {
            match std::panic::catch_unwind(|| {
                stakpak_shared::secrets::initialize_gitleaks_config(privacy_mode)
            }) {
                Ok(_rule_count) => {}
                Err(_) => {
                    // Failed to initialize, will initialize on first use
                }
            }
        });
    }
}

/// Create graceful shutdown handler
async fn create_shutdown_handler(
    shutdown_rx: Option<Receiver<()>>,
    task_manager_handle: Option<std::sync::Arc<stakpak_shared::task_manager::TaskManagerHandle>>,
) {
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
                    return;
                }
            }
        }
    }

    // Shutdown task manager if available
    if let Some(task_manager_handle) = task_manager_handle {
        tracing::info!("Shutting down task manager...");
        if let Err(e) = task_manager_handle.shutdown().await {
            tracing::error!("Failed to shutdown task manager: {}", e);
        } else {
            tracing::info!("Task manager shut down successfully");
        }
    }
}

/// Internal helper function that contains the common server initialization logic
async fn start_server_internal(
    config: MCPServerConfig,
    tcp_listener: TcpListener,
    shutdown_rx: Option<Receiver<()>>,
) -> Result<()> {
    init_gitleaks_if_needed(config.redact_secrets, config.privacy_mode).await;

    // Create and start TaskManager
    let task_manager = TaskManager::new();
    let task_manager_handle = task_manager.handle();

    // Spawn the task manager to run in background_manager_handle_for_
    tokio::spawn(async move {
        task_manager.run().await;
    });

    let tool_container = match config.tool_mode {
        ToolMode::LocalOnly => ToolContainer::new(
            None,
            config.redact_secrets,
            config.privacy_mode,
            config.enabled_tools.clone(),
            task_manager_handle.clone(),
            config.subagent_configs.clone(),
            ToolContainer::tool_router_local(),
        ),
        ToolMode::RemoteOnly => {
            let mut tool_router = ToolContainer::tool_router_remote();
            if config.enabled_tools.slack {
                tool_router += ToolContainer::tool_router_slack();
            }

            ToolContainer::new(
                Some(config.api),
                config.redact_secrets,
                config.privacy_mode,
                config.enabled_tools.clone(),
                task_manager_handle.clone(),
                config.subagent_configs.clone(),
                tool_router,
            )
        }
        ToolMode::Combined => {
            let mut tool_router =
                ToolContainer::tool_router_local() + ToolContainer::tool_router_remote();

            if config.enabled_tools.slack {
                tool_router += ToolContainer::tool_router_slack();
            }

            if config.subagent_configs.is_some() {
                tool_router += ToolContainer::tool_router_subagent();
            }

            ToolContainer::new(
                Some(config.api),
                config.redact_secrets,
                config.privacy_mode,
                config.enabled_tools.clone(),
                task_manager_handle.clone(),
                config.subagent_configs.clone(),
                tool_router,
            )
        }
    }
    .map_err(|e| {
        error!("Failed to create tool container: {}", e);
        anyhow::anyhow!("Failed to create tool container: {}", e)
    })?;

    let service = StreamableHttpService::new(
        move || Ok(tool_container.to_owned()),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);

    if let Some(cert_chain) = config.certificate_chain.as_ref() {
        let tls_config = cert_chain.create_server_config()?;
        let rustls_config =
            axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(tls_config));

        let handle = axum_server::Handle::new();
        let shutdown_handle = handle.clone();
        tokio::spawn(async move {
            create_shutdown_handler(shutdown_rx, Some(task_manager_handle.clone())).await;
            shutdown_handle.graceful_shutdown(None);
        });

        axum_server::from_tcp_rustls(tcp_listener.into_std()?, rustls_config)
            .handle(handle)
            .serve(router.into_make_service())
            .await?;
    } else {
        axum::serve(tcp_listener, router)
            .with_graceful_shutdown(create_shutdown_handler(
                shutdown_rx,
                Some(task_manager_handle.clone()),
            ))
            .await?;
    }

    Ok(())
}

/// npx @modelcontextprotocol/inspector cargo run mcp
pub async fn start_server(
    config: MCPServerConfig,
    tcp_listener: Option<TcpListener>,
    shutdown_rx: Option<Receiver<()>>,
) -> Result<()> {
    let tcp_listener = if let Some(tcp_listener) = tcp_listener {
        tcp_listener
    } else {
        TcpListener::bind(config.bind_address.clone()).await?
    };
    start_server_internal(config, tcp_listener, shutdown_rx).await
}
