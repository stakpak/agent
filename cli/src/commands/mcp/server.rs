use std::sync::Arc;

use stakpak_mcp_server::{EnabledToolsConfig, MCPServerConfig, ToolMode, start_server_stdio};

use crate::{commands::get_client, config::AppConfig};

pub async fn run_server(
    config: AppConfig,
    disable_secret_redaction: bool,
    privacy_mode: bool,
    tool_mode: ToolMode,
    enable_slack_tools: bool,
    _index_big_project: bool,
) -> Result<(), String> {
    match tool_mode {
        ToolMode::RemoteOnly | ToolMode::Combined => {
            // Placeholder for code indexing logic
        }
        ToolMode::LocalOnly => {}
    }

    start_server_stdio(
        MCPServerConfig {
            client: Some(get_client(&config).await?),
            redact_secrets: !disable_secret_redaction,
            privacy_mode,
            enabled_tools: EnabledToolsConfig {
                slack: enable_slack_tools,
            },
            tool_mode,
            subagent_configs: None,
            bind_address: "stdio".to_string(),
            certificate_chain: Arc::new(None),
        },
        None,
    )
    .await
    .map_err(|e| e.to_string())
}
