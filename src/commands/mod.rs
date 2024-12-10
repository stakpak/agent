use clap::Subcommand;

use crate::config::Config;

#[derive(Subcommand)]
pub enum Commands {
    /// Login to Stakpak
    Login {
        /// API key for authentication
        #[arg(long)]
        api_key: Option<String>,
    },

    /// Logout from Stakpak
    Logout,

    /// Deploy your app
    Deploy,

    /// Import existing configurations
    Import,

    /// Clone a Stakpak project
    Clone,
}

impl Commands {
    pub fn run(self, _config: Config) {
        match self {
            Commands::Login { api_key: _ } => {
                println!("Logging in...");
                // Add login logic here
            }
            Commands::Logout => {
                println!("Logging out...");
                // Add logout logic here
            }
            Commands::Deploy => {
                println!("Deploying...");
                // Add deploy logic here
            }
            Commands::Import => {
                println!("Importing...");
                // Add import logic here
            }
            Commands::Clone => {
                println!("Cloning...");
                // Add clone logic here
            }
        }
    }
}
