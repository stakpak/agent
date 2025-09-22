use std::sync::Arc;

use crate::{
    code_index::{get_or_build_local_code_index, start_code_index_watcher},
    config::AppConfig,
    utils::network,
};
use agent::AgentCommands;
use clap::Subcommand;
use flow::{clone, get_flow_ref, push};
use stakpak_api::{
    Client, ClientConfig,
    models::{Document, ProvisionerType, TranspileTargetProvisionerType},
};
use stakpak_mcp_server::{MCPServerConfig, ToolMode, start_server};
use termimad::MadSkin;
use walkdir::WalkDir;

pub mod acp;
pub mod agent;
pub mod auto_update;
pub mod flow;
pub mod warden;

#[derive(Subcommand, PartialEq)]
pub enum ConfigCommands {
    /// Show current configuration
    Show,
    /// Print a complete sample configuration file
    Sample,
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

    /// Get current account
    Account,

    /// List my flows
    List,

    /// Get a flow
    Get {
        /// Flow reference in format: <owner_name>/<flow_name>
        flow_ref: String,
    },

    /// Clone configurations from a flow
    Clone {
        /// Flow reference in format: <owner_name>/<flow_name>(/<version_id_or_tag>)?
        #[arg(name = "flow-ref")]
        flow_ref: String,
        /// Destination directory
        #[arg(long, short)]
        dir: Option<String>,
    },

    /// Query your configurations
    Query {
        /// Query string to search/prompt for over your flows
        query: String,
        /// Limit the query to a specific flow reference in format: <owner_name>/<flow_name>/<version_id_or_tag>
        #[arg(long, short)]
        flow_ref: Option<String>,
        /// Re-generate the semantic query used to find code blocks with natural language
        #[arg(long, short)]
        generate_query: bool,
        /// Synthesize output with an LLM into a custom response
        #[arg(long, short = 'o')]
        synthesize_output: bool,
    },

    /// Push configurations to a flow
    Push {
        /// Flow reference in format: <owner_name>/<flow_name>(/<version_id_or_tag>)?
        #[arg(name = "flow-ref")]
        flow_ref: String,
        /// Create a new index
        #[arg(long, short, default_value_t = false)]
        create: bool,
        /// Source directory
        #[arg(long, short)]
        dir: Option<String>,
        /// Ignore delete operations
        #[arg(long, default_value_t = false)]
        ignore_delete: bool,
        /// Auto approve all changes
        #[arg(long, short = 'y', default_value_t = false)]
        auto_approve: bool,
    },

    /// Transpile configurations
    Transpile {
        /// Source directory
        #[arg(long, short)]
        dir: Option<String>,

        /// Source DSL to transpile from (currently only supports terraform)
        #[arg(long, short = 's')]
        source_provisioner: ProvisionerType,

        /// Target DSL to transpile to (currently only supports eraser)
        #[arg(long, short = 't')]
        target_provisioner: TranspileTargetProvisionerType,
    },

    /// Start the MCP server
    Mcp {
        /// Disable secret redaction (WARNING: this will print secrets to the console)
        #[arg(long = "disable-secret-redaction", default_value_t = false)]
        disable_secret_redaction: bool,

        /// Enable privacy mode to redact private data like IP addresses and AWS account IDs
        #[arg(long = "privacy-mode", default_value_t = false)]
        privacy_mode: bool,

        /// Tool mode to use (local, remote, combined)
        #[arg(long, short = 'm', default_value_t = ToolMode::Combined)]
        tool_mode: ToolMode,

        /// Allow indexing of large projects (more than 500 supported files)
        #[arg(long = "index-big-project", default_value_t = false)]
        index_big_project: bool,

        /// Disable mTLS (WARNING: this will use unencrypted HTTP communication)
        #[arg(long = "disable-mcp-mtls", default_value_t = false)]
        disable_mcp_mtls: bool,
    },

    /// Stakpak Agent (WARNING: These agents are in early alpha development and may be unstable)
    #[command(subcommand)]
    Agent(AgentCommands),

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
            Commands::Mcp {
                disable_secret_redaction,
                privacy_mode,
                tool_mode,
                index_big_project,
                disable_mcp_mtls,
            } => {
                let api_config: ClientConfig = config.clone().into();
                match tool_mode {
                    ToolMode::RemoteOnly | ToolMode::Combined => {
                        match get_or_build_local_code_index(&api_config, None, index_big_project)
                            .await
                        {
                            Ok(_) => {
                                // Indexing was successful, start the file watcher
                                tokio::spawn(async move {
                                    match start_code_index_watcher(&api_config, None) {
                                        Ok(_) => {}
                                        Err(e) => {
                                            eprintln!("Failed to start code index watcher: {}", e);
                                        }
                                    }
                                });
                            }
                            Err(e)
                                if e.contains("threshold") && e.contains("--index-big-project") =>
                            {
                                // This is the expected error when file count exceeds limit
                                // Continue silently without file watcher
                            }
                            Err(e) => {
                                eprintln!("Failed to build code index: {}", e);
                                // Continue without code indexing instead of exiting
                            }
                        }
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
            Commands::Account => {
                let client = Client::new(&(config.into())).map_err(|e| e.to_string())?;
                let data = client.get_my_account().await?;
                println!("{}", data.to_text());
            }
            Commands::List => {
                let client = Client::new(&config.into()).map_err(|e| e.to_string())?;
                let owner_name = client.get_my_account().await?.username;
                let data = client.list_flows(&owner_name).await?;
                println!("{}", data.to_text(&owner_name));
            }
            Commands::Get { flow_ref } => {
                let client = Client::new(&config.into()).map_err(|e| e.to_string())?;
                let parts: Vec<&str> = flow_ref.split('/').collect();

                let (owner_name, flow_name) = if parts.len() == 2 {
                    (parts[0], parts[1])
                } else {
                    return Err("Flow ref must be of the format <owner name>/<flow name>".into());
                };

                let data = client.get_flow(owner_name, flow_name).await?;
                println!("{}", data.to_text(owner_name));
            }
            Commands::Clone { flow_ref, dir } => {
                let client = Client::new(&config.into()).map_err(|e| e.to_string())?;
                let flow_ref = get_flow_ref(&client, flow_ref).await?;
                clone(&client, &flow_ref, dir.as_deref()).await?;
            }
            Commands::Query {
                query,
                flow_ref,
                generate_query,
                synthesize_output,
            } => {
                let client = Client::new(&config.into()).map_err(|e| e.to_string())?;
                let data = client
                    .query_blocks(
                        &query,
                        generate_query,
                        synthesize_output,
                        flow_ref.as_deref(),
                    )
                    .await?;

                let skin = MadSkin::default();
                println!("{}", skin.inline(&data.to_text(synthesize_output)));
            }
            Commands::Push {
                flow_ref,
                create,
                dir,
                ignore_delete,
                auto_approve,
            } => {
                let client = Client::new(&config.into()).map_err(|e| e.to_string())?;

                let save_result =
                    push(&client, flow_ref, create, dir, ignore_delete, auto_approve).await?;

                if let Some(save_result) = save_result {
                    if !save_result.errors.is_empty() {
                        println!("\nSave errors:");
                        for error in save_result.errors {
                            println!("\t{}: {}", error.uri, error.message);
                            if let Some(details) = error.details {
                                println!("\t\t{}", details);
                            }
                        }
                    }

                    let total_blocks =
                        save_result.created_blocks.len() + save_result.modified_blocks.len();

                    if total_blocks > 0 {
                        println!(
                            "Please wait {:.2} minutes for indexing to complete",
                            total_blocks as f64 * 1.5 / 60.0
                        );
                    }
                }
            }
            Commands::Transpile {
                dir,
                source_provisioner,
                target_provisioner,
            } => {
                if target_provisioner != TranspileTargetProvisionerType::EraserDSL {
                    return Err(
                        "Currently only EraserDSL is supported as a transpile target".into(),
                    );
                }
                if source_provisioner != ProvisionerType::Terraform {
                    return Err("Currently only terraform is supported as a source DSL".into());
                }

                let client = Client::new(&config.into()).map_err(|e| e.to_string())?;
                let base_dir = dir.unwrap_or_else(|| ".".into());

                let mut documents = Vec::new();

                for entry in WalkDir::new(&base_dir)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| {
                        // Skip hidden directories and non-supported files
                        let file_name = e.file_name().to_str();
                        match file_name {
                            Some(name) => {
                                // Skip hidden files/dirs that aren't just "."
                                if name.starts_with('.') && name.len() > 1 {
                                    return false;
                                }
                                // Only allow terraform files when from is terraform
                                if e.file_type().is_file() {
                                    name.ends_with(".tf")
                                } else {
                                    true // Allow directories to be traversed
                                }
                            }
                            None => false,
                        }
                    })
                    .filter_map(|e| e.ok())
                {
                    // Skip directories
                    if !entry.file_type().is_file() {
                        continue;
                    }

                    let path = entry.path();
                    // Skip binary files by attempting to read as UTF-8 and checking for errors
                    let content = match std::fs::read_to_string(path) {
                        Ok(content) => content,
                        Err(_) => continue, // Skip file if it can't be read as valid UTF-8
                    };

                    // Convert path to URI format
                    #[allow(clippy::unwrap_used)]
                    let document_path = path
                        .strip_prefix(&base_dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    let document_uri = format!("file://{}", document_path);

                    documents.push(Document {
                        content,
                        uri: document_uri,
                        provisioner: source_provisioner.clone(),
                    });
                }

                if documents.is_empty() {
                    return Err(format!(
                        "No {} files found to transpile",
                        source_provisioner
                    ));
                }

                let result = client
                    .transpile(documents, source_provisioner, target_provisioner)
                    .await?;
                println!(
                    "{}",
                    result
                        .result
                        .blocks
                        .into_iter()
                        .map(|b| b.code)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
            Commands::Agent(agent_commands) => {
                if let AgentCommands::Get { .. } = agent_commands {
                } else {
                    println!();
                    println!(
                        "[WARNING: These agents are in early alpha development and may be unstable]"
                    );
                    println!();
                };

                AgentCommands::run(agent_commands, config).await?;
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
