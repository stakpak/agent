use clap::Parser;
use std::{env, io::Write, path::Path};

mod commands;
mod config;
mod utils;

use commands::{
    Commands,
    agent::{
        self,
        run::{RunAsyncConfig, RunInteractiveConfig, RunNonInteractiveConfig},
    },
};
use config::AppConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utils::check_update::check_update;
use utils::local_context::analyze_local_context;

#[derive(Parser, PartialEq)]
#[command(name = "stakpak")]
#[command(about = "Stakpak CLI tool", long_about = None)]
struct Cli {
    /// Run the agent in non-interactive mode
    #[arg(short = 'p', long = "print", default_value_t = false)]
    print: bool,

    /// Run the agent in asyncronous mode
    #[arg(short = 'a', long = "async", default_value_t = false)]
    r#async: bool,

    /// Resume agent session at a specific checkpoint
    #[arg(short = 'c', long = "checkpoint")]
    checkpoint_id: Option<String>,

    /// Run the agent in a specific directory
    #[arg(short = 'w', long = "workdir")]
    workdir: Option<String>,

    /// Approve the tool call in non-interactive mode
    #[arg(long = "approve", default_value_t = false)]
    approve: bool,

    /// Enable verbose output in non-interactive mode
    #[arg(long = "verbose", default_value_t = false)]
    verbose: bool,

    /// Enable debug output
    #[arg(long = "debug", default_value_t = false)]
    debug: bool,

    /// Disable secret redaction (WARNING: this will print secrets to the console)
    #[arg(long = "disable-secret-redaction", default_value_t = false)]
    disable_secret_redaction: bool,

    /// Prompt to run the agent with in non-interactive mode
    #[clap(required_if_eq("print", "true"))]
    prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

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

    match AppConfig::load() {
        Ok(config) => {
            if config.api_key.is_none() {
                println!();
                println!("Stakpak API Key not found!");
                println!("1. Login to Stakpak from here: https://stakpak.dev/auth/signin");
                println!("2. Go to your profile in the top right corner, and click on 'API Keys'");
                println!("3. Create a new API Key, and copy it");
                print!("Enter your API Key: ");
                if let Err(e) = std::io::stdout().flush() {
                    eprintln!("Failed to flush stdout: {}", e);
                    std::process::exit(1);
                }

                let api_key = match rpassword::read_password() {
                    Ok(key) => key,
                    Err(e) => {
                        eprintln!("\nFailed to read API key: {}", e);
                        std::process::exit(1);
                    }
                };

                let mut updated_config = config.clone();
                updated_config.api_key = Some(api_key.trim().to_string());
                if let Err(e) = updated_config.save() {
                    eprintln!("Failed to save config: {}", e);
                }
                println!("API Key saved successfully!");
                std::process::exit(0);
            }
            match cli.command {
                Some(command) => {
                    let _ = check_update(format!("v{}", env!("CARGO_PKG_VERSION")).as_str()).await;
                    match command.run(config).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Ops! something went wrong: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    let local_context = analyze_local_context().await.ok();

                    match (cli.r#async, cli.print || cli.approve) {
                        // Async mode: run continuously until no more tool calls
                        (true, _) => match agent::run::run_async(
                            config,
                            RunAsyncConfig {
                                prompt: cli.prompt.unwrap_or_default(),
                                verbose: cli.verbose,
                                checkpoint_id: cli.checkpoint_id,
                                local_context,
                                redact_secrets: !cli.disable_secret_redaction,
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

                        // Non-interactive mode: run one step at a time
                        (false, true) => match agent::run::run_non_interactive(
                            config,
                            RunNonInteractiveConfig {
                                prompt: cli.prompt.unwrap_or_default(),
                                approve: cli.approve,
                                verbose: cli.verbose,
                                checkpoint_id: cli.checkpoint_id,
                                local_context,
                                redact_secrets: !cli.disable_secret_redaction,
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
                        (false, false) => match agent::run::run_interactive(
                            config,
                            RunInteractiveConfig {
                                checkpoint_id: cli.checkpoint_id,
                                local_context,
                                redact_secrets: !cli.disable_secret_redaction,
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
