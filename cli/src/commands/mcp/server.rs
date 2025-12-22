use std::path::PathBuf;
use std::sync::Arc;

use stakpak_mcp_server::{EnabledToolsConfig, MCPServerConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::{CertificateChain, CertificateStrategy};
use tokio::net::TcpListener;

use crate::utils::network;
use crate::{commands::get_client, config::AppConfig};

pub struct ServerOptions {
    pub certs_config_dir: Option<PathBuf>,
    pub port: Option<u16>,
    pub disable_secret_redaction: bool,
    pub privacy_mode: bool,
    pub tool_mode: ToolMode,
    pub enable_slack_tools: bool,
    pub _index_big_project: bool,
    pub disable_mcp_mtls: bool,
}

pub async fn setup_certificates(out_dir: Option<PathBuf>, force: bool) -> Result<(), String> {
    println!("Stakpak MCP Certificate Setup\n");

    let cert_dir = out_dir
        .or_else(|| stakpak_shared::cert_utils::default_cert_dir().ok())
        .ok_or_else(|| "Could not determine certificate directory".to_string())?;

    if CertificateChain::exists_in_directory(&cert_dir) && !force {
        println!("Certificates already exist at: {}", cert_dir.display());
        println!("Options:");
        println!("1. Use existing certificates: `cargo run -- mcp start`");
        println!("2. Regenerate with --force: `cargo run -- mcp setup --force`");
        println!("3. Delete manually `rm -rf {}`", cert_dir.display());
        return Err("Certificates already exist".to_string());
    }

    match CertificateChain::generate_and_save(Some(&cert_dir), force) {
        Ok(_chain) => {
            println!("Certificate generation complete!\n");
            println!("Certificates saved to: {}", cert_dir.display());
            println!("Files created:");
            println!("ca.pem");
            println!("server-cert.pem");
            println!("server-key.pem");
            println!("client-cert.pem");
            println!("client-key.pem");
            println!();
            println!("Next steps:");
            println!("1. Start the server:  `cargo run -- mcp start`");
            println!(
                "2. Configure clients with the certificates from: {}",
                cert_dir.display()
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to generate certificates: {}", e);
            Err(format!("Certificate generation failed: {}", e))
        }
    }
}

/// Start the MCP server (standalone HTTP/HTTPS server with tools)
pub async fn run_server(config: AppConfig, options: ServerOptions) -> Result<(), String> {
    match options.tool_mode {
        ToolMode::RemoteOnly | ToolMode::Combined => {
            // Placeholder for code indexing logic
        }
        ToolMode::LocalOnly => {}
    }

    // Bind to port (custom port or auto-select)
    let listener = if let Some(port) = options.port {
        TcpListener::bind(format!("0.0.0.0:{}", port))
            .await
            .map_err(|e| format!("Failed to bind to port {}: {}", port, e))?
    } else {
        network::find_available_bind_address_with_listener()
            .await?
            .1
    };
    let bind_address = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?
        .to_string();

    // Load persisted certificates
    let (certificate_chain, cert_dir) = if !options.disable_mcp_mtls {
        let cert_dir = options
            .certs_config_dir
            .or_else(|| stakpak_shared::cert_utils::default_cert_dir().ok())
            .ok_or_else(|| "Could not determine certificate directory".to_string())?;

        let strategy = CertificateStrategy::Persistent(cert_dir.clone());

        // Check if certificates exist
        if !strategy.exists() {
            eprintln!("No certificates found at: {}", cert_dir.display());
            eprintln!("Please run the setup command first: `cargo run -- mcp setup`");
            eprintln!("Or disable mTLS: `cargo run -- mcp start --disable-mcp-mtls`");
            return Err("Certificates not found, please run 'mcp setup' first".to_string());
        }

        println!(
            "🔐 mTLS enabled - loading certificates from disk: {}",
            cert_dir.display()
        );

        // Verify certificates can be loaded
        if let Err(e) = CertificateChain::load_server_config(&cert_dir) {
            eprintln!("Failed to load certificates: {}", e);
            eprintln!(
                "Run 'cargo run -- mcp setup' to generate certificates, if there's not generated certificates"
            );
            return Err(format!("Failed to load certificates: {}", e));
        }

        println!("Client certificates available at: {}/", cert_dir.display());
        (None, Some(cert_dir))
    } else {
        println!("mTLS disabled");
        (None, None)
    };

    let protocol = if cert_dir.is_some() { "https" } else { "http" };
    println!("MCP server started at {}://{}/mcp", protocol, bind_address);

    start_server(
        MCPServerConfig {
            client: Some(get_client(&config).await?),
            redact_secrets: !options.disable_secret_redaction,
            privacy_mode: options.privacy_mode,
            enabled_tools: EnabledToolsConfig {
                slack: options.enable_slack_tools,
            },
            tool_mode: options.tool_mode,
            subagent_configs: None,
            bind_address,
            certificate_chain: Arc::new(certificate_chain),
            cert_dir,
        },
        Some(listener),
        None,
    )
    .await
    .map_err(|e| e.to_string())
}
