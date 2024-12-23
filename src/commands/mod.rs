use clap::Subcommand;

use crate::{
    client::{models::FlowRef, Client},
    config::AppConfig,
};

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
    Get {
        /// Flow reference in format: <owner_name>/<flow_name>
        flow_ref: String,
    },

    /// Clone a Stakpak project
    Clone {
        /// Flow reference in format: <owner_name>/<flow_name>(/<version_id_or_tag>)?
        #[arg(name = "flow-ref")]
        flow_ref: String,
        /// Destination directory
        #[arg(long, short)]
        dir: Option<String>,
    },

    /// Query your configurations
    Query {
        /// Query string to search/prompt for over your flows
        query: String,
        /// Limit the query to a specific flow reference in format: <owner_name>/<flow_name>/<version_id_or_tag>
        #[arg(long, short)]
        flow_ref: Option<String>,
        /// Re-generate the semantic query used to find code blocks with natural language
        #[arg(long, short)]
        generate_query: bool,
        /// Synthesize output with an LLM into a custom response
        #[arg(long, short = 'o')]
        synthesize_output: bool,
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
            Commands::Get { flow_ref } => {
                if let Ok(client) = Client::new(&config) {
                    let parts: Vec<&str> = flow_ref.split('/').collect();

                    let (owner_name, flow_name) = if parts.len() == 2 {
                        (parts[0], parts[1])
                    } else {
                        eprintln!("Flow ref must be of the format <owner name>/<flow name>");
                        return;
                    };

                    match client.get_flow(owner_name, flow_name).await {
                        Ok(data) => println!("{}", data.to_text(owner_name)),
                        Err(e) => eprintln!("Failed to fetch account {}", e),
                    };
                }
            }
            Commands::Clone { flow_ref, dir } => {
                if let Ok(client) = Client::new(&config) {
                    let parts: Vec<&str> = flow_ref.split('/').collect();

                    let flow_ref = if parts.len() == 2 {
                        let owner_name = parts[0];
                        let flow_name = parts[1];

                        let res = match client.get_flow(owner_name, flow_name).await {
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

                        FlowRef::Version {
                            owner_name: owner_name.to_string(),
                            flow_name: flow_name.to_string(),
                            version_id: latest_version.id.to_string(),
                        }
                    } else {
                        match FlowRef::new(flow_ref) {
                            Ok(flow_ref) => flow_ref,
                            Err(e) => {
                                eprintln!("Failed to parse flow ref: {}", e);
                                return;
                            }
                        }
                    };

                    let documents = match client.get_flow_documents(&flow_ref).await {
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
            Commands::Query {
                query,
                flow_ref,
                generate_query,
                synthesize_output,
            } => {
                if let Ok(client) = Client::new(&config) {
                    match client
                        .query_blocks(
                            &query,
                            generate_query,
                            synthesize_output,
                            flow_ref.as_deref(),
                        )
                        .await
                    {
                        Ok(data) => println!("{}", data.to_text(synthesize_output)),
                        Err(e) => eprintln!("Failed to query blocks {}", e),
                    };
                }
            }
        }
    }
}
