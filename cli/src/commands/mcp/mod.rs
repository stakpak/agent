use std::path::PathBuf;

use clap::Subcommand;
use stakpak_mcp_server::ToolMode;

use crate::config::AppConfig;

pub mod proxy;
pub mod server;

fn find_mcp_proxy_config_file() -> Result<String, String> {
    // Priority 1: ~/.stakpak/mcp.{toml,json}
    let config_path = AppConfig::get_config_path::<&str>(None);
    if let Some(home_stakpak) = config_path.parent() {
        let home_toml = home_stakpak.join("mcp.toml");
        if home_toml.exists() {
            return Ok(home_toml.to_string_lossy().to_string());
        }

        let home_json = home_stakpak.join("mcp.json");
        if home_json.exists() {
            return Ok(home_json.to_string_lossy().to_string());
        }
    }

    // Priority 2: .stakpak/mcp.{toml,json} in current directory
    let cwd_stakpak = PathBuf::from(".stakpak");

    let cwd_stakpak_toml = cwd_stakpak.join("mcp.toml");
    if cwd_stakpak_toml.exists() {
        return Ok(cwd_stakpak_toml.to_string_lossy().to_string());
    }

    let cwd_stakpak_json = cwd_stakpak.join("mcp.json");
    if cwd_stakpak_json.exists() {
        return Ok(cwd_stakpak_json.to_string_lossy().to_string());
    }

    // Priority 3: mcp.{toml,json} in current directory (fallback)
    let cwd_toml = PathBuf::from("mcp.toml");
    if cwd_toml.exists() {
        return Ok("mcp.toml".to_string());
    }

    let cwd_json = PathBuf::from("mcp.json");
    if cwd_json.exists() {
        return Ok("mcp.json".to_string());
    }

    Err("No MCP proxy config file found. Searched in:\n  \
        1. ~/.stakpak/mcp.toml or ~/.stakpak/mcp.json\n  \
        2. .stakpak/mcp.toml or .stakpak/mcp.json\n  \
        3. mcp.toml or mcp.json\n\n\
        Create a config file with your MCP servers."
        .to_string())
}

#[derive(Subcommand, PartialEq)]
pub enum McpCommands {
    /// Start the MCP server (standalone HTTP/HTTPS server with tools)
    Start {
        /// Tool mode to use (local, remote, combined)
        #[arg(long, short = 'm', default_value_t = ToolMode::Combined)]
        tool_mode: ToolMode,

        /// Enable Slack tools (experimental)
        #[arg(long = "enable-slack-tools", default_value_t = false)]
        enable_slack_tools: bool,

        /// Allow indexing of large projects (more than 500 supported files)
        #[arg(long = "index-big-project", default_value_t = false)]
        index_big_project: bool,

        /// Disable mTLS (use plain HTTP instead of HTTPS)
        #[arg(long = "disable-mcp-mtls", default_value_t = false)]
        disable_mcp_mtls: bool,
    },
    /// Start the MCP proxy server (reads config from file, connects to external MCP servers)
    Proxy {
        /// Config file path
        #[arg(long = "config-file")]
        config_file: Option<String>,

        /// Disable secret redaction (WARNING: this will print secrets to the console)
        #[arg(long = "disable-secret-redaction", default_value_t = false)]
        disable_secret_redaction: bool,

        /// Enable privacy mode to redact private data like IP addresses and AWS account IDs
        #[arg(long = "privacy-mode", default_value_t = false)]
        privacy_mode: bool,
    },
}

impl McpCommands {
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
            McpCommands::Start {
                tool_mode,
                enable_slack_tools,
                index_big_project,
                disable_mcp_mtls,
            } => {
                server::run_server(
                    config,
                    tool_mode,
                    enable_slack_tools,
                    index_big_project,
                    disable_mcp_mtls,
                )
                .await
            }
            McpCommands::Proxy {
                config_file,
                disable_secret_redaction,
                privacy_mode,
            } => {
                let config_path = match config_file {
                    Some(path) => path,
                    None => find_mcp_proxy_config_file()?,
                };
                proxy::run_proxy(config_path, disable_secret_redaction, privacy_mode).await
            }
        }
    }
}
