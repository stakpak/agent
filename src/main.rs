use clap::Parser;

mod commands;
mod config;

use commands::Commands;

#[derive(Parser)]
#[command(name = "stakpak")]
#[command(about = "Stakpak CLI tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn main() {
    let cli = Cli::parse();
    let config = config::load_config();
    cli.command.run(config)
}
