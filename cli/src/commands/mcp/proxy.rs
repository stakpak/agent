use rmcp::{ServiceExt, transport::stdio};
use stakpak_mcp_proxy::{client::ClientPoolConfig, server::ProxyServer};

pub async fn run_proxy(
    config_path: String,
    disable_secret_redaction: bool,
    privacy_mode: bool,
) -> Result<(), String> {
    let config = match ClientPoolConfig::from_toml_file(&config_path) {
        Ok(config) => config,
        Err(_) => ClientPoolConfig::from_json_file(&config_path)
            .map_err(|e| format!("Failed to load config from {}: {}", config_path, e))?,
    };

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
