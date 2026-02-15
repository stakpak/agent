use anyhow::Result;
use rmcp::{
    ServiceExt,
    transport::{
        stdio,
        streamable_http_server::{StreamableHttpService, session::local::LocalSessionManager},
    },
};
use std::hash::Hash;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::broadcast::Receiver};
pub use tool_container::ToolContainer;
use tracing::error;

use stakpak_api::AgentProvider;
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::task_manager::{TaskManager, TaskManagerHandle};

pub mod integrations;
pub mod local_tools;
pub mod remote_tools;
pub mod subagent_tools;
pub mod tool_container;

pub mod tool_names {
    pub const VIEW: &str = "view";
    pub const CREATE: &str = "create";
    pub const STR_REPLACE: &str = "str_replace";
    pub const CREATE_FILE: &str = "create_file";
    pub const EDIT_FILE: &str = "edit_file";
    pub const RUN_COMMAND: &str = "run_command";
    pub const SEARCH_DOCS: &str = "search_docs";
    pub const READ_RULEBOOK: &str = "read_rulebook";
    pub const LOCAL_CODE_SEARCH: &str = "local_code_search";
    pub const DELETE_FILE: &str = "delete_file";

    const FS_FILE_READ: &[&str] = &[VIEW];
    const FS_FILE_WRITE: &[&str] = &[CREATE, CREATE_FILE, STR_REPLACE, EDIT_FILE];
    pub const AUTO_APPROVED: &[&str] = &[VIEW, SEARCH_DOCS, READ_RULEBOOK, LOCAL_CODE_SEARCH];

    pub fn is_fs_file_read(name: &str) -> bool {
        FS_FILE_READ.contains(&name)
    }

    pub fn is_fs_file_write(name: &str) -> bool {
        FS_FILE_WRITE.contains(&name)
    }

    pub fn is_fs_tool(name: &str) -> bool {
        is_fs_file_read(name) || is_fs_file_write(name)
    }

    pub fn is_auto_approved(name: &str) -> bool {
        AUTO_APPROVED.contains(&name)
    }
}

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

/// Configuration inherited by subagents so they use the same profile and config as the parent.
#[derive(Clone, Debug, Default)]
pub struct SubagentConfig {
    pub profile_name: Option<String>,
    pub config_path: Option<String>,
}

pub struct MCPServerConfig {
    pub client: Option<Arc<dyn AgentProvider>>,
    pub bind_address: String,
    pub redact_secrets: bool,
    pub privacy_mode: bool,
    pub enabled_tools: EnabledToolsConfig,
    pub tool_mode: ToolMode,
    pub enable_subagents: bool,
    pub certificate_chain: Arc<Option<CertificateChain>>,
    pub subagent_config: SubagentConfig,
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

fn build_tool_container(
    config: &MCPServerConfig,
    task_manager_handle: Arc<TaskManagerHandle>,
) -> Result<ToolContainer> {
    let tool_container = match config.tool_mode {
        ToolMode::LocalOnly => {
            let mut tool_router = ToolContainer::tool_router_local();

            if config.enable_subagents {
                tool_router += ToolContainer::tool_router_subagent();
            }

            ToolContainer::new(
                None,
                config.redact_secrets,
                config.privacy_mode,
                config.enabled_tools.clone(),
                task_manager_handle.clone(),
                tool_router,
                config.subagent_config.clone(),
            )
        }
        ToolMode::RemoteOnly => {
            let mut tool_router = ToolContainer::tool_router_remote();
            if config.enabled_tools.slack {
                tool_router += ToolContainer::tool_router_slack();
            }

            if config.enable_subagents {
                tool_router += ToolContainer::tool_router_subagent();
            }

            ToolContainer::new(
                config.client.clone(),
                config.redact_secrets,
                config.privacy_mode,
                config.enabled_tools.clone(),
                task_manager_handle.clone(),
                tool_router,
                config.subagent_config.clone(),
            )
        }
        ToolMode::Combined => {
            let mut tool_router =
                ToolContainer::tool_router_local() + ToolContainer::tool_router_remote();

            if config.enabled_tools.slack {
                tool_router += ToolContainer::tool_router_slack();
            }

            if config.enable_subagents {
                tool_router += ToolContainer::tool_router_subagent();
            }

            ToolContainer::new(
                config.client.clone(),
                config.redact_secrets,
                config.privacy_mode,
                config.enabled_tools.clone(),
                task_manager_handle.clone(),
                tool_router,
                config.subagent_config.clone(),
            )
        }
    }
    .map_err(|e| {
        error!("Failed to create tool container: {}", e);
        anyhow::anyhow!("Failed to create tool container: {}", e)
    })?;

    Ok(tool_container)
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

    let tool_container = build_tool_container(&config, task_manager_handle.clone())?;

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

/// Start the MCP server over stdio transport.
pub async fn start_server_stdio(
    config: MCPServerConfig,
    shutdown_rx: Option<Receiver<()>>,
) -> Result<()> {
    init_gitleaks_if_needed(config.redact_secrets, config.privacy_mode).await;

    // Create and start TaskManager
    let task_manager = TaskManager::new();
    let task_manager_handle = task_manager.handle();

    tokio::spawn(async move {
        task_manager.run().await;
    });

    let tool_container = build_tool_container(&config, task_manager_handle.clone())?;

    let running_service = tool_container.serve(stdio()).await.map_err(|e| {
        error!("Failed to start stdio MCP server: {}", e);
        anyhow::anyhow!("Failed to start stdio MCP server: {}", e)
    })?;

    let cancellation_token = running_service.cancellation_token();
    let shutdown_task_manager = task_manager_handle.clone();
    let wait_handle = tokio::spawn(async move { running_service.waiting().await });

    // Graceful shutdown: on signal or external shutdown, cancel the running service.
    tokio::spawn(async move {
        create_shutdown_handler(shutdown_rx, Some(shutdown_task_manager)).await;
        cancellation_token.cancel();
    });

    let wait_result = match wait_handle.await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(join_err)) => Err(anyhow::anyhow!(join_err)),
        Err(join_err) => Err(anyhow::anyhow!(join_err)),
    };

    if let Err(e) = task_manager_handle.shutdown().await {
        error!(
            "Failed to shutdown task manager after stdio server exit: {}",
            e
        );
    }

    wait_result
}
