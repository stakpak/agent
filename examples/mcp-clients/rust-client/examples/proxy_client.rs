use anyhow::{Context, Result};
use rmcp::{
    model::{ClientCapabilities, ClientInfo, Implementation},
    service::RunningService,
    transport::TokioChildProcess,
    ClientHandler, RoleClient, ServiceExt,
};
use tokio::process::Command;
use tokio::time::{sleep, Duration};
use tracing::info;

/// Handler for progress notifications from the MCP proxy
#[derive(Clone)]
struct ProxyClientHandler;

impl ClientHandler for ProxyClientHandler {
    async fn on_progress(
        &self,
        progress: rmcp::model::ProgressNotificationParam,
        _ctx: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        if let Some(message) = &progress.message {
            info!(
                "Progress [{}%]: {}",
                progress.progress * 100.0 / progress.total.unwrap_or(100.0),
                message
            );
        }
    }

    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "stakpak-proxy-client".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("Stakpak MCP Proxy Client Example".to_string()),
                icons: Some(vec![]),
                website_url: Some("https://stakpak.dev".to_string()),
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Get the stakpak binary path (defaults to "stakpak" in PATH)
    let stakpak_bin = std::env::var("STAKPAK_BIN").unwrap_or_else(|_| "stakpak".to_string());

    info!("Starting MCP proxy via: {} mcp proxy", stakpak_bin);
    info!("The proxy will read config from ~/.stakpak/mcp.toml (or mcp.json)");

    // Spawn the MCP proxy as a child process
    let mut cmd = Command::new(&stakpak_bin);
    cmd.arg("mcp").arg("proxy");

    // Optional: specify a custom config file
    // cmd.arg("--config-file").arg("/path/to/mcp.toml");

    let proc = TokioChildProcess::new(cmd).context("Failed to spawn stakpak mcp proxy")?;

    // Connect to the proxy via stdio
    let client_handler = ProxyClientHandler;
    let service: RunningService<RoleClient, ProxyClientHandler> = client_handler
        .serve(proc)
        .await
        .context("Failed to initialize MCP proxy connection")?;

    info!("Connected to MCP proxy successfully!");

    if let Some(server_info) = service.peer_info() {
        info!("Proxy server info: {:?}", server_info.server_info);
    }

    // Wait for upstream servers to initialize
    // The proxy connects to upstream MCP servers asynchronously
    info!("Waiting for upstream servers to initialize...");
    sleep(Duration::from_secs(2)).await;

    // List all tools available through the proxy with retry logic
    info!("Getting available tools from MCP proxy (aggregated from all configured servers)");

    let mut tools = Vec::new();
    for attempt in 1..=3 {
        let response = service
            .list_tools(Default::default())
            .await
            .context("Failed to list tools")?;
        tools = response.tools;

        if !tools.is_empty() {
            break;
        }

        if attempt < 3 {
            info!(
                "No tools found yet, retrying in 1 second... (attempt {}/3)",
                attempt
            );
            sleep(Duration::from_secs(1)).await;
        }
    }

    info!("Available tools ({} total):", tools.len());
    for tool in &tools {
        info!(
            "- {} : {}",
            tool.name,
            tool.description.as_deref().unwrap_or("No description")
        );
    }

    if tools.is_empty() {
        info!(
            "No tools available. Make sure you have MCP servers configured in ~/.stakpak/mcp.toml"
        );
        return Ok(());
    }

    // Example: Try calling a filesystem tool if available
    // Note: The proxy prefixes tool names with "{server_name}__" (e.g., "filesystem__read_file")
    if tools.iter().any(|t| t.name == "filesystem__read_file") {
        info!("\n=== Calling filesystem__read_file tool ===");

        use rmcp::model::CallToolRequestParam;

        let tool_params = CallToolRequestParam {
            name: "filesystem__read_file".into(),
            arguments: Some(rmcp::object!({
                "path": "/etc/hostname"
            })),
        };

        info!("Reading /etc/hostname");

        match service.call_tool(tool_params).await {
            Ok(result) => {
                info!("Tool call successful!");
                for content_item in &result.content {
                    if let Some(text) = content_item.as_text() {
                        info!("Content: {}", text.text);
                    }
                }
            }
            Err(e) => {
                info!("Failed to call tool: {:?}", e);
            }
        }
    }

    // Example: Try Stakpak's generate_password tool if available
    // This requires adding Stakpak's MCP server to your proxy config:
    //   [mcpServers.stakpak]
    //   url = "https://127.0.0.1:8420/mcp"
    if tools.iter().any(|t| t.name == "stakpak__generate_password") {
        info!("\n=== Calling stakpak__generate_password tool ===");

        use rmcp::model::CallToolRequestParam;

        let tool_params = CallToolRequestParam {
            name: "stakpak__generate_password".into(),
            arguments: Some(rmcp::object!({
                "length": 16,
                "with_symbols": true
            })),
        };

        info!("Generating password (length=16, with symbols)");

        match service.call_tool(tool_params).await {
            Ok(result) => {
                info!("Tool call successful!");
                for content_item in &result.content {
                    if let Some(text) = content_item.as_text() {
                        info!("Generated password: {}", text.text);
                    }
                }
            }
            Err(e) => {
                info!("Failed to call tool: {:?}", e);
            }
        }
    }

    info!("\nDone! The proxy will shut down when this client exits.");

    Ok(())
}
