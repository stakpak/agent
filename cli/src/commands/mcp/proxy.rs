use rmcp::{ServiceExt, transport::stdio};
use stakpak_mcp_proxy::{
    client::{ClientPoolConfig, ServerConfig},
    server::ProxyServer,
};
use std::collections::HashMap;

pub async fn run_proxy(disable_secret_redaction: bool, privacy_mode: bool) -> Result<(), String> {
    let mut servers: HashMap<String, ServerConfig> = HashMap::new();

    // Always add the stakpak MCP server as a stdio server
    servers.insert(
        "stakpak".to_string(),
        ServerConfig::Stdio {
            command: "cargo".to_string(),
            args: vec![
                "run".to_string(),
                "--".to_string(),
                "mcp".to_string(),
                "start".to_string(),
            ],
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
