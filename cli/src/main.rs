use clap::Parser;
use names::{self, Name};
use stakpak_api::{Client, ClientConfig};
use std::{env, io::Write, path::Path};

mod code_index;
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

use crate::code_index::{get_or_build_local_code_index, start_code_index_watcher};

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

    /// Maximum number of steps the agent can take in async mode
    #[arg(short = 'm', long = "max-steps")]
    max_steps: Option<usize>,

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

    /// Allow indexing of large projects (more than 500 supported files)
    #[arg(long = "index-big-project", default_value_t = false)]
    index_big_project: bool,

    /// Disable official rulebooks in the agent's context
    #[arg(long = "disable-official-rulebooks", default_value_t = false)]
    disable_official_rulebooks: bool,

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
        Ok(mut config) => {
            let mut config_updated = false;

            if config.api_key.is_none() {
                println!();
                println!("Stakpak API Key not found!");
                println!(
                    "- Go to https://stakpak.dev/generate-api-key. Get your api key and paste it below"
                );
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

                config.api_key = Some(api_key.trim().to_string());
                config_updated = true;
                println!("API Key saved successfully!");
            }

            if config.machine_name.is_none() {
                // Generate a random machine name
                let random_name = names::Generator::with_naming(Name::Numbered)
                    .next()
                    .unwrap_or_else(|| "unknown-machine".to_string());

                config.machine_name = Some(random_name);
                config_updated = true;
            }

            if config_updated {
                if let Err(e) = config.save() {
                    eprintln!("Failed to save config: {}", e);
                }
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
                    let local_context = analyze_local_context(&config).await.ok();
                    let api_config: ClientConfig = config.clone().into();
                    let client = if let Ok(client) = Client::new(&api_config) {
                        client
                    } else {
                        eprintln!("Failed to create client");
                        std::process::exit(1);
                    };
                    let rulebooks = client.list_rulebooks().await.ok().map(|rulebooks| {
                        rulebooks
                            .into_iter()
                            .filter(|rulebook| {
                                !cli.disable_official_rulebooks
                                    || !rulebook.uri.starts_with("stakpak://stakpak.dev/")
                            })
                            .collect()
                    });

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
                            std::process::exit(1);
                        }
                    }

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
                                rulebooks,
                                max_steps: cli.max_steps,
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
                                rulebooks,
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
                                rulebooks,
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
