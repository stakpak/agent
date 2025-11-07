use std::sync::Arc;

use crate::{
    // code_index::{get_or_build_local_code_index, start_code_index_watcher},
    config::AppConfig,
    utils::network,
};
use clap::Subcommand;
use rmcp::{ServiceExt, transport::stdio};
use serde::{Deserialize, Serialize};
use stakpak_api::{Client, ClientConfig};
use stakpak_mcp_proxy::{client::ClientPoolConfig, server::ProxyServer};
use stakpak_mcp_server::{EnabledToolsConfig, MCPServerConfig, ToolMode, start_server};

pub mod acp;
pub mod agent;
pub mod auto_update;
pub mod warden;

/// Frontmatter structure for rulebook metadata
#[derive(Deserialize, Serialize)]
struct RulebookFrontmatter {
    uri: String,
    description: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// Parse rulebook metadata from markdown content with YAML frontmatter
/// Expects frontmatter with uri, description, and tags
/// Returns (uri, description, tags, content_without_frontmatter)
fn parse_rulebook_metadata(content: &str) -> Result<(String, String, Vec<String>, String), String> {
    // Check if content starts with frontmatter (---)
    let content = content.trim_start();
    if !content.starts_with("---") {
        return Err("Rulebook file must start with YAML frontmatter (---) containing uri, description, and tags".into());
    }

    // Find the end of frontmatter
    let rest = &content[3..]; // Skip first "---"
    let end_pos = rest
        .find("\n---")
        .ok_or("Frontmatter must end with '---'")?;

    let frontmatter_yaml = &rest[..end_pos];

    // Parse YAML frontmatter
    let frontmatter: RulebookFrontmatter = serde_yaml::from_str(frontmatter_yaml)
        .map_err(|e| format!("Failed to parse YAML frontmatter: {}", e))?;

    // Extract content after frontmatter (skip the closing "---" and any leading whitespace)
    let content_body = rest[end_pos + 4..].trim_start().to_string();

    Ok((
        frontmatter.uri,
        frontmatter.description,
        frontmatter.tags,
        content_body,
    ))
}

#[derive(Subcommand, PartialEq)]
pub enum ConfigCommands {
    /// Show current configuration
    Show,
    /// Print a complete sample configuration file
    Sample,
}

#[derive(Subcommand, PartialEq)]
pub enum RulebookCommands {
    /// Get a specific rulebook or list all rulebooks
    Get {
        /// Rulebook URI (optional - if not provided, lists all rulebooks)
        uri: Option<String>,
    },
    /// Apply/create a rulebook from a markdown file
    Apply {
        /// Path to the markdown file containing the rulebook
        file_path: String,
    },
    /// Delete a rulebook
    Delete {
        /// Rulebook URI to delete
        uri: String,
    },
}

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

        /// Disable mTLS (WARNING: this will use unencrypted HTTP communication)
        #[arg(long = "disable-mcp-mtls", default_value_t = false)]
        disable_mcp_mtls: bool,
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

#[derive(Subcommand, PartialEq)]
pub enum Commands {
    /// Get CLI Version
    Version,
    /// Login to Stakpak
    Login {
        /// API key for authentication
        #[arg(long, env("STAKPAK_API_KEY"))]
        api_key: String,
    },

    /// Logout from Stakpak
    Logout,

    /// Start Agent Client Protocol server (for editor integration)
    ///
    Acp {
        /// Read system prompt from file
        #[arg(long = "system-prompt-file")]
        system_prompt_file: Option<String>,
    },

    /// Set configuration values
    Set {
        /// Set machine name for device identification
        #[arg(long = "machine-name")]
        machine_name: Option<String>,
        /// Enable or disable auto-appending .stakpak to .gitignore files
        #[arg(long = "auto-append-gitignore")]
        auto_append_gitignore: Option<bool>,
    },

    /// Configuration management commands
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Rulebook management commands
    #[command(subcommand, alias = "rb")]
    Rulebooks(RulebookCommands),

    /// Get current account
    Account,

    /// MCP commands
    #[command(subcommand)]
    Mcp(McpCommands),

    /// Stakpak Warden wraps coding agents to apply security policies and limit their capabilities
    Warden {
        /// Environment variables to pass to container
        #[arg(short, long, action = clap::ArgAction::Append)]
        env: Vec<String>,
        /// Additional volumes to mount
        #[arg(short, long, action = clap::ArgAction::Append)]
        volume: Vec<String>,
        #[command(subcommand)]
        command: Option<warden::WardenCommands>,
    },
    /// Update Stakpak Agent to the latest version
    Update,
}

impl Commands {
    pub fn requires_auth(&self) -> bool {
        !matches!(
            self,
            Commands::Login { .. }
                | Commands::Logout
                | Commands::Set { .. }
                | Commands::Config(_)
                | Commands::Version
                | Commands::Update
                | Commands::Acp { .. }
        )
    }
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
            Commands::Mcp(command) => match command {
                McpCommands::Start {
                    disable_secret_redaction,
                    privacy_mode,
                    tool_mode,
                    enable_slack_tools,
                    index_big_project: _,
                    disable_mcp_mtls,
                } => {
                    let _api_config: ClientConfig = config.clone().into();
                    match tool_mode {
                        ToolMode::RemoteOnly | ToolMode::Combined => {
                            // match get_or_build_local_code_index(
                            //     &api_config,
                            //     None,
                            //     index_big_project,
                            // )
                            // .await
                            // {
                            //     Ok(_) => {
                            //         // Indexing was successful, start the file watcher
                            //         tokio::spawn(async move {
                            //             match start_code_index_watcher(&api_config, None) {
                            //                 Ok(_) => {}
                            //                 Err(e) => {
                            //                     eprintln!(
                            //                         "Failed to start code index watcher: {}",
                            //                         e
                            //                     );
                            //                 }
                            //             }
                            //         });
                            //     }
                            //     Err(e)
                            //         if e.contains("threshold")
                            //             && e.contains("--index-big-project") =>
                            //     {
                            //         // This is the expected error when file count exceeds limit
                            //         // Continue silently without file watcher
                            //     }
                            //     Err(e) => {
                            //         eprintln!("Failed to build code index: {}", e);
                            //         // Continue without code indexing instead of exiting
                            //     }
                            // }
                        }
                        ToolMode::LocalOnly => {}
                    }

                    let (bind_address, listener) =
                        network::find_available_bind_address_with_listener().await?;

                    // Generate certificates if mTLS is enabled
                    let certificate_chain = if !disable_mcp_mtls {
                        match stakpak_shared::cert_utils::CertificateChain::generate() {
                            Ok(chain) => {
                                println!("🔐 mTLS enabled - generated certificate chain");
                                if let Ok(ca_pem) = chain.get_ca_cert_pem() {
                                    println!("📜 CA Certificate (copy this to your client):");
                                    println!("{}", ca_pem);
                                }
                                Some(chain)
                            }
                            Err(e) => {
                                eprintln!("Failed to generate certificate chain: {}", e);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        None
                    };

                    let protocol = if !disable_mcp_mtls { "https" } else { "http" };
                    println!("MCP server started at {}://{}/mcp", protocol, bind_address);

                    start_server(
                        MCPServerConfig {
                            api: config.into(),
                            redact_secrets: !disable_secret_redaction,
                            privacy_mode,
                            enabled_tools: EnabledToolsConfig {
                                slack: enable_slack_tools,
                            },
                            tool_mode,
                            subagent_configs: None, // MCP standalone mode doesn't need subagent configs
                            bind_address,
                            certificate_chain: Arc::new(certificate_chain),
                        },
                        Some(listener),
                        None,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                }
                McpCommands::Proxy {
                    config_file,
                    disable_secret_redaction,
                    privacy_mode,
                } => {
                    let config = match ClientPoolConfig::from_toml_file(
                        config_file.as_deref().unwrap_or("mcp.toml"),
                    ) {
                        Ok(config) => config,
                        Err(_) => ClientPoolConfig::from_json_file(
                            config_file.as_deref().unwrap_or("mcp.json"),
                        )
                        .map_err(|e| e.to_string())?,
                    };

                    let server = ProxyServer::new(config, !disable_secret_redaction, privacy_mode)
                        .serve(stdio())
                        .await
                        .map_err(|e| e.to_string())?;

                    server.waiting().await.map_err(|e| e.to_string())?;
                }
            },
            Commands::Login { api_key } => {
                let mut updated_config = config.clone();
                updated_config.api_key = Some(api_key);

                updated_config
                    .save()
                    .map_err(|e| format!("Failed to save config: {}", e))?;
            }
            Commands::Logout => {
                let mut updated_config = config.clone();
                updated_config.api_key = None;

                updated_config
                    .save()
                    .map_err(|e| format!("Failed to save config: {}", e))?;
            }
            Commands::Set {
                machine_name,
                auto_append_gitignore,
            } => {
                let mut updated_config = config.clone();
                let mut config_updated = false;

                if let Some(name) = machine_name {
                    updated_config.machine_name = Some(name.clone());
                    config_updated = true;
                    println!("Machine name set to: {}", name);
                }

                if let Some(append) = auto_append_gitignore {
                    updated_config.auto_append_gitignore = Some(append);
                    config_updated = true;
                    println!("Auto-appending .stakpak to .gitignore: {}", append);
                }

                if config_updated {
                    updated_config
                        .save()
                        .map_err(|e| format!("Failed to save config: {}", e))?;
                } else {
                    println!("No configuration option provided. Available options:");
                    println!(
                        "  --machine-name <name>        Set machine name for device identification"
                    );
                    println!(
                        "  --auto-append-gitignore <bool>  Enable/disable auto-appending .stakpak to .gitignore"
                    );
                }
            }
            Commands::Config(config_command) => match config_command {
                ConfigCommands::Show => {
                    println!("Current configuration:");
                    println!("  Profile: {}", config.profile_name);
                    println!(
                        "  Machine name: {}",
                        config.machine_name.as_deref().unwrap_or("(not set)")
                    );
                    println!(
                        "  Auto-append .stakpak to .gitignore: {}",
                        config.auto_append_gitignore.unwrap_or(true)
                    );
                    println!("  API endpoint: {}", config.api_endpoint);
                    let api_key_display = match &config.api_key {
                        Some(key) if !key.is_empty() => "***".to_string(),
                        _ => "(not set)".to_string(),
                    };
                    println!("  API key: {}", api_key_display);
                }
                ConfigCommands::Sample => {
                    print_sample_config();
                }
            },
            Commands::Rulebooks(rulebook_command) => {
                let client = Client::new(&config.into()).map_err(|e| e.to_string())?;
                match rulebook_command {
                    RulebookCommands::Get { uri } => {
                        if let Some(uri) = uri {
                            // Get specific rulebook and output in apply-compatible format
                            let rulebook = client.get_rulebook_by_uri(&uri).await?;

                            // Create frontmatter struct
                            let frontmatter = RulebookFrontmatter {
                                uri: rulebook.uri,
                                description: rulebook.description,
                                tags: rulebook.tags,
                            };

                            // Serialize frontmatter to YAML
                            let yaml = serde_yaml::to_string(&frontmatter)
                                .map_err(|e| format!("Failed to serialize frontmatter: {}", e))?;

                            // Output in apply-compatible format with YAML frontmatter
                            println!("---");
                            print!("{}", yaml.trim());
                            println!("\n---");
                            println!("{}", rulebook.content);
                        } else {
                            // List all rulebooks
                            let rulebooks = client.list_rulebooks().await?;
                            if rulebooks.is_empty() {
                                println!("No rulebooks found.");
                            } else {
                                println!("Rulebooks:\n");
                                for rb in rulebooks {
                                    println!("  - URI: {}", rb.uri);
                                    println!("    Description: {}", rb.description);
                                    println!("    Tags: {}", rb.tags.join(", "));
                                    println!("    Visibility: {:?}", rb.visibility);
                                }
                            }
                        }
                    }
                    RulebookCommands::Apply { file_path } => {
                        // Read the markdown file
                        let content = std::fs::read_to_string(file_path)
                            .map_err(|e| format!("Failed to read file: {}", e))?;

                        // Parse frontmatter to extract metadata and content body
                        let (uri, description, tags, content_body) =
                            parse_rulebook_metadata(&content)?;

                        // Create the rulebook with content body (without frontmatter)
                        client
                            .create_rulebook(&uri, &description, &content_body, tags, None)
                            .await?;

                        println!("✓ Rulebook created/updated successfully");
                        println!("  URI: {}", uri);
                    }
                    RulebookCommands::Delete { uri } => {
                        client.delete_rulebook(&uri).await?;
                        println!("✓ Rulebook deleted: {}", uri);
                    }
                }
            }
            Commands::Account => {
                let client = Client::new(&(config.into())).map_err(|e| e.to_string())?;
                let data = client.get_my_account().await?;
                println!("{}", data.to_text());
            }
            Commands::Version => {
                println!(
                    "stakpak v{} (https://github.com/stakpak/agent)",
                    env!("CARGO_PKG_VERSION")
                );
            }
            Commands::Warden {
                env,
                volume,
                command,
            } => {
                match command {
                    Some(warden_command) => {
                        warden::WardenCommands::run(warden_command, config).await?;
                    }
                    None => {
                        // Default behavior: run warden with preconfigured setup
                        warden::run_default_warden(config, volume, env).await?;
                    }
                }
            }
            Commands::Update => {
                auto_update::run_auto_update().await?;
            }
            Commands::Acp { system_prompt_file } => {
                let system_prompt = if let Some(system_prompt_file_path) = &system_prompt_file {
                    match std::fs::read_to_string(system_prompt_file_path) {
                        Ok(content) => {
                            println!(
                                "📖 Reading system prompt from file: {}",
                                system_prompt_file_path
                            );
                            Some(content.trim().to_string())
                        }
                        Err(e) => {
                            eprintln!(
                                "Failed to read system prompt file '{}': {}",
                                system_prompt_file_path, e
                            );
                            None
                        }
                    }
                } else {
                    None
                };
                // Start ACP agent
                let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
                let agent =
                    match crate::commands::acp::StakpakAcpAgent::new(config, tx, system_prompt)
                        .await
                    {
                        Ok(agent) => agent,
                        Err(e) => {
                            eprintln!("Failed to create ACP agent: {}", e);
                            std::process::exit(1);
                        }
                    };

                if let Err(e) = agent.run_stdio().await {
                    eprintln!("ACP agent failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Ok(())
    }
}

fn print_sample_config() {
    println!(
        r#"# Stakpak Configuration File

# Profile-based configuration allows different settings for different environments
[profiles]

# Special 'all' profile - settings that apply to ALL profiles as defaults
# Individual profiles can override these settings
[profiles.all]
api_endpoint = "https://apiv2.stakpak.dev"
# Common tools that should be available across all profiles
allowed_tools = ["view", "search_docs", "read_rulebook", "local_code_search"]
# Conservative auto-approve list that works for all environments
auto_approve = ["view", "search_docs", "read_rulebook"]

[profiles.all.rulebooks]
# Common rulebook patterns for all profiles
include = ["stakpak://yourdomain.com/common/**"]
exclude = ["stakpak://yourdomain.com/archive/**"]
include_tags = ["common", "shared"]
exclude_tags = ["archived", "obsolete"]

# Default profile - used when no specific profile is selected
# Inherits from 'all' profile and can override specific settings
[profiles.default]
api_key = "your_api_key_here"

# Extends the 'all' profile's allowed_tools with additional development tools
allowed_tools = ["view", "search_docs", "read_rulebook", "local_code_search", "create", "str_replace", "run_command"]

# Inherits auto_approve from 'all' profile (view, search_docs, read_rulebook)
# No need to redefine unless you want to override

# Rulebook filtering configuration
[profiles.default.rulebooks]
# URI patterns to include (supports glob patterns like * and **)
include = ["stakpak://yourdomain.com/*", "stakpak://**/*.md"]

# URI patterns to exclude (supports glob patterns)
exclude = ["stakpak://restricted.domain.com/**"]

# Tags to include - only rulebooks with these tags will be loaded
include_tags = ["terraform", "kubernetes", "security"]

# Tags to exclude - rulebooks with these tags will be filtered out
exclude_tags = ["deprecated", "experimental"]

# Warden (runtime security) configuration
# When enabled, the main 'stakpak' command will automatically run with Warden security enforcer
# This provides isolation and security policies for the agent execution
[profiles.default.warden]
enabled = true
volumes = [
    # working directory
    "./:/agent:ro",

    # cloud credentials (read-only)
    "~/.aws:/home/agent/.aws:ro",
    "~/.config/gcloud:/home/agent/.config/gcloud:ro",
    "~/.digitalocean:/home/agent/.digitalocean:ro",
    "~/.azure:/home/agent/.azure:ro",
    "~/.kube:/home/agent/.kube:ro",
]

# Production profile - stricter settings for production environments
# Inherits from 'all' profile but restricts tools for safety
[profiles.production]
api_key = "prod_api_key_here"

# Restricts allowed_tools to only read-only operations (overrides 'all' profile)
allowed_tools = ["view", "search_docs", "read_rulebook"]

# Uses the same conservative auto_approve list from 'all' profile
# No need to redefine since 'all' profile already has safe defaults

[profiles.production.rulebooks]
# Only include production-ready rulebooks
include = ["stakpak://yourdomain.com/prod/**"]
exclude = ["stakpak://yourdomain.com/dev/**", "stakpak://yourdomain.com/test/**"]
include_tags = ["production", "stable"]
exclude_tags = ["dev", "test", "experimental"]

# Development profile - more permissive settings for development
# Inherits from 'all' profile and extends with development-specific tools
[profiles.development]
api_key = "dev_api_key_here"

# Extends 'all' profile's allowed_tools with write operations for development
allowed_tools = ["view", "search_docs", "read_rulebook", "local_code_search", "create", "str_replace", "run_command"]

# Extends 'all' profile's auto_approve with additional development tools
auto_approve = ["view", "search_docs", "read_rulebook", "create"]

[profiles.development.rulebooks]
# Include development and test rulebooks
include = ["stakpak://yourdomain.com/dev/**", "stakpak://yourdomain.com/test/**"]
exclude = []
include_tags = ["dev", "test", "experimental"]
exclude_tags = []

# Global settings that apply to all profiles
[settings]
# Machine name for device identification
machine_name = "my-development-machine"

# Automatically append .stakpak to .gitignore files
auto_append_gitignore = true
"#
    );
}
