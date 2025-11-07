use rmcp::service::RunningService;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{RoleClient, ServiceExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;

pub struct ClientPool {
    pub(crate) clients: Arc<Mutex<HashMap<String, RunningService<RoleClient, ()>>>>,
}

impl ClientPool {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_client(&self, name: String, client: RunningService<RoleClient, ()>) {
        self.clients.lock().await.insert(name, client);
    }

    pub async fn get_clients(
        &self,
    ) -> tokio::sync::MutexGuard<'_, HashMap<String, RunningService<RoleClient, ()>>> {
        self.clients.lock().await
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
                ServerConfigJson::UrlBased { url, headers } => ServerConfig::Http { url, headers },
            };
            servers.insert(name, server_config);
        }

        Self { servers }
    }
}



pub async fn initialize_clients(
    config: ClientPoolConfig,
    pool: Arc<ClientPool>,
) -> Arc<ClientPool> {
    for (name, server_config) in config.servers {
        let pool_clone = pool.clone();
        let name_clone = name.clone();

        tokio::spawn(async move {
            match server_config {
                ServerConfig::Stdio { command, args, env } => {
                    let mut cmd = Command::new(&command);
                    for arg in args {
                        cmd.arg(arg);
                    }
                    if let Some(env_vars) = env {
                        cmd.envs(&env_vars);
                    }
                    let proc = match TokioChildProcess::new(cmd) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!("Failed to create process for {}: {:?}", name_clone, e);
                            return;
                        }
                    };

                    // Use () as a default client handler to serve the process
                    match ().serve(proc).await {
                        Ok(client) => {
                            pool_clone.add_client(name_clone.clone(), client).await;
                            tracing::info!("{} MCP client initialized", name_clone);
                        }
                        Err(e) => {
                            tracing::error!("Failed to start {} MCP client: {:?}", name_clone, e);
                        }
                    }
                }
                ServerConfig::Http { url, headers } => {
                    // Validate TLS usage
                    if !url.starts_with("https://") {
                        tracing::warn!(
                            "⚠️  MCP server '{}' is using insecure HTTP connection: {}",
                            name_clone,
                            url
                        );
                        tracing::warn!(
                            "   Consider using HTTPS or pass --allow-insecure-mcp-transport flag"
                        );
                        // TODO: Check for --allow-insecure-mcp-transport flag and return early if not set
                    }

                    let mut client_builder = reqwest::Client::builder();
                    if let Some(headers_map) = headers {
                        let mut header_map = reqwest::header::HeaderMap::new();
                        for (key, value) in headers_map {
                            if let (Ok(header_name), Ok(header_value)) = (
                                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                                reqwest::header::HeaderValue::from_str(&value),
                            ) {
                                header_map.insert(header_name, header_value);
                            } else {
                                tracing::warn!(
                                    "Invalid header for {}: {} = {}",
                                    name_clone,
                                    key,
                                    value
                                );
                            }
                        }
                        client_builder = client_builder.default_headers(header_map);
                    }

                    let http_client = match client_builder.build() {
                        Ok(client) => client,
                        Err(e) => {
                            tracing::error!(
                                "Failed to build HTTP client for {}: {:?}",
                                name_clone,
                                e
                            );
                            return;
                        }
                    };

                    let config = StreamableHttpClientTransportConfig::with_uri(url.as_str());
                    let transport = StreamableHttpClientTransport::<reqwest::Client>::with_client(
                        http_client,
                        config,
                    );
                    match ().serve(transport).await {
                        Ok(client) => {
                            pool_clone.add_client(name_clone.clone(), client).await;
                            tracing::info!("{} MCP client initialized", name_clone);
                        }
                        Err(e) => {
                            tracing::error!("Failed to start {} MCP client: {:?}", name_clone, e);
                        }
                    }
                }
            }
        });
    }

    pool
}
