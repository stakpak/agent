use clap::Subcommand;
use stakpak_mcp_server::ToolMode;

use crate::config::AppConfig;

pub mod proxy;
pub mod server;

#[derive(Subcommand, PartialEq)]
pub enum McpCommands {
    /// Start the MCP server
    Start {
        /// Disable secret redaction (WARNING: this will print secrets to the console)
        #[arg(long = "disable-secret-redaction", default_value_t = false)]
        disable_secret_redaction: bool,

        /// Enable privacy mode to redact private data like IP addresses and AWS account IDs
        #[arg(long = "privacy-mode", default_value_t = false)]
        privacy_mode: bool,

        /// Tool mode to use (local, remote, combined)
        #[arg(long, short = 'm', default_value_t = ToolMode::Combined)]
        tool_mode: ToolMode,

        /// Enable Slack tools (experimental)
        #[arg(long = "enable-slack-tools", default_value_t = false)]
        enable_slack_tools: bool,

        /// Allow indexing of large projects (more than 500 supported files)
        #[arg(long = "index-big-project", default_value_t = false)]
        index_big_project: bool,
    },
    /// Start the MCP proxy server
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
                disable_secret_redaction,
                privacy_mode,
                tool_mode,
                enable_slack_tools,
                index_big_project,
            } => {
                server::run_server(
                    config,
                    disable_secret_redaction,
                    privacy_mode,
                    tool_mode,
                    enable_slack_tools,
                    index_big_project,
                )
                .await
            }
            McpCommands::Proxy {
                config_file: _,
                disable_secret_redaction,
                privacy_mode,
            } => proxy::run_proxy(disable_secret_redaction, privacy_mode).await,
        }
    }
}
