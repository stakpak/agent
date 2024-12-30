use agent::AgentCommands;
use clap::Subcommand;
use termimad::MadSkin;

use crate::{
    client::{models::FlowRef, Client},
    config::AppConfig,
};

pub mod agent;

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
    /// Stakpak Agent (WARNING: These agents are in early alpha development and may be unstable)
    #[command(subcommand)]
    Agent(AgentCommands),
    // /// Import existing configurations
    // Import,
}

impl Commands {
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
            Commands::Login { api_key } => {
                let mut updated_config = config.clone();
                updated_config.api_key = Some(api_key);

                updated_config
                    .save()
                    .map_err(|e| format!("Failed to save config: {}", e))?;
            }
            Commands::Logout => {
                let mut updated_config = config.clone();
                updated_config.api_key = None;

                updated_config
                    .save()
                    .map_err(|e| format!("Failed to save config: {}", e))?;
            }
            Commands::Account => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let data = client.get_my_account().await?;
                println!("{}", data.to_text());
            }
            Commands::List => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let owner_name = client.get_my_account().await?.username;
                let data = client.list_flows(&owner_name).await?;
                println!("{}", data.to_text(&owner_name));
            }
            Commands::Get { flow_ref } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let parts: Vec<&str> = flow_ref.split('/').collect();

                let (owner_name, flow_name) = if parts.len() == 2 {
                    (parts[0], parts[1])
                } else {
                    return Err("Flow ref must be of the format <owner name>/<flow name>".into());
                };

                let data = client.get_flow(owner_name, flow_name).await?;
                println!("{}", data.to_text(owner_name));
            }
            Commands::Clone { flow_ref, dir } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let parts: Vec<&str> = flow_ref.split('/').collect();

                let flow_ref = if parts.len() == 2 {
                    let owner_name = parts[0];
                    let flow_name = parts[1];

                    let res = client.get_flow(owner_name, flow_name).await?;

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
                    FlowRef::new(flow_ref)
                        .map_err(|e| format!("Failed to parse flow ref: {}", e))?
                };

                let documents = client.get_flow_documents(&flow_ref).await?;
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
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("Failed to create directory {}: {}", parent.display(), e)
                        })?;
                    }

                    // Write the files
                    std::fs::write(&full_path, doc.content).map_err(|e| {
                        format!("Failed to write file {}: {}", full_path.display(), e)
                    })?;
                }

                println!("Successfully cloned flow to \"{}\"", base_dir);
            }
            Commands::Query {
                query,
                flow_ref,
                generate_query,
                synthesize_output,
            } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let data = client
                    .query_blocks(
                        &query,
                        generate_query,
                        synthesize_output,
                        flow_ref.as_deref(),
                    )
                    .await?;

                let skin = MadSkin::default();
                println!("{}", skin.inline(&data.to_text(synthesize_output)));
            }
            Commands::Agent(agent_commands) => {
                if let AgentCommands::Get { .. } = agent_commands {
                } else {
                    println!();
                    println!(
                    "[WARNING: These agents are in early alpha development and may be unstable]"
                );
                    println!();
                };

                AgentCommands::run(agent_commands, config).await?;
            }
        }
        Ok(())
    }
}
