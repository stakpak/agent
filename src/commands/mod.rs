use std::collections::{HashMap, HashSet};

use agent::AgentCommands;
use chrono::Utc;
use clap::Subcommand;
use termimad::MadSkin;
use walkdir::WalkDir;

use crate::{
    client::{
        models::{Document, FlowRef},
        Client, Edit,
    },
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

    /// Clone configurations from a flow
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

    /// Push configurations to a flow
    Push {
        /// Flow reference in format: <owner_name>/<flow_name>(/<version_id_or_tag>)?
        #[arg(name = "flow-ref")]
        flow_ref: String,
        /// Create a new index
        #[arg(long, short, default_value_t = false)]
        create: bool,
        /// Source directory
        #[arg(long, short)]
        dir: Option<String>,
        /// Ignore delete operations
        #[arg(long, default_value_t = false)]
        ignore_delete: bool,
        /// Auto approve all changes
        #[arg(long, short = 'y', default_value_t = false)]
        auto_approve: bool,
    },

    /// Stakpak Agent (WARNING: These agents are in early alpha development and may be unstable)
    #[command(subcommand)]
    Agent(AgentCommands),
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

                let flow_ref = match parts.len() {
                    3 => FlowRef::Version {
                        owner_name: parts[0].to_string(),
                        flow_name: parts[1].to_string(),
                        version_id: parts[2].to_string(),
                    },
                    2 => {
                        let owner_name = parts[0];
                        let flow_name = parts[1];

                        let res = client.get_flow(owner_name, flow_name).await?;

                        let latest_version = res
                            .resource
                            .versions
                            .iter()
                            .max_by_key(|v| v.created_at)
                            .ok_or("No versions found")?;

                        FlowRef::Version {
                            owner_name: owner_name.to_string(),
                            flow_name: flow_name.to_string(),
                            version_id: latest_version.id.to_string(),
                        }
                    }
                    _ => FlowRef::new(flow_ref)
                        .map_err(|e| format!("Failed to parse flow ref: {}", e))?,
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

                    println!("Cloned {} -> \"{}\"", doc.uri, full_path.display());
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
            Commands::Push {
                flow_ref,
                create,
                dir,
                ignore_delete,
                auto_approve,
            } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let parts: Vec<&str> = flow_ref.split('/').collect();

                let flow_ref = match parts.len() {
                    3 => FlowRef::Version {
                        owner_name: parts[0].to_string(),
                        flow_name: parts[1].to_string(),
                        version_id: parts[2].to_string(),
                    },
                    2 => {
                        let owner_name = parts[0];
                        let flow_name = parts[1];

                        if create {
                            let result = client.create_flow(flow_name, None).await?;
                            FlowRef::Version {
                                owner_name: owner_name.to_string(),
                                flow_name: flow_name.to_string(),
                                version_id: result.version_id.to_string(),
                            }
                        } else {
                            let result = client.get_flow(owner_name, flow_name).await?;

                            let latest_version = result
                                .resource
                                .versions
                                .iter()
                                .max_by_key(|v| v.created_at)
                                .ok_or("No versions found")?;

                            FlowRef::Version {
                                owner_name: owner_name.to_string(),
                                flow_name: flow_name.to_string(),
                                version_id: latest_version.id.to_string(),
                            }
                        }
                    }
                    _ => FlowRef::new(flow_ref)
                        .map_err(|e| format!("Failed to parse flow ref: {}", e))?,
                };

                println!("Pushing to flow version: {}\n", flow_ref);

                let documents_map: HashMap<String, Document> = client
                    .get_flow_documents(&flow_ref)
                    .await?
                    .documents
                    .into_iter()
                    .map(|doc| (doc.uri.clone(), doc))
                    .collect();

                let base_dir = dir.unwrap_or_else(|| ".".into());

                let mut edits = Vec::new();
                let mut processed_uris = HashSet::new();
                let mut files_synced = 0;
                let mut files_deleted = 0;

                for entry in WalkDir::new(&base_dir)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| {
                        // Skip hidden directories and non-supported files
                        let file_name = e.file_name().to_str();
                        match file_name {
                            Some(name) => {
                                // Skip hidden files/dirs that aren't just "."
                                if name.starts_with('.') && name.len() > 1 {
                                    return false;
                                }
                                // Only allow supported files
                                if e.file_type().is_file() {
                                    name.ends_with(".tf")
                                        || name.ends_with(".yaml")
                                        || name.ends_with(".yml")
                                        || name.to_lowercase().contains("dockerfile")
                                } else {
                                    true // Allow directories to be traversed
                                }
                            }
                            None => false,
                        }
                    })
                    .filter_map(|e| e.ok())
                {
                    // Skip directories
                    if !entry.file_type().is_file() {
                        continue;
                    }

                    let path = entry.path();
                    // Skip binary files by attempting to read as UTF-8 and checking for errors
                    let content = match std::fs::read_to_string(path) {
                        Ok(content) => content,
                        Err(_) => continue, // Skip file if it can't be read as valid UTF-8
                    };

                    // Convert path to URI format
                    let document_uri = format!(
                        "file:///{}",
                        path.strip_prefix(&base_dir)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    );
                    processed_uris.insert(document_uri.clone());

                    if let Some(document) = documents_map.get(&document_uri) {
                        if content == document.content {
                            // println!("\tunchanged:\t{}", document_uri);
                            continue;
                        }

                        println!("\tmodified:\t{}", document_uri);
                        edits.extend([
                            Edit {
                                document_uri: document_uri.clone(),

                                start_byte: 0,
                                start_row: 0,
                                start_column: 0,

                                end_byte: document.content.len(),
                                end_row: document.content.lines().count(),
                                end_column: document
                                    .content
                                    .lines()
                                    .last()
                                    .map_or(0, |line| line.len()),

                                content: document.content.to_owned(),

                                language: "".to_string(),
                                operation: "delete".to_string(),
                                timestamp: Utc::now(),
                            },
                            Edit {
                                document_uri,

                                start_byte: 0,
                                start_row: 0,
                                start_column: 0,

                                end_byte: content.len(),
                                end_row: content.lines().count(),
                                end_column: content.lines().last().map_or(0, |line| line.len()),

                                content,

                                language: "".to_string(),
                                operation: "insert".to_string(),
                                timestamp: Utc::now(),
                            },
                        ]);
                    } else {
                        println!("\tadded:\t{}", document_uri);
                        edits.push(Edit {
                            document_uri,

                            start_byte: 0,
                            start_row: 0,
                            start_column: 0,

                            end_byte: content.len(),
                            end_row: content.lines().count(),
                            end_column: content.lines().last().map_or(0, |line| line.len()),

                            content,

                            language: "".to_string(),
                            operation: "insert".to_string(),
                            timestamp: Utc::now(),
                        });
                    };

                    files_synced += 1;
                }

                if !ignore_delete {
                    // Handle deleted files
                    for (uri, document) in documents_map {
                        if !processed_uris.contains(&uri) {
                            println!("\tdeleted:\t{}", uri);
                            edits.push(Edit {
                                document_uri: uri,
                                start_byte: 0,
                                start_row: 0,
                                start_column: 0,
                                end_byte: document.content.len(),
                                end_row: document.content.lines().count(),
                                end_column: document
                                    .content
                                    .lines()
                                    .last()
                                    .map_or(0, |line| line.len()),
                                content: "".to_string(),
                                language: "".to_string(),
                                operation: "delete".to_string(),
                                timestamp: Utc::now(),
                            });
                            files_deleted += 1;
                        }
                    }
                }

                let total_changes = files_deleted + files_synced;

                if total_changes == 0 {
                    println!("No changes found");
                    return Ok(());
                }

                println!("\nSyncing {} files", files_synced);
                println!("Deleting {} files", files_deleted);

                if !auto_approve {
                    println!("\nDo you want to continue? Type 'yes' to confirm: ");
                    let mut input = String::new();
                    std::io::stdin()
                        .read_line(&mut input)
                        .map_err(|e| format!("Failed to read input: {}", e))?;

                    if input.trim() != "yes" {
                        return Ok(());
                    }
                }

                let save_result = client.save_edits(&flow_ref, edits).await?;
                let total_blocks =
                    save_result.created_blocks.len() + save_result.modified_blocks.len();

                if total_blocks > 0 {
                    println!(
                        "Please wait {:.2} minutes for indexing to complete",
                        total_blocks as f64 * 1.5 / 60.0
                    );
                }
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
