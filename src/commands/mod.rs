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

    /// Get a flow
    Get { flow_name: String },

    /// Clone a Stakpak project
    Clone {
        flow_name: String,
        flow_version: Option<String>,
        /// Destination directory
        #[arg(long, short)]
        dir: Option<String>,
    },
    // /// Deploy your app
    // Deploy,

    // /// Import existing configurations
    // Import,
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
                        Ok(data) => println!("{}", data.to_text()),
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
                        Ok(data) => println!("{}", data.to_text()),
                        Err(e) => eprintln!("Failed to fetch account {}", e),
                    };
                }
            }
            Commands::Get { flow_name } => {
                if let Ok(client) = Client::new(&config) {
                    let owner_name = match client.get_my_account().await {
                        Ok(data) => data.username,
                        Err(e) => {
                            eprintln!("Failed to fetch account {}", e);
                            return;
                        }
                    };
                    match client.get_flow(&owner_name, &flow_name).await {
                        Ok(data) => println!("{}", data.to_text()),
                        Err(e) => eprintln!("Failed to fetch account {}", e),
                    };
                }
            }
            Commands::Clone {
                flow_name,
                flow_version,
                dir,
            } => {
                if let Ok(client) = Client::new(&config) {
                    let owner_name = match client.get_my_account().await {
                        Ok(data) => data.username,
                        Err(e) => {
                            eprintln!("Failed to fetch account {}", e);
                            return;
                        }
                    };

                    let version = match flow_version {
                        Some(flow_version) => flow_version,
                        None => {
                            let res = match client.get_flow(&owner_name, &flow_name).await {
                                Ok(data) => data,
                                Err(e) => {
                                    eprintln!("Failed to fetch flow {}", e);
                                    return;
                                }
                            };
                            let latest_version = res
                                .resource
                                .versions
                                .iter()
                                .max_by_key(|v| v.created_at)
                                .unwrap_or_else(|| &res.resource.versions[0]);
                            latest_version.id.to_string()
                        }
                    };

                    let documents = match client
                        .get_flow_documents(&owner_name, &flow_name, &version)
                        .await
                    {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("Failed to fetch documents {}", e);
                            return;
                        }
                    };

                    let base_dir = dir.unwrap_or_else(|| ".".into());

                    for doc in documents
                        .documents
                        .into_iter()
                        .chain(documents.additional_documents)
                    {
                        let path = doc.uri.strip_prefix("file:///").unwrap_or(&doc.uri);
                        let full_path = std::path::Path::new(&base_dir).join(path);

                        // Create parent directories if they don't exist
                        if let Some(parent) = full_path.parent() {
                            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                                eprintln!("Failed to create directory {}: {}", parent.display(), e);
                            });
                        }

                        // Write the files
                        std::fs::write(&full_path, doc.content).unwrap_or_else(|e| {
                            eprintln!("Failed to write file {}: {}", full_path.display(), e);
                        });
                    }

                    println!("Successfully cloned flow to \"{}\"", base_dir);
                }
            }
        }
    }
}
