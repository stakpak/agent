use std::sync::Arc;

use stakpak_api::local::skills::default_skill_directories;
use stakpak_mcp_server::{
    EnabledToolsConfig, MCPServerConfig, SubagentConfig, ToolMode, start_server,
};
use stakpak_shared::cert_utils::{CertificateChain, MtlsIdentity};

use crate::utils::network;
use crate::{commands::get_client, config::AppConfig};

/// Environment variable that, when set, contains the PEM-encoded CA certificate
/// of the client. The server will trust this CA for mTLS client verification and
/// generate its own server identity independently.
///
/// This is the secure sandbox flow: only public CA certificates are exchanged,
/// private keys never leave their respective processes.
const TRUSTED_CLIENT_CA_ENV: &str = "STAKPAK_MCP_CLIENT_CA";

/// Start the MCP server (standalone HTTP/HTTPS server with tools)
#[allow(clippy::too_many_arguments)]
pub async fn run_server(
    config: AppConfig,
    tool_mode: ToolMode,
    enable_slack_tools: bool,
    _index_big_project: bool,
    disable_mcp_mtls: bool,
) -> Result<(), String> {
    match tool_mode {
        ToolMode::RemoteOnly | ToolMode::Combined => {
            // Placeholder for code indexing logic
        }
        ToolMode::LocalOnly => {}
    }

    let (bind_address, listener) = if let Ok(fixed_port) = std::env::var("STAKPAK_MCP_PORT") {
        let port: u16 = fixed_port
            .parse()
            .map_err(|e| format!("Invalid STAKPAK_MCP_PORT '{fixed_port}': {e}"))?;
        let host = if crate::utils::local_context::detect_container_environment() {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };
        let addr = format!("{host}:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind MCP server to {addr}: {e}"))?;
        (addr, listener)
    } else {
        network::find_available_bind_address_with_listener().await?
    };

    let (certificate_chain, server_tls_config) =
        if let Ok(trusted_client_ca_pem) = std::env::var(TRUSTED_CLIENT_CA_ENV) {
            // Sandbox mode: a parent process provided the client's CA cert.
            // Generate our own server identity and trust their CA. Only public
            // CA certificates cross the process boundary — no private keys.
            let server_identity = MtlsIdentity::generate_server()
                .map_err(|e| format!("Failed to generate server identity: {e}"))?;

            let tls_config = server_identity
                .create_server_config(&trusted_client_ca_pem)
                .map_err(|e| format!("Failed to create server TLS config: {e}"))?;

            // Output our server CA cert so the client can trust us.
            let server_ca_pem = server_identity
                .ca_cert_pem()
                .map_err(|e| format!("Failed to get server CA PEM: {e}"))?;

            println!("🔐 mTLS enabled - independent identity (sandbox mode)");
            println!("---BEGIN STAKPAK SERVER CA---");
            println!("{server_ca_pem}");
            println!("---END STAKPAK SERVER CA---");

            (None, Some(Arc::new(tls_config)))
        } else if !disable_mcp_mtls {
            match CertificateChain::generate() {
                Ok(chain) => {
                    println!("🔐 mTLS enabled - generated certificate chain");
                    if let Ok(ca_pem) = chain.get_ca_cert_pem() {
                        println!("📜 CA Certificate (copy this to your client):");
                        println!("{}", ca_pem);
                    }
                    (Some(chain), None)
                }
                Err(e) => {
                    eprintln!("Failed to generate certificate chain: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            (None, None)
        };

    let protocol = if !disable_mcp_mtls { "https" } else { "http" };
    println!("MCP server started at {}://{}/mcp", protocol, bind_address);
    println!(
        "⚠️  Secret redaction is handled by the proxy layer. Run behind 'stakpak mcp proxy' for secret protection."
    );

    start_server(
        MCPServerConfig {
            client: Some(get_client(&config).await?),
            enabled_tools: EnabledToolsConfig {
                slack: enable_slack_tools,
            },
            tool_mode,
            enable_subagents: true,
            bind_address,
            certificate_chain: Arc::new(certificate_chain),
            skill_directories: default_skill_directories(),
            subagent_config: SubagentConfig {
                profile_name: Some(config.profile_name.clone()),
                config_path: Some(config.config_path.clone()),
            },
            server_tls_config,
        },
        Some(listener),
        None,
    )
    .await
    .map_err(|e| e.to_string())
}
