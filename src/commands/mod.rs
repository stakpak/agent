use clap::Subcommand;

use crate::config::AppConfig;

#[derive(Subcommand)]
pub enum Commands {
    /// Login to Stakpak
    Login {
        /// API key for authentication
        #[arg(long, env("STAKPAK_API_KEY"))]
        api_key: String,
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
    pub fn run(self, config: AppConfig) {
        match self {
            Commands::Login { api_key } => {
                let mut updated_config = config.clone();
                updated_config.api_key = Some(api_key);

                println!("Storing credentials...");
                if let Err(e) = updated_config.save() {
                    eprintln!("Failed to save config: {}", e);
                    return;
                }
                println!("Logged in successfully");
            }
            Commands::Logout => {
                let mut updated_config = config.clone();
                updated_config.api_key = None;

                println!("Removing credentials...");
                if let Err(e) = updated_config.save() {
                    eprintln!("Failed to save config: {}", e);
                    return;
                }
                println!("Logged out successfully");
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
