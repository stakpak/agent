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
        #[serde(default)]
        disabled: bool,
    },
    UrlBased {
        url: String,
        #[serde(default)]
        headers: Option<HashMap<String, String>>,
        #[serde(default)]
        disabled: bool,
    },
}

impl ServerConfigJson {
    fn is_disabled(&self) -> bool {
        match self {
            ServerConfigJson::CommandBased { disabled, .. } => *disabled,
            ServerConfigJson::UrlBased { disabled, .. } => *disabled,
        }
    }
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
            
            if server_config_json.is_disabled() {
                continue;
            }

            let server_config = match server_config_json {
                ServerConfigJson::CommandBased {
                    command, args, env, ..
                } => {
                    let env = env.map(|vars| {
                        vars.into_iter()
                            .map(|(k, v)| (k, substitute_env_vars(&v)))
                            .collect()
                    });
                    ServerConfig::Stdio { command, args, env }
                }
                ServerConfigJson::UrlBased { url, headers, .. } => ServerConfig::Http {
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

/// Substitute `$VAR` and `${VAR}` patterns in a string with environment variable values.
/// Unknown variables are left as-is.
fn substitute_env_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'{') {
                // ${VAR} form
                chars.next(); // consume '{'
                let mut var_name = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    var_name.push(c);
                }
                match std::env::var(&var_name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        result.push_str("${");
                        result.push_str(&var_name);
                        result.push('}');
                    }
                }
            } else {
                // $VAR form — collect alphanumeric + underscore manually
                // (take_while would consume the first non-matching char)
                let mut var_name = String::new();
                loop {
                    match chars.peek() {
                        Some(&c) if c.is_alphanumeric() || c == '_' => {
                            var_name.push(c);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if var_name.is_empty() {
                    result.push('$');
                } else {
                    match std::env::var(&var_name) {
                        Ok(val) => result.push_str(&val),
                        Err(_) => {
                            result.push('$');
                            result.push_str(&var_name);
                        }
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_no_vars() {
        assert_eq!(substitute_env_vars("hello world"), "hello world");
    }

    #[test]
    fn test_substitute_dollar_sign_only() {
        assert_eq!(substitute_env_vars("price is $"), "price is $");
    }

    #[test]
    fn test_substitute_unknown_var_preserved() {
        assert_eq!(substitute_env_vars("$UNKNOWN_VAR_XYZ"), "$UNKNOWN_VAR_XYZ");
    }

    #[test]
    fn test_substitute_unknown_braced_var_preserved() {
        assert_eq!(
            substitute_env_vars("${UNKNOWN_VAR_XYZ}"),
            "${UNKNOWN_VAR_XYZ}"
        );
    }

    #[test]
    fn test_substitute_known_var() {
        unsafe { std::env::set_var("TEST_MCP_SUBSTITUTE", "secret_value") };
        assert_eq!(substitute_env_vars("$TEST_MCP_SUBSTITUTE"), "secret_value");
        assert_eq!(
            substitute_env_vars("${TEST_MCP_SUBSTITUTE}"),
            "secret_value"
        );
    }

    #[test]
    fn test_substitute_var_in_middle() {
        unsafe { std::env::set_var("TEST_MCP_KEY", "abc123") };
        assert_eq!(
            substitute_env_vars("prefix_${TEST_MCP_KEY}_suffix"),
            "prefix_abc123_suffix"
        );
    }

    #[test]
    fn test_substitute_multiple_vars() {
        unsafe { std::env::set_var("TEST_MCP_A", "one") };
        unsafe { std::env::set_var("TEST_MCP_B", "two") };
        assert_eq!(
            substitute_env_vars("$TEST_MCP_A and $TEST_MCP_B"),
            "one and two"
        );
    }

    #[test]
    fn test_disabled_server_filtered_out() {
        let toml_str = r#"
[mcpServers.active]
command = "npx"
args = ["-y", "active-server"]

[mcpServers.disabled-one]
command = "npx"
args = ["-y", "disabled-server"]
disabled = true

[mcpServers.active-url]
url = "https://example.com/mcp"

[mcpServers.disabled-url]
url = "https://disabled.com/mcp"
disabled = true
"#;
        let config = ClientPoolConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(config.servers.len(), 2);
        assert!(config.servers.contains_key("active"));
        assert!(config.servers.contains_key("active-url"));
        assert!(!config.servers.contains_key("disabled-one"));
        assert!(!config.servers.contains_key("disabled-url"));
    }

    #[test]
    fn test_disabled_false_not_filtered() {
        let toml_str = r#"
[mcpServers.myserver]
command = "npx"
args = ["-y", "my-server"]
disabled = false
"#;
        let config = ClientPoolConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert!(config.servers.contains_key("myserver"));
    }

    #[test]
    fn test_default_not_disabled() {
        let toml_str = r#"
[mcpServers.myserver]
command = "npx"
args = ["-y", "my-server"]
"#;
        let config = ClientPoolConfig::from_toml_str(toml_str).unwrap();
        assert_eq!(config.servers.len(), 1);
    }

    #[test]
    fn test_env_substitution_in_config() {
        unsafe { std::env::set_var("TEST_MCP_TOKEN", "my-token-value") };
        let toml_str = r#"
[mcpServers.github]
command = "npx"
args = ["-y", "server"]
env = { GITHUB_TOKEN = "$TEST_MCP_TOKEN" }
"#;
        let config = ClientPoolConfig::from_toml_str(toml_str).unwrap();
        match config.servers.get("github").unwrap() {
            ServerConfig::Stdio { env: Some(env), .. } => {
                assert_eq!(env.get("GITHUB_TOKEN").unwrap(), "my-token-value");
            }
            _ => panic!("Expected Stdio config"),
        }
    }
}
