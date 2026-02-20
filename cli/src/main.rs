// CLI crate uses string slicing for parsing model strings, frontmatter, tool names,
// error messages, and rendering. All indices come from find()/rfind() of ASCII
// delimiters on the same strings.
#![allow(clippy::string_slice)]

// On Linux musl, the default allocator aggressively munmap's freed pages back to the
// kernel. This causes use-after-free SIGSEGV in libsql's sqlite3Close() when concurrent
// threads race between Database::drop() and page reclamation. jemalloc retains freed
// pages in its arena, preventing this class of crash.
//
// IMPORTANT: tikv-jemallocator must be built with `unprefixed_malloc_on_supported_platforms`
// so jemalloc provides the actual malloc/free/calloc/realloc symbols. Without this feature,
// jemalloc uses prefixed names (_rjem_je_malloc) and only handles Rust allocations via
// #[global_allocator]. SQLite's embedded C code (compiled via libsql-ffi) calls the
// system malloc() directly — which on musl is the aggressive allocator that caused the crash.
//
// Gated to Linux only — this fix targets musl; macOS/Windows don't need allocator overrides.
#[cfg(all(feature = "jemalloc", target_os = "linux"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use clap::Parser;
use names::{self, Name};
use rustls::crypto::CryptoProvider;
use stakpak_api::local::skills::{default_skill_directories, discover_skills};
use stakpak_api::models::Skill;
use stakpak_api::{AgentClient, AgentClientConfig, AgentProvider};
use stakpak_mcp_server::EnabledToolsConfig;
use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

mod apikey_auth;
// mod code_index;
mod commands;
mod config;
mod onboarding;
mod utils;

use commands::{
    Commands,
    agent::{
        self,
        run::{
            AsyncOutcome, OutputFormat, ResumeInput, RunAsyncConfig, RunInteractiveConfig,
            pause::EXIT_CODE_PAUSED,
        },
    },
};
use config::{AppConfig, ModelsCache};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utils::agents_md::discover_agents_md;
use utils::apps_md::discover_apps_md;
use utils::check_update::{auto_update, check_update};
use utils::gitignore;
use utils::local_context::analyze_local_context;

use crate::onboarding::{OnboardingMode, run_onboarding};
// use crate::code_index::{get_or_build_local_code_index, start_code_index_watcher};

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
    #[arg(short = 'c', long = "checkpoint", conflicts_with = "session_id")]
    checkpoint_id: Option<String>,

    /// Resume from the latest checkpoint in a specific session
    #[arg(short = 's', long = "session", conflicts_with = "checkpoint_id")]
    session_id: Option<String>,

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

    /// Enter plan mode — research and draft a plan before executing
    #[arg(long = "plan", default_value_t = false)]
    plan: bool,

    /// Auto-approve the plan when status becomes 'reviewing' (async mode)
    #[arg(long = "plan-approved", default_value_t = false)]
    plan_approved: bool,

    /// Read feedback from a file and inject as plan feedback (async mode)
    #[arg(long = "plan-feedback")]
    plan_feedback: Option<String>,

    /// Archive any existing plan and start fresh (async mode, requires --plan)
    #[arg(long = "plan-new", default_value_t = false)]
    plan_new: bool,

    /// Allow indexing of large projects (more than 500 supported files)
    #[arg(long = "index-big-project", default_value_t = false)]
    index_big_project: bool,

    /// Enable Slack tools (experimental)
    #[arg(long = "enable-slack-tools", default_value_t = false)]
    enable_slack_tools: bool,

    /// Disable mTLS (WARNING: this will use unencrypted HTTP communication)
    #[arg(long = "disable-mcp-mtls", default_value_t = false)]
    disable_mcp_mtls: bool,

    /// Disable subagents
    #[arg(long = "disable-subagents", default_value_t = false)]
    disable_subagents: bool,

    /// Pause when tools require approval (async mode only)
    #[arg(long = "pause-on-approval", default_value_t = false)]
    pause_on_approval: bool,

    /// Approve a specific tool call by ID when resuming (can be repeated)
    #[arg(long = "approve", action = clap::ArgAction::Append)]
    approve: Option<Vec<String>>,

    /// Reject a specific tool call by ID when resuming (can be repeated)
    #[arg(long = "reject", action = clap::ArgAction::Append)]
    reject: Option<Vec<String>>,

    /// Approve all pending tool calls when resuming
    #[arg(long = "approve-all", default_value_t = false)]
    approve_all: bool,

    /// Reject all pending tool calls when resuming
    #[arg(long = "reject-all", default_value_t = false)]
    reject_all: bool,

    /// Ignore AGENTS.md files (skip discovery and injection)
    #[arg(long = "ignore-agents-md", default_value_t = false)]
    ignore_agents_md: bool,

    /// Ignore APPS.md files (skip discovery and injection)
    #[arg(long = "ignore-apps-md", default_value_t = false)]
    ignore_apps_md: bool,

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
    config_path: Option<PathBuf>,

    /// Model to use (e.g., "claude-opus-4-5", "claude-haiku-4-5", "gpt-5.2")
    #[arg(long = "model")]
    model: Option<String>,

    /// Prompt to run the agent
    prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[tokio::main]
async fn main() {
    // Initialize rustls crypto provider
    let _ = CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider());

    // Handle default for "stakpak config" -> "stakpak config list"
    let args: Vec<String> = std::env::args().collect();
    let mut modified_args = args.clone();
    if args.len() == 2 && args[1] == "config" {
        modified_args.push("list".to_string());
    }

    let cli = if modified_args != args {
        Cli::parse_from(&modified_args)
    } else {
        Cli::parse()
    };

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
            // Check if warden is enabled in profile and we're not already inside warden
            let should_use_warden = config.warden.as_ref().map(|w| w.enabled).unwrap_or(false)
                && std::env::var("STAKPAK_SKIP_WARDEN").is_err()
                && cli.command.is_none(); // Only for main agent, not for subcommands

            if should_use_warden {
                // Re-execute stakpak inside warden container
                if let Err(e) = commands::warden::run_stakpak_in_warden(
                    config,
                    &std::env::args().collect::<Vec<_>>(),
                )
                .await
                {
                    eprintln!("Failed to run stakpak in warden: {}", e);
                    std::process::exit(1);
                }
                return; // Exit after warden execution completes
            }

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

            // Run interactive/async agent when no subcommand or Init; otherwise run the subcommand
            if matches!(cli.command, None | Some(Commands::Init)) {
                // Run onboarding if no credentials are configured at all
                // Check both Stakpak API key AND local provider keys
                let has_stakpak_key = config.get_stakpak_api_key().is_some();
                let has_provider_keys = !config.get_llm_provider_config().providers.is_empty();
                if !has_stakpak_key && !has_provider_keys {
                    run_onboarding(&mut config, OnboardingMode::Default).await;
                }

                let _ = gitignore::ensure_stakpak_in_gitignore(&config);

                let send_init_prompt_on_start = cli.command == Some(Commands::Init);

                // Initialize models cache in background (fetch if missing/stale)
                let cache_task = tokio::spawn(async {
                    if let Err(e) = ModelsCache::get().await {
                        tracing::warn!("Failed to load models cache: {}", e);
                    }
                });

                let local_context = analyze_local_context(&config).await.ok();

                let agents_md = if cli.ignore_agents_md {
                    None
                } else {
                    std::env::current_dir()
                        .ok()
                        .and_then(|cwd| discover_agents_md(&cwd))
                };

                let apps_md = if cli.ignore_apps_md {
                    None
                } else {
                    std::env::current_dir()
                        .ok()
                        .and_then(|cwd| discover_apps_md(&cwd))
                };

                // Use credential resolution with auth.toml fallback chain
                // Refresh OAuth tokens in parallel to minimize startup delay
                let providers = config.get_llm_provider_config_async().await;

                // Create unified AgentClient - automatically routes through Stakpak when API key is present
                let mut client_config = AgentClientConfig::new().with_providers(providers);

                if let Some(api_key) = config.get_stakpak_api_key() {
                    client_config = client_config.with_stakpak(
                        stakpak_api::StakpakConfig::new(api_key)
                            .with_endpoint(config.api_endpoint.clone()),
                    );
                }
                if let Some(smart_model) = &config.smart_model {
                    client_config = client_config.with_smart_model(smart_model.clone());
                }
                if let Some(eco_model) = &config.eco_model {
                    client_config = client_config.with_eco_model(eco_model.clone());
                }
                if let Some(recovery_model) = &config.recovery_model {
                    client_config = client_config.with_recovery_model(recovery_model.clone());
                }

                let client: Arc<dyn AgentProvider> =
                    Arc::new(AgentClient::new(client_config).await.unwrap_or_else(|e| {
                        eprintln!("Failed to create client: {}", e);
                        std::process::exit(1);
                    }));

                // Parallelize HTTP calls for faster startup
                let current_version = format!("v{}", env!("CARGO_PKG_VERSION"));
                let client_for_rulebooks = client.clone();
                let config_for_rulebooks = config.clone();

                let (api_result, update_result, rulebooks_result) = tokio::join!(
                    client.get_my_account(),
                    check_update(&current_version),
                    async {
                        client_for_rulebooks
                            .list_rulebooks()
                            .await
                            .ok()
                            .map(|rulebooks| {
                                if let Some(rulebook_config) = &config_for_rulebooks.rulebooks {
                                    rulebook_config.filter_rulebooks(rulebooks)
                                } else {
                                    rulebooks
                                }
                            })
                    }
                );

                match api_result {
                    Ok(_) => {}
                    Err(e) => {
                        // Only exit on error if using Stakpak API (has API key)
                        if has_stakpak_key {
                            println!();
                            println!("❌ API key validation failed: {}", e);
                            println!("Please check your API key and run the below command");
                            println!();
                            println!("\x1b[1;34mstakpak login --api-key <your-api-key>\x1b[0m");
                            println!();
                            std::process::exit(1);
                        }
                    }
                }

                let _ = update_result;
                let rulebooks = rulebooks_result;

                let enable_subagents = !cli.disable_subagents;
                let skills: Option<Vec<Skill>> = {
                    let mut merged: Vec<Skill> = rulebooks
                        .iter()
                        .flatten()
                        .cloned()
                        .map(Skill::from)
                        .collect();

                    let skill_dirs = default_skill_directories();
                    let local_skills = discover_skills(&skill_dirs);
                    merged.extend(local_skills);

                    if merged.is_empty() {
                        None
                    } else {
                        Some(merged)
                    }
                };

                // match get_or_build_local_code_index(&config, None, cli.index_big_project)
                //     .await
                // {
                //     Ok(_) => {
                //         // Indexing was successful, start the file watcher
                //         tokio::spawn(async move {
                //             match start_code_index_watcher(&config, None) {
                //                 Ok(_) => {}
                //                 Err(e) => {
                //                     eprintln!("Failed to start code index watcher: {}", e);
                //                 }
                //             }
                //         });
                //     }
                //     Err(e) if e.contains("threshold") && e.contains("--index-big-project") => {
                //         // This is the expected error when file count exceeds limit
                //         // Continue silently without file watcher
                //     }
                //     Err(e) => {
                //         eprintln!("Failed to build code index: {}", e);
                //         // Continue without code indexing instead of exiting
                //     }
                // }

                let system_prompt = if let Some(system_prompt_file_path) = &cli.system_prompt_file {
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
                            if cli.output_format != OutputFormat::Json {
                                println!("📖 Reading prompt from file: {}", prompt_file_path);
                            }
                            content.trim().to_string()
                        }
                        Err(e) => {
                            eprintln!("Failed to read prompt file '{}': {}", prompt_file_path, e);
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
                let default_model = config.get_default_model(cli.model.as_deref());
                let checkpoint_id = cli.checkpoint_id.clone();
                let session_id = cli.session_id.clone();

                let result = match use_async_mode {
                    // Async mode: run continuously until no more tool calls (or max_steps=1 for single-step)
                    true => {
                        let async_result = agent::run::run_async(
                            config,
                            RunAsyncConfig {
                                prompt,
                                verbose: cli.verbose,
                                checkpoint_id: checkpoint_id.clone(),
                                session_id: session_id.clone(),
                                local_context,
                                redact_secrets: !cli.disable_secret_redaction,
                                privacy_mode: cli.privacy_mode,
                                enable_subagents,
                                skills,
                                max_steps,
                                output_format: cli.output_format,
                                enable_mtls: !cli.disable_mcp_mtls,
                                allowed_tools,
                                system_prompt,
                                enabled_tools: EnabledToolsConfig {
                                    slack: cli.enable_slack_tools,
                                },
                                model: default_model.clone(),
                                agents_md: agents_md.clone(),
                                apps_md: apps_md.clone(),
                                plan_mode: cli.plan,
                                plan_approved: cli.plan_approved,
                                plan_feedback: cli.plan_feedback.clone(),
                                plan_new: cli.plan_new,
                                pause_on_approval: cli.pause_on_approval,
                                resume_input: if cli.approve.is_some()
                                    || cli.reject.is_some()
                                    || cli.approve_all
                                    || cli.reject_all
                                {
                                    Some(ResumeInput {
                                        approved: cli
                                            .approve
                                            .unwrap_or_default()
                                            .into_iter()
                                            .collect(),
                                        rejected: cli
                                            .reject
                                            .unwrap_or_default()
                                            .into_iter()
                                            .collect(),
                                        approve_all: cli.approve_all,
                                        reject_all: cli.reject_all,
                                        prompt: None,
                                    })
                                } else {
                                    None
                                },
                                auto_approve_tools: None,
                            },
                        )
                        .await;

                        // Handle AsyncOutcome → exit code
                        match async_result {
                            Ok(AsyncOutcome::Paused { .. }) => {
                                cache_task.abort();
                                std::process::exit(EXIT_CODE_PAUSED);
                            }
                            Ok(AsyncOutcome::Completed { .. }) => Ok(()),
                            Ok(AsyncOutcome::Failed { error }) => Err(error),
                            Err(e) => Err(e),
                        }
                    }

                    // Interactive mode: run in TUI
                    false => {
                        agent::run::run_interactive(
                            config,
                            RunInteractiveConfig {
                                checkpoint_id,
                                session_id,
                                local_context,
                                rulebooks,
                                redact_secrets: !cli.disable_secret_redaction,
                                privacy_mode: cli.privacy_mode,
                                enable_subagents,
                                skills,
                                enable_mtls: !cli.disable_mcp_mtls,
                                is_git_repo: gitignore::is_git_repo(),
                                study_mode: cli.study_mode,
                                plan_mode: cli.plan,
                                system_prompt,
                                allowed_tools,
                                auto_approve,
                                enabled_tools: EnabledToolsConfig {
                                    slack: cli.enable_slack_tools,
                                },
                                model: default_model,
                                agents_md,
                                apps_md,
                                send_init_prompt_on_start,
                            },
                        )
                        .await
                    }
                };

                // Cancel background cache task on exit
                cache_task.abort();

                if let Err(e) = result {
                    eprintln!("Ops! something went wrong: {}", e);
                    std::process::exit(1);
                }
            } else if let Some(command) = cli.command {
                // Run a specific subcommand (Account, Config, etc.)
                if config.get_stakpak_api_key().is_none() && command.requires_auth() {
                    run_onboarding(&mut config, OnboardingMode::Default).await;
                }
                let _ = gitignore::ensure_stakpak_in_gitignore(&config);
                match command.run(config).await {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Ops! something went wrong: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        Err(e) => eprintln!("Failed to load config: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_session_flag() {
        let parsed = Cli::try_parse_from(["stakpak", "-s", "session-id", "hello"]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            assert_eq!(cli.session_id, Some("session-id".to_string()));
            assert_eq!(cli.checkpoint_id, None);
            assert_eq!(cli.prompt, Some("hello".to_string()));
        }
    }

    #[test]
    fn cli_rejects_checkpoint_and_session_together() {
        let parsed = Cli::try_parse_from([
            "stakpak",
            "-c",
            "checkpoint-id",
            "-s",
            "session-id",
            "hello",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn cli_parses_up_alias_foreground_flag() {
        let parsed = Cli::try_parse_from(["stakpak", "up", "--foreground"]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            match cli.command {
                Some(Commands::Up { args }) => {
                    assert!(args.foreground);
                }
                _ => panic!("Expected up command"),
            }
        }
    }

    #[test]
    fn cli_parses_up_defaults_to_background() {
        let parsed = Cli::try_parse_from(["stakpak", "up"]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            match cli.command {
                Some(Commands::Up { args }) => {
                    assert!(!args.foreground);
                }
                _ => panic!("Expected up command"),
            }
        }
    }

    #[test]
    fn cli_parses_down_alias_uninstall_flag() {
        let parsed = Cli::try_parse_from(["stakpak", "down", "--uninstall"]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            match cli.command {
                Some(Commands::Down { args }) => {
                    assert!(args.uninstall);
                }
                _ => panic!("Expected down command"),
            }
        }
    }

    #[test]
    fn cli_parses_up_non_interactive_and_force_flags() {
        let parsed = Cli::try_parse_from([
            "stakpak",
            "up",
            "--non-interactive",
            "--force",
            "--foreground",
        ]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            match cli.command {
                Some(Commands::Up { args }) => {
                    assert!(args.non_interactive);
                    assert!(args.force);
                    assert!(args.foreground);
                }
                _ => panic!("Expected up command"),
            }
        }
    }

    #[test]
    fn cli_rejects_profile_flag_on_up() {
        let parsed = Cli::try_parse_from(["stakpak", "up", "--profile", "staging"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn cli_accepts_root_profile_with_up() {
        let parsed = Cli::try_parse_from(["stakpak", "--profile", "staging", "up"]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            assert_eq!(cli.profile.as_deref(), Some("staging"));
            match cli.command {
                Some(Commands::Up { .. }) => {}
                _ => panic!("Expected up command"),
            }
        }
    }

    #[test]
    fn cli_rejects_profile_flag_on_autopilot_status() {
        let parsed =
            Cli::try_parse_from(["stakpak", "autopilot", "status", "--profile", "staging"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn cli_parses_auth_login_endpoint_flag() {
        let parsed = Cli::try_parse_from([
            "stakpak",
            "auth",
            "login",
            "--provider",
            "stakpak",
            "--api-key",
            "test-key",
            "--endpoint",
            "https://self-hosted.example.com",
        ]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            match cli.command {
                Some(Commands::Auth(commands::AuthCommands::Login { endpoint, .. })) => {
                    assert_eq!(endpoint.as_deref(), Some("https://self-hosted.example.com"));
                }
                _ => panic!("Expected auth login command"),
            }
        }
    }

    #[test]
    fn autopilot_related_commands_do_not_require_auth() {
        assert!(
            !Commands::Autopilot(commands::AutopilotCommands::Status {
                json: false,
                recent_runs: None,
            })
            .requires_auth()
        );

        assert!(
            !Commands::Up {
                args: commands::autopilot::StartArgs {
                    bind: "127.0.0.1:4096".to_string(),
                    show_token: false,
                    no_auth: false,
                    model: None,
                    auto_approve_all: false,
                    foreground: false,
                    non_interactive: false,
                    force: false,
                },
            }
            .requires_auth()
        );

        assert!(
            !Commands::Down {
                args: commands::autopilot::StopArgs { uninstall: false },
            }
            .requires_auth()
        );
    }
}
