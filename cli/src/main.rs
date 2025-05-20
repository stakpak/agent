use clap::Parser;

mod client;
mod commands;
mod config;
mod utils;

use commands::{Commands, agent};
use config::AppConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utils::check_update::check_update;

#[derive(Parser)]
#[command(name = "stakpak")]
#[command(about = "Stakpak CLI tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let _ = check_update(format!("v{}", env!("CARGO_PKG_VERSION")).as_str()).await;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("error,{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    match AppConfig::load() {
        Ok(config) => match cli.command {
            Some(command) => match command.run(config).await {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Ops! something went wrong: {}", e);
                    std::process::exit(1);
                }
            },
            None => match agent::code::run(config).await {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Ops! something went wrong: {}", e);
                    std::process::exit(1);
                }
            },
        },
        Err(e) => eprintln!("Failed to load config: {}", e),
    }
}
