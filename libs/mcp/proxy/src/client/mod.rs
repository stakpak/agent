use rmcp::ClientHandler;
use rmcp::model::{
    CancelledNotificationParam, ClientCapabilities, ClientInfo, Implementation,
    ProgressNotificationParam,
};
use rmcp::service::{NotificationContext, Peer, RunningService};
use rmcp::{RoleClient, RoleServer};
use serde::Deserialize;
use stakpak_shared::cert_utils::CertificateChain;
use std::collections::HashMap;
use std::fs;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Client handler that forwards notifications from upstream servers to downstream server
#[derive(Clone)]
pub struct ProxyClientHandler {
    downstream_peer: Arc<Mutex<Option<Peer<RoleServer>>>>,
}

impl ProxyClientHandler {
    pub fn new(downstream_peer: Arc<Mutex<Option<Peer<RoleServer>>>>) -> Self {
        Self { downstream_peer }
    }
}

impl ClientHandler for ProxyClientHandler {
    async fn on_progress(
        &self,
        notification: ProgressNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) {
        // Then forward progress notification from upstream server to downstream server
        let peer = self.downstream_peer.lock().await;
        if let Some(ref peer) = *peer {
            let _ = peer.notify_progress(notification).await;
        } else {
            tracing::debug!("Progress notification received but no downstream peer available");
        }
    }

    async fn on_cancelled(
        &self,
        notification: CancelledNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) {
        // Forward cancellation notification from upstream server to downstream server
        let peer = self.downstream_peer.lock().await;
        if let Some(ref peer) = *peer {
            let _ = peer.notify_cancelled(notification).await;
        } else {
            tracing::debug!("Cancellation notification received but no downstream peer available");
        }
    }

    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "proxy-client-handler".to_string(),
                version: "0.1.0".to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
        }
    }
}

pub struct ClientPool {
    pub(crate) clients: Arc<Mutex<HashMap<String, RunningService<RoleClient, ProxyClientHandler>>>>,
}

impl ClientPool {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_client(
        &self,
        name: String,
        client: RunningService<RoleClient, ProxyClientHandler>,
    ) {
        self.clients.lock().await.insert(name, client);
    }

    pub async fn get_clients(
        &self,
    ) -> tokio::sync::MutexGuard<'_, HashMap<String, RunningService<RoleClient, ProxyClientHandler>>>
    {
        self.clients.lock().await
    }

    /// Get a cloned peer for a specific client without holding the lock during async operations.
    /// This prevents mutex contention during long-running tool calls.
    pub async fn get_client_peer(&self, name: &str) -> Option<Peer<RoleClient>> {
        let clients = self.clients.lock().await;
        clients.get(name).map(|running_service| {
            // RunningService derefs to Peer<R>, and Peer is Clone
            running_service.deref().clone()
        })
    }

    /// Get all client names currently in the pool
    pub async fn get_client_names(&self) -> Vec<String> {
        let clients = self.clients.lock().await;
        clients.keys().cloned().collect()
    }

    /// Get cloned peers for all clients without holding the lock during async operations
    pub async fn get_all_client_peers(&self) -> HashMap<String, Peer<RoleClient>> {
        let clients = self.clients.lock().await;
        clients
            .iter()
            .map(|(name, running_service)| (name.clone(), running_service.deref().clone()))
            .collect()
    }
}

impl Default for ClientPool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum ServerConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Option<HashMap<String, String>>,
    },
    Http {
        url: String,
        headers: Option<HashMap<String, String>>,
        /// Optional certificate chain for mTLS (used for local server connections)
        certificate_chain: Arc<Option<CertificateChain>>,
        /// Pre-built client TLS config. When set, takes precedence over `certificate_chain`.
        client_tls_config: Option<Arc<rustls::ClientConfig>>,
    },
}

#[derive(Debug, Deserialize)]
struct McpConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, ServerConfigJson>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ServerConfigJson {
    CommandBased {
        command: String,
        args: Vec<String>,
        #[serde(default)]
        env: Option<HashMap<String, String>>,
    },
    UrlBased {
        url: String,
        #[serde(default)]
        headers: Option<HashMap<String, String>>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ClientPoolConfig {
    pub servers: HashMap<String, ServerConfig>,
}

impl ClientPoolConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_servers(servers: HashMap<String, ServerConfig>) -> Self {
        Self { servers }
    }

    pub fn from_json_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_json_str(&content)
    }

    pub fn from_json_str(json_str: &str) -> anyhow::Result<Self> {
        let mcp_config: McpConfig = serde_json::from_str(json_str)?;
        Ok(Self::from_mcp_config(mcp_config))
    }

    pub fn from_toml_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    pub fn from_toml_str(toml_str: &str) -> anyhow::Result<Self> {
        let mcp_config: McpConfig = toml::from_str(toml_str)?;
        Ok(Self::from_mcp_config(mcp_config))
    }

    fn from_mcp_config(mcp_config: McpConfig) -> Self {
        let mut servers = HashMap::new();

        for (name, server_config_json) in mcp_config.mcp_servers {
            let server_config = match server_config_json {
                ServerConfigJson::CommandBased { command, args, env } => {
                    ServerConfig::Stdio { command, args, env }
                }
                ServerConfigJson::UrlBased { url, headers } => ServerConfig::Http {
                    url,
                    headers,
                    certificate_chain: Arc::new(None),
                    client_tls_config: None,
                },
            };
            servers.insert(name, server_config);
        }

        Self { servers }
    }
}
