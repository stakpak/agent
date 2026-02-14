use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::config::AppConfig;
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use stakpak_api::{AgentClient, AgentClientConfig, AgentProvider, StakpakConfig};

pub mod acp;
pub mod agent;
pub mod auth;
pub mod auto_update;
pub mod board;
pub mod gateway;
pub mod mcp;
pub mod warden;
pub mod watch;

pub use auth::AuthCommands;
pub use gateway::GatewayCommands;
pub use mcp::McpCommands;
pub use watch::WatchCommands;

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
    /// List and select profiles interactively
    #[command(name = "list", alias = "ls")]
    List,
    /// Show current configuration
    Show,
    /// Print a complete sample configuration file
    Sample,
    /// Create a new profile
    New,
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
pub enum Commands {
    /// Get CLI Version
    Version,
    /// Login to Stakpak (DEPRECATED: use `stakpak auth login -p stakpak` instead)
    #[command(hide = true)]
    Login {
        /// API key for authentication
        #[arg(long, env("STAKPAK_API_KEY"))]
        api_key: String,
    },

    /// Logout from Stakpak (DEPRECATED: use `stakpak auth logout -p stakpak` instead)
    #[command(hide = true)]
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

    /// Analyze your infrastructure setup
    Init,

    /// MCP commands
    #[command(subcommand)]
    Mcp(McpCommands),

    /// Provider authentication commands (OAuth, API keys)
    #[command(subcommand)]
    Auth(AuthCommands),

    /// Messaging gateway commands
    #[command(subcommand)]
    Gateway(GatewayCommands),

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
    /// Task board for tracking complex work (cards, checklists, comments)
    /// Run `stakpak board --help` for available commands.
    Board {
        /// Arguments to pass to the board plugin
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Update Stakpak Agent to the latest version
    Update,

    /// Start the HTTP/SSE server runtime
    Serve {
        /// Bind address, e.g. 127.0.0.1:4096
        #[arg(long, default_value = "127.0.0.1:4096")]
        bind: String,

        /// Bearer token required for protected routes. If omitted, a secure token is generated.
        #[arg(long)]
        auth_token: Option<String>,

        /// Show generated auth token in stdout (local dev only)
        #[arg(long, default_value_t = false)]
        show_token: bool,

        /// Disable auth checks for protected routes (local dev only)
        #[arg(long, default_value_t = false)]
        no_auth: bool,

        /// Override default model for server runs (provider/model or model id)
        #[arg(long)]
        model: Option<String>,

        /// Auto-approve all tools (CI/headless only)
        #[arg(long, default_value_t = false)]
        auto_approve_all: bool,

        /// Also start the messaging gateway
        #[arg(long, default_value_t = false)]
        gateway: bool,

        /// Path to gateway config file (requires --gateway)
        #[arg(long)]
        gateway_config: Option<std::path::PathBuf>,
    },

    /// Start everything: serve + gateway + watch
    Up {
        /// Bind address, e.g. 127.0.0.1:4096
        #[arg(long, default_value = "127.0.0.1:4096")]
        bind: String,

        /// Show generated auth token in stdout (local dev only)
        #[arg(long, default_value_t = false)]
        show_token: bool,

        /// Disable auth checks for protected routes (local dev only)
        #[arg(long, default_value_t = false)]
        no_auth: bool,

        /// Override default model for server runs (provider/model or model id)
        #[arg(long)]
        model: Option<String>,

        /// Auto-approve all tools (CI/headless only)
        #[arg(long, default_value_t = false)]
        auto_approve_all: bool,

        /// Don't start gateway runtime
        #[arg(long, default_value_t = false)]
        no_gateway: bool,

        /// Don't start watch scheduler
        #[arg(long, default_value_t = false)]
        no_watch: bool,

        /// Path to gateway config file
        #[arg(long)]
        gateway_config: Option<std::path::PathBuf>,
    },

    /// Run the autonomous watch agent with scheduled triggers
    #[command(subcommand)]
    Watch(WatchCommands),
}

async fn build_agent_client(config: &AppConfig) -> Result<AgentClient, String> {
    // Use credential resolution with auth.toml fallback chain
    // Refresh OAuth tokens in parallel to minimize startup delay
    let providers = config.get_llm_provider_config_async().await;

    let stakpak = config.get_stakpak_api_key().map(|api_key| StakpakConfig {
        api_key,
        api_endpoint: config.api_endpoint.clone(),
    });

    AgentClient::new(AgentClientConfig {
        stakpak,
        providers,
        eco_model: config.eco_model.clone(),
        recovery_model: config.recovery_model.clone(),
        smart_model: config.smart_model.clone(),
        store_path: None,
        hook_registry: None,
    })
    .await
    .map_err(|e| format!("Failed to create agent client: {}", e))
}

async fn get_client(config: &AppConfig) -> Result<Arc<dyn AgentProvider>, String> {
    Ok(Arc::new(build_agent_client(config).await?))
}

/// Helper function to convert AppConfig's config_path to Option<&Path>
fn get_config_path_option(config: &AppConfig) -> Option<&Path> {
    if config.config_path.is_empty() {
        None
    } else {
        Some(Path::new(&config.config_path))
    }
}

fn expand_gateway_approval_allowlist(tools: &[String]) -> Vec<String> {
    let mut normalized = BTreeSet::new();

    for tool in tools {
        let trimmed = tool.trim();
        if trimmed.is_empty() {
            continue;
        }

        normalized.insert(trimmed.to_string());
        if !trimmed.starts_with("stakpak__") {
            normalized.insert(format!("stakpak__{trimmed}"));
        }
    }

    normalized.into_iter().collect()
}

fn loopback_server_url(listener_addr: SocketAddr) -> String {
    let port = listener_addr.port();
    if listener_addr.ip().is_ipv6() {
        format!("http://[::1]:{port}")
    } else {
        format!("http://127.0.0.1:{port}")
    }
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
                | Commands::Auth(_)
                | Commands::Gateway(_)
                | Commands::Serve { .. }
                | Commands::Up { .. }
                | Commands::Watch(_)
        )
    }
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
            Commands::Mcp(command) => {
                command.run(config).await?;
            }
            Commands::Login { api_key } => {
                // Show deprecation warning
                eprintln!("\x1b[33mWarning: 'stakpak login' is deprecated.\x1b[0m");
                eprintln!("Please use: \x1b[1;34mstakpak auth login --provider stakpak\x1b[0m");
                eprintln!();

                let mut updated_config = config.clone();
                updated_config.api_key = Some(api_key);

                updated_config
                    .save()
                    .map_err(|e| format!("Failed to save config: {}", e))?;
            }
            Commands::Logout => {
                // Show deprecation warning
                eprintln!("\x1b[33mWarning: 'stakpak logout' is deprecated.\x1b[0m");
                eprintln!("Please use: \x1b[1;34mstakpak auth logout --provider stakpak\x1b[0m");
                eprintln!();

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
            Commands::Config(config_command) => {
                match config_command {
                    ConfigCommands::List => {
                        // Interactive profile selection menu
                        use crate::onboarding::menu::select_profile_interactive;
                        let config_path = get_config_path_option(&config);
                        if let Some(selected_profile) =
                            select_profile_interactive(config_path).await
                        {
                            if selected_profile == "CREATE_NEW_PROFILE" {
                                // Create new profile
                                use crate::onboarding::{OnboardingMode, run_onboarding};
                                let mut mutable_config = config.clone();
                                run_onboarding(&mut mutable_config, OnboardingMode::New).await;

                                // Ask if user wants to continue to stakpak
                                use crate::onboarding::menu::prompt_yes_no;
                                use crate::onboarding::navigation::NavResult;
                                if let NavResult::Forward(Some(true)) =
                                    prompt_yes_no("Continue to stakpak?", true)
                                {
                                    // Re-execute stakpak with the new profile
                                    let new_profile = mutable_config.profile_name.clone();
                                    re_execute_stakpak_with_profile(
                                        &new_profile,
                                        get_config_path_option(&config),
                                    );
                                }
                            } else {
                                // Switch to selected profile
                                re_execute_stakpak_with_profile(
                                    &selected_profile,
                                    get_config_path_option(&config),
                                );
                            }
                        }
                    }
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
                    ConfigCommands::New => {
                        use crate::onboarding::{OnboardingMode, run_onboarding};
                        let mut mutable_config = config.clone();
                        run_onboarding(&mut mutable_config, OnboardingMode::New).await;

                        use crate::onboarding::menu::prompt_yes_no;
                        use crate::onboarding::navigation::NavResult;
                        if let NavResult::Forward(Some(true)) =
                            prompt_yes_no("Continue to stakpak?", true)
                        {
                            let new_profile = mutable_config.profile_name.clone();
                            re_execute_stakpak_with_profile(
                                &new_profile,
                                get_config_path_option(&config),
                            );
                        }
                    }
                }
            }
            Commands::Rulebooks(rulebook_command) => {
                let client = get_client(&config).await?;
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
                let client = get_client(&config).await?;
                let data = client.get_my_account().await?;
                println!("{}", data.to_text());
            }
            Commands::Init => {
                // Handled in main: starts interactive session with init prompt sent on start
                unreachable!("stakpak init is handled before Commands::run()")
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
            Commands::Board { args } => {
                board::run_board(args).await?;
            }
            Commands::Update => {
                auto_update::run_auto_update(false).await?;
            }
            Commands::Gateway(gateway_command) => {
                gateway_command.run(config).await?;
            }
            Commands::Up {
                bind,
                show_token,
                no_auth,
                model,
                auto_approve_all,
                no_gateway,
                no_watch,
                gateway_config,
            } => {
                let watch_task = if no_watch {
                    None
                } else {
                    Some(tokio::spawn(async {
                        if let Err(error) = crate::commands::watch::commands::run_watch().await {
                            eprintln!("Watch runtime exited: {}", error);
                        }
                    }))
                };

                let current_exe = std::env::current_exe()
                    .map_err(|e| format!("Failed to resolve current executable: {}", e))?;

                let mut serve_cmd = tokio::process::Command::new(current_exe);
                if config.profile_name != "default" {
                    serve_cmd.arg("--profile").arg(&config.profile_name);
                }
                serve_cmd.arg("serve");
                serve_cmd.arg("--bind").arg(bind);

                if show_token {
                    serve_cmd.arg("--show-token");
                }
                if no_auth {
                    serve_cmd.arg("--no-auth");
                }
                if auto_approve_all {
                    serve_cmd.arg("--auto-approve-all");
                }
                if !no_gateway {
                    serve_cmd.arg("--gateway");
                }
                if let Some(model) = model {
                    serve_cmd.arg("--model").arg(model);
                }
                if let Some(path) = gateway_config {
                    serve_cmd.arg("--gateway-config").arg(path);
                }

                let status = serve_cmd
                    .status()
                    .await
                    .map_err(|e| format!("Failed to start serve runtime: {}", e))?;

                if let Some(task) = watch_task {
                    task.abort();
                    let _ = task.await;
                }

                if !status.success() {
                    return Err(format!(
                        "Serve runtime exited with status {}",
                        status
                            .code()
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "signal".to_string())
                    ));
                }
            }
            Commands::Serve {
                bind,
                auth_token,
                show_token,
                no_auth,
                model,
                auto_approve_all,
                gateway,
                gateway_config,
            } => {
                if no_auth && auth_token.is_some() {
                    return Err(
                        "Cannot combine --no-auth with --auth-token. Remove one of them."
                            .to_string(),
                    );
                }

                let (auth_config, generated_auth_token) = if no_auth {
                    (stakpak_server::AuthConfig::disabled(), None)
                } else {
                    let token = auth_token
                        .unwrap_or_else(|| stakpak_shared::utils::generate_password(64, true));
                    (
                        stakpak_server::AuthConfig::token(token.clone()),
                        Some(token),
                    )
                };

                let listener = tokio::net::TcpListener::bind(&bind)
                    .await
                    .map_err(|e| format!("Failed to bind {}: {}", bind, e))?;

                let runtime_client = build_agent_client(&config).await?;
                let storage = runtime_client.session_storage().clone();

                let events = Arc::new(stakpak_server::EventLog::new(4096));
                let idempotency = Arc::new(stakpak_server::IdempotencyStore::new(
                    std::time::Duration::from_secs(24 * 60 * 60),
                ));
                let inference = Arc::new(
                    stakai::Inference::builder()
                        .with_registry(runtime_client.stakai().registry().clone())
                        .build()
                        .map_err(|e| format!("Failed to initialize inference runtime: {}", e))?,
                );

                let mut models = runtime_client.list_models().await;
                let requested_model = model.or(config.model.clone());
                let auto_approve_tools = config.auto_approve.clone();
                let allowed_tools = config.allowed_tools.clone();

                let requested_model_from_catalog = requested_model.as_deref().and_then(|name| {
                    if let Some((provider, id)) = name.split_once('/') {
                        return models
                            .iter()
                            .find(|model| model.provider == provider && model.id == id)
                            .cloned();
                    }

                    models.iter().find(|model| model.id == name).cloned()
                });

                let requested_custom_model = requested_model.as_deref().and_then(|name| {
                    name.split_once('/')
                        .map(|(provider, id)| stakai::Model::custom(id, provider))
                });

                let default_model = requested_model_from_catalog
                    .clone()
                    .or(requested_custom_model)
                    .or_else(|| models.first().cloned())
                    .or_else(|| Some(stakai::Model::custom("gpt-4o-mini", "openai")));

                if let Some(requested) = requested_model.as_deref()
                    && requested_model_from_catalog.is_none()
                {
                    if requested.contains('/') {
                        eprintln!(
                            "⚠ Requested model '{}' is not in the catalog; using it as a custom model id.",
                            requested
                        );
                    } else if let Some(resolved) = default_model.as_ref() {
                        eprintln!(
                            "⚠ Requested model '{}' not found in catalog; using fallback '{}/{}'.",
                            requested, resolved.provider, resolved.id
                        );
                    }
                }

                if models.is_empty()
                    && let Some(default_model) = default_model.clone()
                {
                    models.push(default_model);
                }

                let tool_approval_policy = if auto_approve_all {
                    stakpak_server::ToolApprovalPolicy::All
                } else {
                    let policy = stakpak_server::ToolApprovalPolicy::with_defaults();
                    if let Some(ref auto_approve_tools) = auto_approve_tools {
                        policy.with_overrides(
                            auto_approve_tools
                                .iter()
                                .cloned()
                                .map(|tool| (tool, stakpak_server::ToolApprovalAction::Approve)),
                        )
                    } else {
                        policy
                    }
                };

                let mcp_init_config = crate::commands::agent::run::mcp_init::McpInitConfig {
                    redact_secrets: true,
                    privacy_mode: false,
                    enabled_tools: stakpak_mcp_server::EnabledToolsConfig { slack: false },
                    enable_mtls: true,
                    enable_subagents: true,
                    allowed_tools,
                };

                let mcp_init_result =
                    crate::commands::agent::run::mcp_init::initialize_mcp_server_and_tools(
                        &config,
                        mcp_init_config,
                        None,
                    )
                    .await
                    .map_err(|e| format!("Failed to initialize MCP stack: {}", e))?;

                let mcp_tools = mcp_init_result
                    .mcp_tools
                    .iter()
                    .map(|tool| stakai::Tool {
                        tool_type: "function".to_string(),
                        function: stakai::ToolFunction {
                            name: tool.name.as_ref().to_string(),
                            description: tool
                                .description
                                .as_ref()
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            parameters: serde_json::Value::Object((*tool.input_schema).clone()),
                        },
                        provider_options: None,
                    })
                    .collect();

                let app_state = stakpak_server::AppState::new(
                    storage,
                    events,
                    idempotency,
                    inference,
                    models,
                    default_model,
                    tool_approval_policy,
                )
                .with_mcp(
                    mcp_init_result.client,
                    mcp_tools,
                    Some(mcp_init_result.server_shutdown_tx),
                    Some(mcp_init_result.proxy_shutdown_tx),
                );

                let gateway_runtime = if gateway {
                    let listener_addr = listener
                        .local_addr()
                        .map_err(|e| format!("Failed to inspect listener address: {}", e))?;
                    let loopback_url = loopback_server_url(listener_addr);
                    let loopback_token = if no_auth {
                        String::new()
                    } else {
                        generated_auth_token.clone().unwrap_or_default()
                    };

                    let gateway_cli = stakpak_gateway::GatewayCliFlags {
                        url: Some(loopback_url),
                        token: Some(loopback_token),
                        ..Default::default()
                    };

                    let mut gateway_cfg = stakpak_gateway::GatewayConfig::load(
                        gateway_config.as_deref(),
                        &gateway_cli,
                    )
                    .map_err(|e| format!("Failed to load gateway config: {}", e))?;

                    if auto_approve_all {
                        gateway_cfg.gateway.approval_mode = stakpak_gateway::ApprovalMode::AllowAll;
                        gateway_cfg.gateway.approval_allowlist.clear();
                    } else if let Some(auto_approve_tools) = auto_approve_tools.as_ref() {
                        gateway_cfg.gateway.approval_mode =
                            stakpak_gateway::ApprovalMode::Allowlist;
                        gateway_cfg.gateway.approval_allowlist =
                            expand_gateway_approval_allowlist(auto_approve_tools);
                    }

                    if !gateway_cfg.has_channels() {
                        println!(
                            "Gateway enabled but no channels configured. Skipping gateway runtime."
                        );
                        None
                    } else {
                        Some(Arc::new(
                            stakpak_gateway::Gateway::new(gateway_cfg)
                                .await
                                .map_err(|e| {
                                    format!("Failed to initialize gateway runtime: {}", e)
                                })?,
                        ))
                    }
                } else {
                    None
                };

                let refresh_state = app_state.clone();
                let (refresh_shutdown_tx, mut refresh_shutdown_rx) =
                    tokio::sync::watch::channel(false);
                let refresh_task = tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                                if let Err(error) = refresh_state.refresh_mcp_tools().await {
                                    eprintln!("[mcp-refresh] {}", error);
                                }
                            }
                            changed = refresh_shutdown_rx.changed() => {
                                if changed.is_err() || *refresh_shutdown_rx.borrow() {
                                    break;
                                }
                            }
                        }
                    }
                });

                let shutdown_state = app_state.clone();
                let shutdown_refresh_tx = refresh_shutdown_tx.clone();

                let base_app = stakpak_server::router(app_state, auth_config);
                let app = if let Some(gateway_runtime) = gateway_runtime.as_ref() {
                    let gateway_routes = gateway_runtime.api_router();
                    base_app.nest_service("/v1/gateway", gateway_routes.into_service())
                } else {
                    base_app
                };

                println!("Stakpak server listening on http://{}", bind);
                println!("Profile: {}", config.profile_name);
                println!(
                    "Session backend: {}",
                    if runtime_client.has_stakpak() {
                        "stakpak remote"
                    } else {
                        "local sqlite"
                    }
                );

                if no_auth {
                    println!("Auth: disabled (--no-auth)");
                } else if let Some(token) = generated_auth_token {
                    println!("Auth: enabled (Bearer token required)");
                    if show_token {
                        println!("Authorization: Bearer {}", token);
                    } else {
                        println!(
                            "Use --show-token to print the generated token in local development."
                        );
                    }
                }

                if gateway_runtime.is_some() {
                    println!("Gateway: enabled");
                }

                let gateway_cancel = tokio_util::sync::CancellationToken::new();
                let gateway_task = if let Some(gateway_runtime) = gateway_runtime.as_ref() {
                    let gateway_runtime = gateway_runtime.clone();
                    let cancel = gateway_cancel.clone();
                    Some(tokio::spawn(
                        async move { gateway_runtime.run(cancel).await },
                    ))
                } else {
                    None
                };
                let gateway_cancel_for_shutdown = gateway_cancel.clone();

                let shutdown = async move {
                    let _ = tokio::signal::ctrl_c().await;

                    gateway_cancel_for_shutdown.cancel();

                    for (session_id, run_id) in shutdown_state.run_manager.running_runs().await {
                        let _ = shutdown_state
                            .run_manager
                            .cancel_run(session_id, run_id)
                            .await;
                    }

                    let drain_deadline =
                        tokio::time::Instant::now() + std::time::Duration::from_secs(5);
                    loop {
                        if shutdown_state.run_manager.running_runs().await.is_empty()
                            || tokio::time::Instant::now() >= drain_deadline
                        {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }

                    let _ = shutdown_refresh_tx.send(true);

                    if let Some(tx) = shutdown_state.mcp_server_shutdown_tx.as_ref() {
                        let _ = tx.send(());
                    }
                    if let Some(tx) = shutdown_state.mcp_proxy_shutdown_tx.as_ref() {
                        let _ = tx.send(());
                    }
                };

                let serve_result = axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown)
                    .await;

                gateway_cancel.cancel();
                if let Some(task) = gateway_task {
                    match task.await {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => eprintln!("Gateway runtime error: {}", e),
                        Err(e) => eprintln!("Gateway runtime task failed: {}", e),
                    }
                }

                let _ = refresh_shutdown_tx.send(true);
                if !refresh_task.is_finished() {
                    refresh_task.abort();
                }
                let _ = refresh_task.await;

                serve_result.map_err(|e| format!("Server error: {}", e))?;
            }
            Commands::Watch(watch_command) => {
                use crate::commands::watch::commands::{
                    DescribeResource, GetResource, WatchCommands, fire_trigger, init_config,
                    install_watch, prune_history, reload_watch, resume_run, run_watch,
                    show_history, show_run, show_status, show_trigger, stop_watch, uninstall_watch,
                };
                match watch_command {
                    WatchCommands::Run => {
                        run_watch().await?;
                    }
                    WatchCommands::Stop => {
                        stop_watch().await?;
                    }
                    WatchCommands::Status => {
                        show_status().await?;
                    }
                    WatchCommands::Get { resource } => match resource {
                        GetResource::Triggers => {
                            show_status().await?; // Status already shows triggers
                        }
                        GetResource::Runs { trigger, limit } => {
                            show_history(trigger.as_deref(), Some(limit)).await?;
                        }
                    },
                    WatchCommands::Describe { resource } => match resource {
                        DescribeResource::Trigger { name } => {
                            show_trigger(&name).await?;
                        }
                        DescribeResource::Run { id } => {
                            show_run(id).await?;
                        }
                    },
                    WatchCommands::Fire { trigger, dry_run } => {
                        fire_trigger(&trigger, dry_run).await?;
                    }
                    WatchCommands::Resume { run_id, force } => {
                        resume_run(run_id, force).await?;
                    }
                    WatchCommands::Prune { days } => {
                        prune_history(days).await?;
                    }
                    WatchCommands::Init { force } => {
                        init_config(force).await?;
                    }
                    WatchCommands::Install { force } => {
                        install_watch(force).await?;
                    }
                    WatchCommands::Uninstall => {
                        uninstall_watch().await?;
                    }
                    WatchCommands::Reload => {
                        reload_watch().await?;
                    }
                }
            }
            Commands::Auth(auth_command) => {
                auth_command.run(config).await?;
            }
            Commands::Acp { system_prompt_file } => {
                // Force auto-update before starting ACP session (no prompt)
                use crate::utils::check_update::force_auto_update;
                if let Err(e) = force_auto_update().await {
                    // Log error but continue - don't block ACP if update check fails
                    eprintln!("Update check failed: {}", e);
                }

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

# Preferred external editor for /editor command (vim, nvim, nano, code, etc.)
editor = "nano"
"#
    );
}

/// Re-execute stakpak with a specific profile
fn re_execute_stakpak_with_profile(profile: &str, config_path: Option<&std::path::Path>) {
    let mut cmd = Command::new("stakpak");
    cmd.arg("--profile").arg(profile);

    if let Some(config_path) = config_path {
        cmd.arg("--config").arg(config_path);
    }

    // Preserve other args but skip "config" subcommand
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        // Skip "config" subcommand and its value
        if arg == "config" {
            skip_next = true;
            continue;
        }
        // Skip --profile and --config if they exist (we're setting them explicitly)
        if arg == "--profile" || arg == "--config" {
            skip_next = true;
            continue;
        }
        // Skip the value after --profile= or --config=
        if arg.starts_with("--profile=") || arg.starts_with("--config=") {
            continue;
        }
        cmd.arg(arg);
    }

    let status = cmd.status();
    match status {
        Ok(s) if s.success() => {
            std::process::exit(s.code().unwrap_or(0));
        }
        Ok(s) => {
            std::process::exit(s.code().unwrap_or(1));
        }
        Err(_) => {
            std::process::exit(1);
        }
    }
}
