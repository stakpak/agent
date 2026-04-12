use std::collections::HashMap;

use clap::Subcommand;
use stakpak_mcp_server::ToolMode;

use crate::config::AppConfig;

use stakpak_mcp_config::{
    McpServerEntry, add_server, find_config_file, load_config, remove_server, resolve_config_path,
    save_config, set_server_disabled,
};

pub mod proxy;
pub mod server;

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
    /// Add an MCP server to config
    Add {
        /// Server name (unique identifier)
        name: String,

        /// Command for stdio transport
        #[arg(long)]
        command: Option<String>,

        /// Comma-separated arguments
        #[arg(long, allow_hyphen_values = true)]
        args: Option<String>,

        /// Environment variables (KEY=VALUE, repeatable)
        #[arg(long = "env")]
        envs: Vec<String>,

        /// HTTP URL for remote transport
        #[arg(long)]
        url: Option<String>,

        /// HTTP headers (KEY=VALUE, repeatable)
        #[arg(long = "headers")]
        headers: Vec<String>,

        /// JSON config string (alternative to --command/--url)
        #[arg(long)]
        json: Option<String>,

        /// Add in disabled state
        #[arg(long)]
        disabled: bool,

        /// Config file path
        #[arg(long = "config-file")]
        config_file: Option<String>,
    },
    /// Remove an MCP server from config
    Remove {
        /// Server name to remove
        name: String,

        /// Config file path
        #[arg(long = "config-file")]
        config_file: Option<String>,
    },
    /// List configured MCP servers
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Config file path
        #[arg(long = "config-file")]
        config_file: Option<String>,
    },
    /// Show details for a specific MCP server
    Get {
        /// Server name
        name: String,

        /// Config file path
        #[arg(long = "config-file")]
        config_file: Option<String>,
    },
    /// Enable a MCP server
    Enable {
        /// Server name to enable
        name: String,

        /// Config file path
        #[arg(long = "config-file")]
        config_file: Option<String>,
    },
    /// Disable an MCP server without removing it
    Disable {
        /// Server name to disable
        name: String,

        /// Config file path
        #[arg(long = "config-file")]
        config_file: Option<String>,
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
                    None => find_config_file()?,
                };
                proxy::run_proxy(config_path, disable_secret_redaction, privacy_mode).await
            }
            McpCommands::Add {
                name,
                command,
                args,
                envs,
                url,
                headers,
                json,
                disabled,
                config_file,
            } => {
                let entry = if let Some(json_str) = json {
                    let mut entry = serde_json::from_str::<McpServerEntry>(&json_str)
                        .map_err(|e| format!("Invalid JSON config: {e}"))?;
                    entry.set_disabled(disabled);
                    entry
                } else if let Some(url) = url {
                    let headers = parse_key_values(&headers)?;
                    McpServerEntry::UrlBased {
                        url,
                        headers: if headers.is_empty() {
                            None
                        } else {
                            Some(headers)
                        },
                        disabled,
                    }
                } else if let Some(command) = command {
                    let args = args
                        .map(|s| s.split(',').map(|a| a.trim().to_string()).collect())
                        .unwrap_or_default();
                    let env = parse_key_values(&envs)?;
                    McpServerEntry::CommandBased {
                        command,
                        args,
                        env: if env.is_empty() { None } else { Some(env) },
                        disabled,
                    }
                } else {
                    return Err(
                        "Must specify --command, --url, or --json. See 'stakpak mcp add --help'."
                            .to_string(),
                    );
                };

                let path = resolve_config_path(config_file.as_deref());
                let mut cfg = load_config(&path)?;
                add_server(&mut cfg, &name, entry)?;
                save_config(&cfg, &path)?;

                println!("Added MCP server '{name}' to {}", path.display());
                Ok(())
            }
            McpCommands::Remove { name, config_file } => {
                let path = resolve_config_path(config_file.as_deref());
                let mut cfg = load_config(&path)?;
                remove_server(&mut cfg, &name)?;
                save_config(&cfg, &path)?;

                println!("Removed MCP server '{name}'.");
                Ok(())
            }
            McpCommands::List { json, config_file } => {
                let path = resolve_config_path(config_file.as_deref());
                let cfg = load_config(&path)?;

                if cfg.servers.is_empty() {
                    println!("No MCP servers configured.");
                    return Ok(());
                }

                if json {
                    let output = serde_json::to_string_pretty(&cfg.servers)
                        .map_err(|e| format!("Failed to serialize: {e}"))?;
                    println!("{output}");
                    return Ok(());
                }

                let name_header = "NAME";
                let type_header = "TYPE";
                let cmd_header = "COMMAND/URL";
                let status_header = "STATUS";
                println!("{name_header:<20} {type_header:<8} {cmd_header:<50} {status_header}");
                for (name, entry) in &cfg.servers {
                    let status = if entry.is_disabled() {
                        "disabled"
                    } else {
                        "enabled"
                    };
                    println!(
                        "{:<20} {:<8} {:<50} {}",
                        name,
                        entry.entry_type(),
                        entry.summary_truncated(50),
                        status,
                    );
                }

                Ok(())
            }
            McpCommands::Get { name, config_file } => {
                let path = resolve_config_path(config_file.as_deref());
                let cfg = load_config(&path)?;

                let entry = cfg
                    .servers
                    .get(&name)
                    .ok_or_else(|| format!("Server '{name}' not found."))?;

                let json = serde_json::to_string_pretty(&entry)
                    .map_err(|e| format!("Failed to serialize: {e}"))?;
                println!("{json}");
                Ok(())
            }
            McpCommands::Enable { name, config_file } => {
                let path = resolve_config_path(config_file.as_deref());
                let mut cfg = load_config(&path)?;
                set_server_disabled(&mut cfg, &name, false)?;
                save_config(&cfg, &path)?;

                println!("Enabled MCP server '{name}'.");
                Ok(())
            }
            McpCommands::Disable { name, config_file } => {
                let path = resolve_config_path(config_file.as_deref());
                let mut cfg = load_config(&path)?;
                set_server_disabled(&mut cfg, &name, true)?;
                save_config(&cfg, &path)?;

                println!("Disabled MCP server '{name}'.");
                Ok(())
            }
        }
    }
}

/// Parse KEY=VALUE pairs from a vec of strings.
fn parse_key_values(pairs: &[String]) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for pair in pairs {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("Invalid KEY=VALUE pair: '{pair}'"))?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}
