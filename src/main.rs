use clap::Parser;

mod client;
mod commands;
mod config;
mod utils;

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
        Ok(config) => match Commands::run(cli.command, config).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Ops! something went wrong: {}", e);
                std::process::exit(1);
            }
        },
        Err(e) => eprintln!("Failed to load config: {}", e),
    }
}
