use anyhow::{Context, Result};
use reqwest::Client;
use rmcp::{
    model::{ClientCapabilities, ClientInfo, Implementation},
    service::RunningService,
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    RoleClient, ServiceExt,
};
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use tracing::info;
use tracing_subscriber;

fn load_certs(path: &Path) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let file = File::open(path).context(format!("Failed to open cert file: {:?}", path))?;
    let mut reader = BufReader::new(file);
    let certs: Vec<_> = certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse certificates")?;
    Ok(certs)
}

fn load_private_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let file = File::open(path).context(format!("Failed to open key file: {:?}", path))?;
    let mut reader = BufReader::new(file);
    let keys = pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse private key")?;

    if keys.is_empty() {
        anyhow::bail!("No private key found in file");
    }

    Ok(rustls::pki_types::PrivateKeyDer::Pkcs8(keys[0].clone_key()))
}

fn create_mtls_client_config(
    ca_cert_path: &Path,
    client_cert_path: &Path,
    client_key_path: &Path,
) -> Result<ClientConfig> {
    // Install default crypto provider if not already installed
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let ca_certs = load_certs(ca_cert_path)?;
    let mut root_cert_store = RootCertStore {
        roots: Vec::with_capacity(ca_certs.len()),
    };
    for cert in ca_certs {
        root_cert_store
            .add(cert)
            .context("Failed to add CA cert to root store")?;
    }

    let client_certs = load_certs(client_cert_path)?;
    let client_key = load_private_key(client_key_path)?;

    let config = ClientConfig::builder()
        .with_root_certificates(root_cert_store)
        .with_client_auth_cert(client_certs, client_key)
        .context("Failed to build client config with mTLS")?;

    Ok(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let certs_dir = std::env::var("CERTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".stakpak").join("certs"))
                .expect("HOME environment variable not set")
        });

    let port = std::env::var("MCP_SERVER_PORT").unwrap_or_else(|_| "8420".to_string());
    let server_url = format!("https://127.0.0.1:{}", port);

    let ca_cert = certs_dir.join("ca.pem");
    let client_cert = certs_dir.join("client-cert.pem");
    let client_key = certs_dir.join("client-key.pem");

    info!("Connecting to MCP server with mTLS");
    info!("Server: {}", server_url);
    info!("Certificates directory: {}", certs_dir.display());

    let tls_config = create_mtls_client_config(&ca_cert, &client_cert, &client_key)
        .context("Failed to create mTLS configuration")?;

    let http_client = Client::builder()
        .use_preconfigured_tls(tls_config)
        .build()
        .context("Failed to build HTTP client")?;

    let transport = StreamableHttpClientTransport::with_client(
        http_client,
        StreamableHttpClientTransportConfig::with_uri(format!("{}/mcp", server_url)),
    );

    // Initialize MCP client
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "stakpak-rust-client".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            title: Some("Stakpak Rust MCP Client".to_string()),
            icons: Some(vec![]),
            website_url: Some("https://stakpak.dev".to_string()),
        },
    };

    let service: RunningService<RoleClient, rmcp::model::InitializeRequestParam> = client_info
        .serve(transport)
        .await
        .context("Failed to initialize MCP service")?;

    info!("Connected to MCP server successfully!");

    if let Some(server_info) = service.peer_info() {
        info!("Server info: {:?}", server_info.server_info);
    }

    info!("Getting available tools from MCP server");
    let response = service
        .list_tools(Default::default())
        .await
        .context("Failed to list tools")?;

    let tools = response.tools;

    info!("Available tools:");
    for tool in &tools {
        info!(
            "- {} : {}",
            tool.name,
            tool.description.as_deref().unwrap_or("No description")
        );
    }

    if tools.is_empty() {
        info!("No tools available from server");
        return Ok(());
    }

    if tools.iter().any(|t| t.name == "generate_password") {
        info!("\n=== Calling generate_password tool ===");

        use rmcp::model::CallToolRequestParam;

        let tool_params = CallToolRequestParam {
            name: "generate_password".into(),
            arguments: Some(rmcp::object!({
                "length": 20,
                "no_symbols": false
            })),
        };

        info!("Requesting password generation (length=20, with symbols)");

        match service.call_tool(tool_params).await {
            Ok(result) => {
                info!("Tool call successful!");
                info!("Result: {:?}", result);

                if !result.content.is_empty() {
                    info!("Generated password:");
                    for content_item in &result.content {
                        let x = content_item.as_text().unwrap().text.to_string();
                        info!("{}", x);
                    }
                }
            }
            Err(e) => {
                info!("Failed to call tool: {:?}", e);
            }
        }
    } else {
        info!("generate_password tool is not available on this server");
    }

    Ok(())
}
