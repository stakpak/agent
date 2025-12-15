use rmcp::{ServiceExt, transport::stdio};
use stakpak_mcp_proxy::{
    client::{ClientPoolConfig, ServerConfig},
    server::ProxyServer,
};
use std::collections::HashMap;

pub async fn run_proxy(disable_secret_redaction: bool, privacy_mode: bool) -> Result<(), String> {
    let mut servers: HashMap<String, ServerConfig> = HashMap::new();

    // Get the path to the current executable for spawning the MCP server
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to get current executable path: {}", e))?
        .to_string_lossy()
        .to_string();

    // Always add the stakpak MCP server as a stdio server
    servers.insert(
        "stakpak".to_string(),
        ServerConfig::Stdio {
            command: current_exe,
            args: vec!["mcp".to_string(), "start".to_string()],
            env: None,
        },
    );

    let config = ClientPoolConfig::with_servers(servers);

    let server = ProxyServer::new(config, !disable_secret_redaction, privacy_mode)
        .serve(stdio())
        .await
        .map_err(|e| e.to_string())?;

    server
        .waiting()
        .await
        .map_err(|e| e.to_string())
        .map(|_| ())
}
