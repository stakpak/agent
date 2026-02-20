//! MCP (Model Context Protocol) initialization module
//!
//! This module handles the initialization of the MCP server and proxy infrastructure
//! for the interactive agent mode. It sets up:
//! - Certificate chains for mTLS communication
//! - Local MCP server with tools
//! - Proxy server that aggregates multiple MCP servers
//! - Client connection to the proxy

use crate::commands::agent::run::helpers::convert_tools_with_filter;
use crate::commands::get_client;
use crate::config::AppConfig;
use crate::utils::network;
use stakpak_api::local::skills::default_skill_directories;
use stakpak_mcp_client::McpClient;
use stakpak_mcp_proxy::client::{ClientPoolConfig, ServerConfig};
use stakpak_mcp_proxy::server::start_proxy_server;
use stakpak_mcp_server::{
    EnabledToolsConfig, MCPServerConfig, SubagentConfig, ToolMode, start_server,
};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::integrations::openai::ToolCallResultProgress;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

/// Configuration options for MCP initialization
#[allow(dead_code)]
pub struct McpInitConfig {
    // --- server config ---
    /// Configuration for which tools are enabled
    pub enabled_tools: EnabledToolsConfig,
    /// Whether to enable mTLS for secure communication
    pub enable_mtls: bool,
    /// Whether to enable subagent tools
    pub enable_subagents: bool,
    /// Optional list of allowed tool names (filters tools if specified)
    pub allowed_tools: Option<Vec<String>>,
    /// Configuration inherited by subagents (profile, config path)
    pub subagent_config: SubagentConfig,

    // --- proxy config (secret redaction is handled exclusively by the proxy) ---
    /// Whether to redact secrets in tool responses
    pub redact_secrets: bool,
    /// Whether to enable privacy mode (redact IPs, account IDs, etc.)
    pub privacy_mode: bool,
}

impl Default for McpInitConfig {
    fn default() -> Self {
        Self {
            enabled_tools: EnabledToolsConfig { slack: false },
            enable_mtls: true,
            enable_subagents: true,
            allowed_tools: None,
            subagent_config: SubagentConfig::default(),
            redact_secrets: true,
            privacy_mode: false,
        }
    }
}

/// Result of MCP initialization containing all necessary handles and tools
pub struct McpInitResult {
    /// The MCP client connected to the proxy
    pub client: Arc<McpClient>,
    /// Raw MCP tools from the server
    pub mcp_tools: Vec<rmcp::model::Tool>,
    /// Converted tools for OpenAI format
    pub tools: Vec<stakpak_shared::models::integrations::openai::Tool>,
    /// Shutdown handle for the MCP server
    pub server_shutdown_tx: broadcast::Sender<()>,
    /// Shutdown handle for the proxy server
    pub proxy_shutdown_tx: broadcast::Sender<()>,
}

/// Certificate chains for server and proxy communication
struct CertificateChains {
    /// Certificate chain for MCP server <-> Proxy communication
    server_chain: Arc<Option<CertificateChain>>,
    /// Certificate chain for Proxy <-> Client communication
    proxy_chain: Arc<CertificateChain>,
}

/// Server binding information
struct ServerBinding {
    address: String,
    listener: TcpListener,
}

impl CertificateChains {
    /// Generate two separate certificate chains for server and proxy
    fn generate() -> Result<Self, String> {
        let server_chain =
            Arc::new(Some(CertificateChain::generate().map_err(|e| {
                format!("Failed to generate server certificates: {}", e)
            })?));

        let proxy_chain = Arc::new(
            CertificateChain::generate()
                .map_err(|e| format!("Failed to generate proxy certificates: {}", e))?,
        );

        Ok(Self {
            server_chain,
            proxy_chain,
        })
    }
}

impl ServerBinding {
    /// Find an available port and create a TCP listener
    async fn new(purpose: &str) -> Result<Self, String> {
        let (address, listener) = network::find_available_bind_address_with_listener()
            .await
            .map_err(|e| format!("Failed to find available port for {}: {}", purpose, e))?;

        Ok(Self { address, listener })
    }

    /// Get the HTTPS URL for this binding
    fn https_url(&self, path: &str) -> String {
        format!("https://{}{}", self.address, path)
    }
}

/// Start the local MCP server with tools
async fn start_mcp_server(
    app_config: &AppConfig,
    mcp_config: &McpInitConfig,
    binding: ServerBinding,
    cert_chain: Arc<Option<CertificateChain>>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), String> {
    let api_client = get_client(app_config).await?;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();

    let bind_address = binding.address.clone();
    let enabled_tools = mcp_config.enabled_tools.clone();
    let enable_subagents = mcp_config.enable_subagents;
    let subagent_config = mcp_config.subagent_config.clone();

    tokio::spawn(async move {
        let server_config = MCPServerConfig {
            client: Some(api_client),
            bind_address,
            enabled_tools,
            tool_mode: ToolMode::Combined,
            enable_subagents,
            certificate_chain: cert_chain,
            skill_directories: default_skill_directories(),
            subagent_config,
            server_tls_config: None,
        };

        // Signal that we're about to start
        let _ = ready_tx.send(Ok(()));

        if let Err(e) = start_server(server_config, Some(binding.listener), Some(shutdown_rx)).await
        {
            tracing::error!("Local MCP server error: {}", e);
        }
    });

    // Wait for server to signal it's starting
    ready_rx
        .await
        .map_err(|_| "MCP server task failed to start".to_string())?
        .map_err(|e| format!("MCP server failed to start: {}", e))?;

    // Small delay to ensure the server is actually listening
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(())
}

/// Build the proxy configuration with upstream servers
fn build_proxy_config(
    local_server_url: String,
    server_cert_chain: Arc<Option<CertificateChain>>,
) -> ClientPoolConfig {
    let mut servers: HashMap<String, ServerConfig> = HashMap::new();

    // Add local MCP server (tools) as upstream
    servers.insert(
        "stakpak".to_string(),
        ServerConfig::Http {
            url: local_server_url,
            headers: None,
            certificate_chain: server_cert_chain,
            client_tls_config: None,
        },
    );

    // Add external paks server
    servers.insert(
        "paks".to_string(),
        ServerConfig::Http {
            url: "https://apiv2.stakpak.dev/v1/paks/mcp".to_string(),
            headers: None,
            certificate_chain: Arc::new(None),
            client_tls_config: None,
        },
    );

    ClientPoolConfig::with_servers(servers)
}

/// Start the proxy server
async fn start_proxy(
    pool_config: ClientPoolConfig,
    mcp_config: &McpInitConfig,
    binding: ServerBinding,
    cert_chain: Arc<CertificateChain>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), String> {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();

    let redact_secrets = mcp_config.redact_secrets;
    let privacy_mode = mcp_config.privacy_mode;

    tokio::spawn(async move {
        // Signal that we're about to start
        let _ = ready_tx.send(Ok(()));

        if let Err(e) = start_proxy_server(
            pool_config,
            binding.listener,
            cert_chain,
            redact_secrets,
            privacy_mode,
            Some(shutdown_rx),
        )
        .await
        {
            tracing::error!("Proxy server error: {}", e);
        }
    });

    // Wait for proxy to signal it's starting
    ready_rx
        .await
        .map_err(|_| "Proxy server task failed to start".to_string())?
        .map_err(|e| format!("Proxy server failed to start: {}", e))?;

    Ok(())
}

/// Connect to the proxy with retry logic
async fn connect_to_proxy(
    proxy_url: &str,
    cert_chain: Arc<CertificateChain>,
    progress_tx: Option<Sender<ToolCallResultProgress>>,
) -> Result<Arc<McpClient>, String> {
    const MAX_RETRIES: u32 = 5;
    let mut retry_delay = tokio::time::Duration::from_millis(50);
    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        match stakpak_mcp_client::connect_https(
            proxy_url,
            Some(cert_chain.clone()),
            progress_tx.clone(),
        )
        .await
        {
            Ok(client) => return Ok(Arc::new(client)),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(retry_delay).await;
                    retry_delay *= 2; // Exponential backoff
                }
            }
        }
    }

    Err(format!(
        "Failed to connect to MCP proxy after {} retries: {}",
        MAX_RETRIES,
        last_error.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// Initialize the MCP server, proxy, and client infrastructure
///
/// This function sets up the complete MCP infrastructure:
/// 1. Generates certificate chains for mTLS
/// 2. Starts the local MCP server with tools
/// 3. Starts the proxy server that aggregates MCP servers
/// 4. Connects a client to the proxy
///
/// Returns the client, tools, and shutdown handles for graceful cleanup.
pub async fn initialize_mcp_server_and_tools(
    app_config: &AppConfig,
    mcp_config: McpInitConfig,
    progress_tx: Option<Sender<ToolCallResultProgress>>,
) -> Result<McpInitResult, String> {
    // 1. Generate certificate chains
    let certs = CertificateChains::generate()?;

    // 2. Find available ports
    let server_binding = ServerBinding::new("MCP server").await?;
    let proxy_binding = ServerBinding::new("proxy").await?;

    let local_mcp_server_url = server_binding.https_url("/mcp");
    let proxy_url = proxy_binding.https_url("/mcp");

    // 3. Create shutdown channels
    let (server_shutdown_tx, server_shutdown_rx) = broadcast::channel::<()>(1);
    let (proxy_shutdown_tx, proxy_shutdown_rx) = broadcast::channel::<()>(1);

    // 4. Start local MCP server
    start_mcp_server(
        app_config,
        &mcp_config,
        server_binding,
        certs.server_chain.clone(),
        server_shutdown_rx,
    )
    .await?;

    // 5. Build and start proxy
    let pool_config = build_proxy_config(local_mcp_server_url, certs.server_chain);
    start_proxy(
        pool_config,
        &mcp_config,
        proxy_binding,
        certs.proxy_chain.clone(),
        proxy_shutdown_rx,
    )
    .await?;

    // 6. Connect client to proxy
    let mcp_client = connect_to_proxy(&proxy_url, certs.proxy_chain, progress_tx).await?;

    // 7. Get tools from MCP client
    let mcp_tools = stakpak_mcp_client::get_tools(&mcp_client)
        .await
        .map_err(|e| format!("Failed to get tools: {}", e))?;

    // Use allowed_tools from mcp_config if provided, otherwise fall back to app_config
    let allowed_tools_ref = mcp_config
        .allowed_tools
        .as_ref()
        .or(app_config.allowed_tools.as_ref());
    let tools = convert_tools_with_filter(&mcp_tools, allowed_tools_ref);

    Ok(McpInitResult {
        client: mcp_client,
        mcp_tools,
        tools,
        server_shutdown_tx,
        proxy_shutdown_tx,
    })
}
