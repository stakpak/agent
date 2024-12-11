use clap::Parser;

mod client;
mod commands;
mod config;

use commands::Commands;
use config::AppConfig;

#[derive(Parser)]
#[command(name = "stakpak")]
#[command(about = "Stakpak CLI tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match AppConfig::load() {
        Ok(config) => cli.command.run(config).await,
        Err(e) => eprintln!("Failed to load config: {}", e),
    }
}
