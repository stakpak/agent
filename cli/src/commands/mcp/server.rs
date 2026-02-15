use std::sync::Arc;

use stakpak_mcp_server::{EnabledToolsConfig, MCPServerConfig, SubagentConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::CertificateChain;

use crate::utils::network;
use crate::{commands::get_client, config::AppConfig};

/// Start the MCP server (standalone HTTP/HTTPS server with tools)
pub async fn run_server(
    config: AppConfig,
    disable_secret_redaction: bool,
    privacy_mode: bool,
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

    let (bind_address, listener) = network::find_available_bind_address_with_listener().await?;

    let certificate_chain = if !disable_mcp_mtls {
        match CertificateChain::generate() {
            Ok(chain) => {
                println!("🔐 mTLS enabled - generated certificate chain");
                if let Ok(ca_pem) = chain.get_ca_cert_pem() {
                    println!("📜 CA Certificate (copy this to your client):");
                    println!("{}", ca_pem);
                }
                Some(chain)
            }
            Err(e) => {
                eprintln!("Failed to generate certificate chain: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let protocol = if !disable_mcp_mtls { "https" } else { "http" };
    println!("MCP server started at {}://{}/mcp", protocol, bind_address);

    start_server(
        MCPServerConfig {
            client: Some(get_client(&config).await?),
            redact_secrets: !disable_secret_redaction,
            privacy_mode,
            enabled_tools: EnabledToolsConfig {
                slack: enable_slack_tools,
            },
            tool_mode,
            enable_subagents: true,
            bind_address,
            certificate_chain: Arc::new(certificate_chain),
            subagent_config: SubagentConfig {
                profile_name: Some(config.profile_name.clone()),
                config_path: Some(config.config_path.clone()),
            },
        },
        Some(listener),
        None,
    )
    .await
    .map_err(|e| e.to_string())
}
