use clap::Parser;
use names::{self, Name};
use rustls::crypto::CryptoProvider;
use stakpak_api::{Client, ClientConfig};
use stakpak_mcp_server::EnabledToolsConfig;
use stakpak_shared::models::subagent::SubagentConfigs;
use std::{env, path::Path};

mod apkey_auth;
mod code_index;
mod commands;
mod config;
mod utils;

use commands::{
    Commands,
    agent::{
        self,
        run::{OutputFormat, RunAsyncConfig, RunInteractiveConfig},
    },
};
use config::AppConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utils::check_update::{auto_update, check_update};
use utils::gitignore;
use utils::local_context::analyze_local_context;

use crate::apkey_auth::prompt_for_api_key;
use crate::code_index::{get_or_build_local_code_index, start_code_index_watcher};

#[derive(Parser, PartialEq)]
#[command(name = "stakpak")]
#[command(about = "Stakpak CLI tool", long_about = None)]
struct Cli {
    /// Run the agent for a single step and print the response
    #[arg(short = 'p', long = "print", default_value_t = false)]
    print: bool,

    /// Run the agent in async mode (multiple steps until completion)
    #[arg(short = 'a', long = "async", default_value_t = false)]
    r#async: bool,

    /// Maximum number of steps the agent can take (default: 50 for --async, 1 for --print/--approve)
    #[arg(short = 'm', long = "max-steps")]
    max_steps: Option<usize>,

    /// Resume agent session at a specific checkpoint
    #[arg(short = 'c', long = "checkpoint")]
    checkpoint_id: Option<String>,

    /// Run the agent in a specific directory
    #[arg(short = 'w', long = "workdir")]
    workdir: Option<String>,

    /// Enable verbose output
    #[arg(long = "verbose", default_value_t = false)]
    verbose: bool,

    /// Output format: json or text
    #[arg(short = 'o', long = "output", default_value_t = OutputFormat::Text)]
    output_format: OutputFormat,

    /// Enable debug output
    #[arg(long = "debug", default_value_t = false)]
    debug: bool,

    /// Disable secret redaction (WARNING: this will print secrets to the console)
    #[arg(long = "disable-secret-redaction", default_value_t = false)]
    disable_secret_redaction: bool,

    /// Enable privacy mode to redact private data like IP addresses and AWS account IDs
    #[arg(long = "privacy-mode", default_value_t = false)]
    privacy_mode: bool,

    /// Enable study mode to use the agent as a study assistant
    #[arg(long = "study-mode", default_value_t = false)]
    study_mode: bool,

    /// Allow indexing of large projects (more than 500 supported files)
    #[arg(long = "index-big-project", default_value_t = false)]
    index_big_project: bool,

    /// Enable Slack tools (experimental)
    #[arg(long = "enable-slack-tools", default_value_t = false)]
    enable_slack_tools: bool,

    /// Disable mTLS (WARNING: this will use unencrypted HTTP communication)
    #[arg(long = "disable-mcp-mtls", default_value_t = false)]
    disable_mcp_mtls: bool,

    /// Enable subagents
    #[arg(long = "enable-subagents", default_value_t = false)]
    enable_subagents: bool,

    /// Subagent configuration file subagents.toml
    #[arg(long = "subagent-config")]
    subagent_config_path: Option<String>,

    /// Allow only the specified tool in the agent's context
    #[arg(short = 't', long = "tool", action = clap::ArgAction::Append)]
    allowed_tools: Option<Vec<String>>,

    /// Read system prompt from file
    #[arg(long = "system-prompt-file")]
    system_prompt_file: Option<String>,

    /// Read prompt from file (runs in async mode only)
    #[arg(long = "prompt-file")]
    prompt_file: Option<String>,

    /// Configuration profile to use (can also be set with STAKPAK_PROFILE env var)
    #[arg(long = "profile")]
    profile: Option<String>,

    /// Custom path to config file (overrides default ~/.stakpak/config.toml)
    #[arg(long = "config")]
    config_path: Option<String>,

    /// Prompt to run the agent
    prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

static DEFAULT_SUBAGENT_CONFIG: &str = include_str!("../../subagents.toml");

#[tokio::main]
async fn main() {
    // Initialize rustls crypto provider
    let _ = CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider());

    let cli = Cli::parse();

    // Only run auto-update in interactive mode (when no command is specified)
    if cli.command.is_none()
        && !cli.r#async
        && !cli.print
        && let Err(e) = auto_update().await
    {
        eprintln!("Auto-update failed: {}", e);
    }

    if let Some(workdir) = cli.workdir {
        let workdir = Path::new(&workdir);
        if let Err(e) = env::set_current_dir(workdir) {
            eprintln!("Failed to set current directory: {}", e);
            std::process::exit(1);
        }
    }

    if cli.debug {
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| format!("error,{}=debug", env!("CARGO_CRATE_NAME")).into()),
            )
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    // Determine which profile to use: CLI arg > STAKPAK_PROFILE env var > "default"
    let profile_name = cli
        .profile
        .or_else(|| std::env::var("STAKPAK_PROFILE").ok())
        .unwrap_or_else(|| "default".to_string());

    match AppConfig::load(&profile_name, cli.config_path.as_deref()) {
        Ok(mut config) => {
            if config.machine_name.is_none() {
                // Generate a random machine name
                let random_name = names::Generator::with_naming(Name::Numbered)
                    .next()
                    .unwrap_or_else(|| "unknown-machine".to_string());

                config.machine_name = Some(random_name);

                if let Err(e) = config.save() {
                    eprintln!("Failed to save config: {}", e);
                }
            }

            match cli.command {
                Some(command) => {
                    // check_update is only run in interactive mode (when no command is specified)
                    if config.api_key.is_none() && command.requires_auth() {
                        prompt_for_api_key(&mut config).await;
                    }

                    // Ensure .stakpak is in .gitignore (after workdir is set, before command execution)
                    let _ = gitignore::ensure_stakpak_in_gitignore(&config);

                    match command.run(config).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Ops! something went wrong: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    if config.api_key.is_none() {
                        prompt_for_api_key(&mut config).await;
                    }
                    let local_context = analyze_local_context(&config).await.ok();
                    let api_config: ClientConfig = config.clone().into();
                    let client = if let Ok(client) = Client::new(&api_config) {
                        client
                    } else {
                        eprintln!("Failed to create client");
                        std::process::exit(1);
                    };

                    match client.get_my_account().await {
                        Ok(_) => {}
                        Err(e) => {
                            println!();
                            println!("❌ API key validation failed: {}", e);
                            println!("Please check your API key and run the below command");
                            println!();
                            println!("\x1b[1;34mstakpak login --api-key <your-api-key>\x1b[0m");
                            println!();
                            std::process::exit(1);
                        }
                    }

                    // Check for updates in interactive mode
                    let _ = check_update(format!("v{}", env!("CARGO_PKG_VERSION")).as_str()).await;
                    let rulebooks = client.list_rulebooks().await.ok().map(|rulebooks| {
                        if let Some(rulebook_config) = &config.rulebooks {
                            rulebook_config.filter_rulebooks(rulebooks)
                        } else {
                            rulebooks
                        }
                    });

                    let subagent_configs = if cli.enable_subagents {
                        if let Some(subagent_config_path) = &cli.subagent_config_path {
                            SubagentConfigs::load_from_file(subagent_config_path)
                                .map_err(|e| {
                                    eprintln!("Warning: Failed to load subagent configs: {}", e);
                                    e
                                })
                                .ok()
                        } else {
                            SubagentConfigs::load_from_str(DEFAULT_SUBAGENT_CONFIG)
                                .map_err(|e| {
                                    eprintln!("Warning: Failed to load subagent configs: {}", e);
                                    e
                                })
                                .ok()
                        }
                    } else {
                        None
                    };

                    match get_or_build_local_code_index(&api_config, None, cli.index_big_project)
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
                        Err(e) if e.contains("threshold") && e.contains("--index-big-project") => {
                            // This is the expected error when file count exceeds limit
                            // Continue silently without file watcher
                        }
                        Err(e) => {
                            eprintln!("Failed to build code index: {}", e);
                            // Continue without code indexing instead of exiting
                        }
                    }

                    let system_prompt =
                        if let Some(system_prompt_file_path) = &cli.system_prompt_file {
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
                                    std::process::exit(1);
                                }
                            }
                        } else {
                            None
                        };

                    let prompt = if let Some(prompt_file_path) = &cli.prompt_file {
                        match std::fs::read_to_string(prompt_file_path) {
                            Ok(content) => {
                                println!("📖 Reading prompt from file: {}", prompt_file_path);
                                content.trim().to_string()
                            }
                            Err(e) => {
                                eprintln!(
                                    "Failed to read prompt file '{}': {}",
                                    prompt_file_path, e
                                );
                                std::process::exit(1);
                            }
                        }
                    } else {
                        cli.prompt.unwrap_or_default()
                    };

                    // When using --prompt-file, force async mode only
                    let use_async_mode = cli.r#async || cli.print;

                    // Determine max_steps: 1 for single-step mode (--print/--approve), user setting or default for --async
                    let max_steps = if cli.print {
                        Some(1) // Force single step for non-interactive-like behavior
                    } else {
                        cli.max_steps // Use user setting or default (50)
                    };

                    // Ensure .stakpak is in .gitignore before running agent
                    let _ = gitignore::ensure_stakpak_in_gitignore(&config);

                    let allowed_tools = cli.allowed_tools.or_else(|| config.allowed_tools.clone());
                    let auto_approve = config.auto_approve.clone();

                    match use_async_mode {
                        // Async mode: run continuously until no more tool calls (or max_steps=1 for single-step)
                        true => match agent::run::run_async(
                            config,
                            RunAsyncConfig {
                                prompt,
                                verbose: cli.verbose,
                                checkpoint_id: cli.checkpoint_id,
                                local_context,
                                redact_secrets: !cli.disable_secret_redaction,
                                privacy_mode: cli.privacy_mode,
                                rulebooks,
                                subagent_configs,
                                max_steps,
                                output_format: cli.output_format,
                                enable_mtls: !cli.disable_mcp_mtls,
                                allowed_tools,
                                system_prompt,
                                enabled_tools: EnabledToolsConfig {
                                    slack: cli.enable_slack_tools,
                                },
                            },
                        )
                        .await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("Ops! something went wrong: {}", e);
                                std::process::exit(1);
                            }
                        },

                        // Interactive mode: run in TUI
                        false => match agent::run::run_interactive(
                            config,
                            RunInteractiveConfig {
                                checkpoint_id: cli.checkpoint_id,
                                local_context,
                                redact_secrets: !cli.disable_secret_redaction,
                                privacy_mode: cli.privacy_mode,
                                rulebooks,
                                subagent_configs,
                                enable_mtls: !cli.disable_mcp_mtls,
                                is_git_repo: gitignore::is_git_repo(),
                                study_mode: cli.study_mode,
                                system_prompt,
                                allowed_tools,
                                auto_approve,
                                enabled_tools: EnabledToolsConfig {
                                    slack: cli.enable_slack_tools,
                                },
                            },
                        )
                        .await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("Ops! something went wrong: {}", e);
                                std::process::exit(1);
                            }
                        },
                    }
                }
            }
        }
        Err(e) => eprintln!("Failed to load config: {}", e),
    }
}
