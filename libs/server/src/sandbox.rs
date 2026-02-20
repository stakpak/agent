//! Per-session sandboxed MCP server management.
//!
//! When sandbox mode is enabled for a session, a `stakpak mcp start` server
//! is spawned inside a warden container. The host-side proxy connects to it
//! via HTTPS/mTLS, and tool calls from the agent loop are routed through the
//! containerized server — executing `run_command`, file I/O, etc. inside the
//! sandbox.
//!
//! **mTLS key exchange** — each side generates its own identity independently:
//!
//! 1. Host generates a client identity (CA + leaf cert + key, all in memory)
//! 2. Host passes the client **CA cert** (public only) to the container via env var
//! 3. Container generates a server identity (CA + leaf cert + key, all in memory)
//! 4. Container outputs the server **CA cert** (public only) to stdout
//! 5. Host parses the server CA cert and builds a client TLS config
//!
//! Private keys never leave their respective processes.

use stakpak_mcp_client::McpClient;
use stakpak_mcp_proxy::client::{ClientPoolConfig, ServerConfig};
use stakpak_mcp_proxy::server::start_proxy_server;
use stakpak_shared::cert_utils::{CertificateChain, MtlsIdentity};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::net::TcpListener;
use tokio::process::Child;
use tokio::sync::broadcast;

/// Environment variable used to pass the client CA cert PEM to the container.
const TRUSTED_CLIENT_CA_ENV: &str = "STAKPAK_MCP_CLIENT_CA";

/// Configuration for spawning sandboxed MCP servers.
#[derive(Clone, Debug)]
pub struct SandboxConfig {
    /// Path to the warden binary.
    pub warden_path: String,
    /// Container image for the sandbox (e.g., `ghcr.io/stakpak/agent:v1.2.3`).
    pub image: String,
    /// Volume mounts for the container (e.g., `["./:/agent:ro"]`).
    pub volumes: Vec<String>,
}

/// A running sandboxed MCP server with its associated proxy and client.
///
/// Drop this struct to shut down the sandbox.
pub struct SandboxedMcpServer {
    /// MCP client connected via the per-session proxy.
    pub client: Arc<McpClient>,
    /// Tools available from the sandboxed server.
    pub tools: Vec<stakai::Tool>,
    /// Channel to shut down the per-session proxy.
    proxy_shutdown_tx: broadcast::Sender<()>,
    /// The warden container child process.
    container_process: Child,
}

impl SandboxedMcpServer {
    /// Spawn a sandboxed MCP server inside a warden container and connect to it.
    ///
    /// 1. Generates a client mTLS identity (private key stays in host memory)
    /// 2. Passes the client CA cert (public) to the container via env var
    /// 3. Spawns `warden wrap <image> -- stakpak mcp start`
    /// 4. Parses the server CA cert (public) from the container's stdout
    /// 5. Builds a client TLS config trusting the server CA, using the client key
    /// 6. Starts a per-session MCP proxy pointing to the container
    /// 7. Connects a client to the proxy
    pub async fn spawn(config: &SandboxConfig) -> Result<Self, String> {
        // 1. Generate client identity — private key stays in host memory
        let client_identity = MtlsIdentity::generate_client()
            .map_err(|e| format!("Failed to generate client identity: {e}"))?;

        let client_ca_pem = client_identity
            .ca_cert_pem()
            .map_err(|e| format!("Failed to get client CA PEM: {e}"))?;

        // 2. Find a free port for the container's MCP server to expose
        let container_host_port = find_free_port()
            .await
            .map_err(|e| format!("Failed to find free port for sandbox: {e}"))?;

        // 3. Spawn warden container, passing client CA cert (public) via env var
        let mut container_process =
            spawn_warden_container(config, container_host_port, &client_ca_pem)
                .await
                .map_err(|e| format!("Failed to spawn sandbox container: {e}"))?;

        // 4. Parse the server CA cert (public) from the container's stdout
        let server_ca_pem = parse_server_ca_from_stdout(&mut container_process).await?;
        tracing::info!(
            "Parsed server CA from container stdout ({} bytes)",
            server_ca_pem.len()
        );

        // 5. Build client TLS config — trusts server CA, authenticates with our key
        let container_client_config = client_identity
            .create_client_config(&server_ca_pem)
            .map_err(|e| format!("Failed to create client TLS config: {e}"))?;

        // 6. Wait for the MCP server inside the container to be ready
        let server_url = format!("https://127.0.0.1:{container_host_port}/mcp");
        tracing::info!(url = %server_url, "Waiting for sandbox MCP server to be ready");
        wait_for_server_ready(&server_url, &container_client_config).await?;
        tracing::info!("Sandbox MCP server is ready");

        // 7. Start a per-session proxy connecting to the sandboxed server
        let (proxy_shutdown_tx, proxy_shutdown_rx) = broadcast::channel::<()>(1);

        let proxy_binding = find_available_binding("sandbox proxy").await?;
        let proxy_url = format!("https://{}/mcp", proxy_binding.address);

        let proxy_cert_chain = Arc::new(
            CertificateChain::generate()
                .map_err(|e| format!("Failed to generate proxy certificates: {e}"))?,
        );

        let pool_config = build_sandbox_proxy_config(server_url, Arc::new(container_client_config));

        let proxy_chain_for_server = proxy_cert_chain.clone();
        let proxy_listener = proxy_binding.listener;
        tokio::spawn(async move {
            if let Err(e) = start_proxy_server(
                pool_config,
                proxy_listener,
                proxy_chain_for_server,
                true,  // redact_secrets
                false, // privacy_mode
                Some(proxy_shutdown_rx),
            )
            .await
            {
                tracing::error!("Sandbox proxy error: {e}");
            }
        });

        // Small delay for proxy to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 8. Connect client to proxy
        let client = connect_to_proxy(&proxy_url, proxy_cert_chain).await?;

        // 9. Get tools
        let mcp_tools = stakpak_mcp_client::get_tools(&client)
            .await
            .map_err(|e| format!("Failed to get sandbox tools: {e}"))?;

        let tools = mcp_tools
            .into_iter()
            .map(|tool| stakai::Tool {
                tool_type: "function".to_string(),
                function: stakai::ToolFunction {
                    name: tool.name.as_ref().to_string(),
                    description: tool
                        .description
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .unwrap_or_default(),
                    parameters: serde_json::Value::Object((*tool.input_schema).clone()),
                },
                provider_options: None,
            })
            .collect();

        Ok(Self {
            client,
            tools,
            proxy_shutdown_tx,
            container_process,
        })
    }

    /// Shut down the sandbox: stop the proxy and kill the container.
    pub async fn shutdown(mut self) {
        let _ = self.proxy_shutdown_tx.send(());

        let _ = self.container_process.kill().await;
        let _ = self.container_process.wait().await;
    }
}

async fn spawn_warden_container(
    config: &SandboxConfig,
    host_port: u16,
    client_ca_pem: &str,
) -> Result<Child, String> {
    use stakpak_shared::container::{expand_volume_path, is_named_volume};

    let mut cmd = tokio::process::Command::new(&config.warden_path);
    cmd.arg("wrap");
    cmd.arg(&config.image);

    // Mount configured volumes
    for vol in &config.volumes {
        let expanded = expand_volume_path(vol);
        let host_path = expanded.split(':').next().unwrap_or(&expanded);
        // Named volumes (e.g. "stakpak-aqua-cache:/container/path") don't have a
        // host filesystem path — mount them unconditionally. Bind mounts are only
        // added when the host path actually exists.
        if is_named_volume(host_path) || Path::new(host_path).exists() {
            cmd.args(["--volume", &expanded]);
        }
    }

    // Port forwarding for the MCP server — publish on the sidecar so the
    // host can reach the container's MCP server port directly.
    cmd.args(["-p", &format!("127.0.0.1:{host_port}:8080")]);

    // Prevent warden re-entry
    cmd.args(["--env", "STAKPAK_SKIP_WARDEN=1"]);

    // Tell the MCP server to bind to a fixed port inside the container
    // so it matches the published port on the sidecar.
    cmd.args(["--env", "STAKPAK_MCP_PORT=8080"]);

    // Pass the client CA cert (public only) so the server can trust the client.
    cmd.args(["--env", &format!("{TRUSTED_CLIENT_CA_ENV}={client_ca_pem}")]);

    // Pass through API credentials if set
    if let Ok(api_key) = std::env::var("STAKPAK_API_KEY") {
        cmd.args(["--env", &format!("STAKPAK_API_KEY={api_key}")]);
    }
    if let Ok(profile) = std::env::var("STAKPAK_PROFILE") {
        cmd.args(["--env", &format!("STAKPAK_PROFILE={profile}")]);
    }
    if let Ok(endpoint) = std::env::var("STAKPAK_API_ENDPOINT") {
        cmd.args(["--env", &format!("STAKPAK_API_ENDPOINT={endpoint}")]);
    }

    // The MCP server detects STAKPAK_MCP_CLIENT_CA and generates its own
    // server identity, outputting the server CA cert to stdout.
    cmd.args(["--", "stakpak", "mcp", "start"]);

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn warden process: {e}"))?;

    Ok(child)
}

/// Parse the server CA certificate PEM from the container's stdout.
///
/// The MCP server outputs the server CA cert between structured delimiters:
/// ```text
/// ---BEGIN STAKPAK SERVER CA---
/// -----BEGIN CERTIFICATE-----
/// ...
/// -----END CERTIFICATE-----
/// ---END STAKPAK SERVER CA---
/// ```
async fn parse_server_ca_from_stdout(process: &mut Child) -> Result<String, String> {
    let stdout = process
        .stdout
        .take()
        .ok_or_else(|| "Container stdout not captured".to_string())?;

    let mut reader = tokio::io::BufReader::new(stdout);
    let mut server_ca_pem = String::new();
    let mut in_server_ca = false;
    let mut line = String::new();

    let timeout_duration = tokio::time::Duration::from_secs(60);
    let deadline = tokio::time::Instant::now() + timeout_duration;

    tracing::debug!("Starting to read container stdout for server CA...");

    loop {
        line.clear();
        let bytes_read = tokio::time::timeout_at(deadline, reader.read_line(&mut line))
            .await
            .map_err(|_| {
                "Timed out waiting for container to output server CA certificate".to_string()
            })?
            .map_err(|e| format!("Failed to read container stdout: {e}"))?;

        if bytes_read == 0 {
            tracing::error!("Container stdout EOF before server CA was found");
            return Err("Container exited before outputting server CA certificate".to_string());
        }

        let trimmed = line.trim();
        tracing::debug!(line = %trimmed, bytes = bytes_read, "Read line from container stdout");

        if trimmed == "---BEGIN STAKPAK SERVER CA---" {
            in_server_ca = true;
            continue;
        }

        if trimmed == "---END STAKPAK SERVER CA---" {
            tracing::debug!("Found end of server CA block");
            break;
        }

        if in_server_ca {
            server_ca_pem.push_str(trimmed);
            server_ca_pem.push('\n');
        }
    }

    let server_ca_pem = server_ca_pem.trim().to_string();

    if server_ca_pem.is_empty() {
        return Err("Failed to parse server CA certificate from container output".to_string());
    }

    Ok(server_ca_pem)
}

async fn wait_for_server_ready(
    url: &str,
    client_config: &rustls::ClientConfig,
) -> Result<(), String> {
    let http_client = reqwest::Client::builder()
        .use_preconfigured_tls(client_config.clone())
        .build()
        .map_err(|e| format!("Failed to build readiness check client: {e}"))?;

    let mut last_error = String::new();
    for attempt in 0..30 {
        tokio::time::sleep(tokio::time::Duration::from_millis(if attempt < 5 {
            500
        } else {
            1000
        }))
        .await;

        match http_client.get(url).send().await {
            Ok(_) => {
                tracing::info!(attempt, "Sandbox MCP server ready");
                return Ok(());
            }
            Err(e) => {
                last_error = format!("{e:?}");
                tracing::debug!(attempt, error = %last_error, "Readiness check failed");
            }
        }
    }

    Err(format!(
        "Sandbox MCP server failed to become ready after 30 attempts: {last_error}"
    ))
}

struct ProxyBinding {
    address: String,
    listener: TcpListener,
}

async fn find_available_binding(purpose: &str) -> Result<ProxyBinding, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind port for {purpose}: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get address for {purpose}: {e}"))?;
    Ok(ProxyBinding {
        address: addr.to_string(),
        listener,
    })
}

async fn find_free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind ephemeral port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get ephemeral port: {e}"))?
        .port();
    // Drop the listener to free the port for Docker to use
    drop(listener);
    Ok(port)
}

fn build_sandbox_proxy_config(
    sandbox_server_url: String,
    client_tls_config: Arc<rustls::ClientConfig>,
) -> ClientPoolConfig {
    let mut servers: HashMap<String, ServerConfig> = HashMap::new();

    // Register the sandboxed MCP server under the same name ("stakpak") so
    // tool names like `stakpak__run_command` route correctly through the proxy.
    servers.insert(
        "stakpak".to_string(),
        ServerConfig::Http {
            url: sandbox_server_url,
            headers: None,
            certificate_chain: Arc::new(None),
            client_tls_config: Some(client_tls_config),
        },
    );

    // Keep the external paks server accessible
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

async fn connect_to_proxy(
    proxy_url: &str,
    cert_chain: Arc<CertificateChain>,
) -> Result<Arc<McpClient>, String> {
    const MAX_RETRIES: u32 = 5;
    let mut retry_delay = tokio::time::Duration::from_millis(50);
    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        match stakpak_mcp_client::connect_https(proxy_url, Some(cert_chain.clone()), None).await {
            Ok(client) => return Ok(Arc::new(client)),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(retry_delay).await;
                    retry_delay *= 2;
                }
            }
        }
    }

    Err(format!(
        "Failed to connect to sandbox proxy after {MAX_RETRIES} retries: {}",
        last_error.map(|e| e.to_string()).unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_server_ca_from_structured_output() {
        let output = "\
🔐 mTLS enabled - independent identity (sandbox mode)
---BEGIN STAKPAK SERVER CA---
-----BEGIN CERTIFICATE-----
MIIB0zCCAXmgAwIBAgIUFAKE=
-----END CERTIFICATE-----
---END STAKPAK SERVER CA---
MCP server started at https://0.0.0.0:8080/mcp
";

        let expected_ca = "\
-----BEGIN CERTIFICATE-----
MIIB0zCCAXmgAwIBAgIUFAKE=
-----END CERTIFICATE-----";

        // Parse the same way parse_server_ca_from_stdout does
        let mut server_ca_pem = String::new();
        let mut in_server_ca = false;

        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed == "---BEGIN STAKPAK SERVER CA---" {
                in_server_ca = true;
                continue;
            }
            if trimmed == "---END STAKPAK SERVER CA---" {
                break;
            }
            if in_server_ca {
                server_ca_pem.push_str(trimmed);
                server_ca_pem.push('\n');
            }
        }

        assert_eq!(server_ca_pem.trim(), expected_ca);
    }

    #[test]
    fn mtls_identity_cross_trust() {
        use stakpak_shared::cert_utils::MtlsIdentity;

        // Ensure a crypto provider is installed for rustls
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // Simulate the sandbox mTLS exchange
        let client_identity = MtlsIdentity::generate_client().expect("generate client identity");
        let server_identity = MtlsIdentity::generate_server().expect("generate server identity");

        let client_ca_pem = client_identity.ca_cert_pem().expect("client CA PEM");
        let server_ca_pem = server_identity.ca_cert_pem().expect("server CA PEM");

        // Server trusts client CA, client trusts server CA
        let _server_config = server_identity
            .create_server_config(&client_ca_pem)
            .expect("server config with client CA trust");
        let _client_config = client_identity
            .create_client_config(&server_ca_pem)
            .expect("client config with server CA trust");

        // Only public CA certs were exchanged — private keys stayed in their
        // respective MtlsIdentity structs.
        assert!(client_ca_pem.contains("BEGIN CERTIFICATE"));
        assert!(server_ca_pem.contains("BEGIN CERTIFICATE"));
        assert!(!client_ca_pem.contains("PRIVATE KEY"));
        assert!(!server_ca_pem.contains("PRIVATE KEY"));
    }

    // ── Named volume detection in expand_volume_path / mount filter ────

    #[test]
    fn expand_volume_path_leaves_named_volumes_unchanged() {
        use stakpak_shared::container::expand_volume_path;
        let named = "stakpak-aqua-cache:/home/agent/.local/share/aquaproj-aqua";
        assert_eq!(expand_volume_path(named), named);
    }

    /// Named volumes (no `/` or `.` prefix in the host part) must pass the
    /// mount filter even though they don't exist on the host filesystem.
    #[test]
    fn named_volume_is_detected_correctly() {
        use stakpak_shared::container::is_named_volume;
        let cases = vec![
            ("stakpak-aqua-cache", true),
            ("my-volume", true),
            ("./relative/path", false),
            ("/absolute/path", false),
            ("relative/with/slash", false),
            (".", false),
        ];
        for (host_part, expected) in cases {
            assert_eq!(
                is_named_volume(host_part),
                expected,
                "host_part={host_part:?} expected named={expected}"
            );
        }
    }
}
