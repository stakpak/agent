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
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let port = std::env::var("MCP_SERVER_PORT").unwrap_or_else(|_| "8420".to_string());
    let server_url = format!("http://127.0.0.1:{}", port);

    info!("Connecting to MCP server");
    info!("Server: {}", server_url);

    let http_client = Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    let transport = StreamableHttpClientTransport::with_client(
        http_client,
        StreamableHttpClientTransportConfig::with_uri(format!("{}/mcp", server_url)),
    );

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
                "with_symbols": true
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
