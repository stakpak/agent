use clap::Subcommand;

use crate::{client::Client, config::AppConfig};

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

    /// Get current account
    Account,

    /// List my flows
    List,
    // /// Deploy your app
    // Deploy,

    // /// Import existing configurations
    // Import,

    // /// Clone a Stakpak project
    // Clone,
}

impl Commands {
    pub async fn run(self, config: AppConfig) {
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
            Commands::Account => {
                if let Ok(client) = Client::new(&config) {
                    match client.get_my_account().await {
                        Ok(data) => println!("{}", serde_json::to_string_pretty(&data).unwrap()),
                        Err(e) => eprintln!("Failed to fetch account {}", e),
                    };
                }
            }
            Commands::List => {
                if let Ok(client) = Client::new(&config) {
                    let owner_name = match client.get_my_account().await {
                        Ok(data) => data.username,
                        Err(e) => {
                            eprintln!("Failed to fetch account {}", e);
                            return;
                        }
                    };
                    match client.list_flows(&owner_name).await {
                        Ok(data) => println!("{}", serde_json::to_string_pretty(&data).unwrap()),
                        Err(e) => eprintln!("Failed to fetch account {}", e),
                    };
                }
            } // Commands::Deploy => {
              //     println!("Deploying...");
              //     // Add deploy logic here
              // }
              // Commands::Import => {
              //     println!("Importing...");
              //     // Add import logic here
              // }
              // Commands::Clone => {
              //     println!("Cloning...");
              //     // Add clone logic here
              // }
        }
    }
}
