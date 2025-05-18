use agent::{AgentCommands, get_or_create_session, run_agent};
use clap::Subcommand;
use flow::{clone, get_flow_ref, push, sync};
use stakpak_shared::models::integrations::openai::ChatMessage;
use stakpak_tui::Msg;
use termimad::MadSkin;
use walkdir::WalkDir;

use crate::{
    client::{
        Client,
        models::{AgentID, Document, ProvisionerType, TranspileTargetProvisionerType},
    },
    config::AppConfig,
};

pub mod agent;
pub mod flow;

#[derive(Subcommand)]
pub enum Commands {
    /// Get CLI Version
    Version,
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

    /// Sync configurations from and to a flow
    Sync {
        /// Flow reference in format: <owner_name>/<flow_name>(/<version_id_or_tag>)?
        #[arg(name = "flow-ref")]
        flow_ref: String,
        /// Source/Destination directory
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

    /// Apply configurations
    Apply {
        /// Flow reference in format: <owner_name>/<flow_name>(/<version_id_or_tag>)?
        #[arg(name = "flow-ref")]
        flow_ref: String,

        /// Target directory
        #[arg(long, short)]
        dir: Option<String>,

        /// Provisioner type to apply (terraform, kubernetes, dockerfile, github-actions)
        #[arg(long, short = 'p')]
        provisioner: Option<ProvisionerType>,
    },

    /// Transpile configurations
    Transpile {
        /// Source directory
        #[arg(long, short)]
        dir: Option<String>,

        /// Source DSL to transpile from (currently only supports terraform)
        #[arg(long, short = 's')]
        source_provisioner: ProvisionerType,

        /// Target DSL to transpile to (currently only supports eraser)
        #[arg(long, short = 't')]
        target_provisioner: TranspileTargetProvisionerType,
    },
    // Open the coding assistant
    Code,

    /// Start the MCP server
    Mcp,

    /// Stakpak Agent (WARNING: These agents are in early alpha development and may be unstable)
    #[command(subcommand)]
    Agent(AgentCommands),
}

impl Commands {
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
            Commands::Code => {
                let mut messages: Vec<ChatMessage> = Vec::new();
                let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Msg>(100);
                let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<String>(100);

                let tui_handle = tokio::spawn(async move {
                    let _ = stakpak_tui::run_tui(input_rx, output_tx)
                        .await
                        .map_err(|e| e.to_string());
                });

                let client_handle: tokio::task::JoinHandle<Result<(), String>> = tokio::spawn(
                    async move {
                        let client = Client::new(&config).map_err(|e| e.to_string())?;
                        while let Some(user_input) = output_rx.recv().await {
                            messages.push(ChatMessage {
                                role: stakpak_shared::models::integrations::openai::Role::User,
                                content: Some(
                                    stakpak_shared::models::integrations::openai::MessageContent::String(
                                    user_input,
                                ),
                            ),
                                name: None,
                                tool_calls: None,
                                tool_call_id: None,
                            });

                            let response = match client.chat_completion(messages.clone()).await {
                                Ok(response) => response,
                                Err(e) => {
                                    input_tx.send(Msg::Quit).await.map_err(|e| e.to_string())?;
                                    return Err(e.to_string());
                                }
                            };

                            messages.push(response.choices[0].message.clone());

                            input_tx
                            .send(Msg::InputSubmittedWith(
                                response.choices[0]
                                    .message
                                    .content
                                    .clone()
                                    .unwrap_or(stakpak_shared::models::integrations::openai::MessageContent::String("".to_string()))
                                    .to_string(),
                            ))
                            .await
                            .map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    },
                );

                let (_, client_res) =
                    tokio::try_join!(tui_handle, client_handle).map_err(|e| e.to_string())?;
                client_res?; // If your client returns Result<(), String>
            }
            Commands::Mcp => {
                stakpak_mcp_server::start_server()
                    .await
                    .map_err(|e| e.to_string())?;
            }
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
                let flow_ref = get_flow_ref(&client, flow_ref).await?;
                clone(&client, &flow_ref, dir.as_deref()).await?;
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
            Commands::Sync { flow_ref, dir } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let flow_ref = get_flow_ref(&client, flow_ref).await?;
                sync(&config, &client, &flow_ref, dir.as_deref()).await?;
            }
            Commands::Push {
                flow_ref,
                create,
                dir,
                ignore_delete,
                auto_approve,
            } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;

                let save_result =
                    push(&client, flow_ref, create, dir, ignore_delete, auto_approve).await?;

                if let Some(save_result) = save_result {
                    if !save_result.errors.is_empty() {
                        println!("\nSave errors:");
                        for error in save_result.errors {
                            println!("\t{}: {}", error.uri, error.message);
                            if let Some(details) = error.details {
                                println!("\t\t{}", details);
                            }
                        }
                    }

                    let total_blocks =
                        save_result.created_blocks.len() + save_result.modified_blocks.len();

                    if total_blocks > 0 {
                        println!(
                            "Please wait {:.2} minutes for indexing to complete",
                            total_blocks as f64 * 1.5 / 60.0
                        );
                    }
                }
            }
            Commands::Transpile {
                dir,
                source_provisioner,
                target_provisioner,
            } => {
                if target_provisioner != TranspileTargetProvisionerType::EraserDSL {
                    return Err(
                        "Currently only EraserDSL is supported as a transpile target".into(),
                    );
                }
                if source_provisioner != ProvisionerType::Terraform {
                    return Err("Currently only terraform is supported as a source DSL".into());
                }

                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let base_dir = dir.unwrap_or_else(|| ".".into());

                let mut documents = Vec::new();

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
                                // Only allow terraform files when from is terraform
                                if e.file_type().is_file() {
                                    name.ends_with(".tf")
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

                    documents.push(Document {
                        content,
                        uri: document_uri,
                        provisioner: source_provisioner.clone(),
                    });
                }

                if documents.is_empty() {
                    return Err(format!(
                        "No {} files found to transpile",
                        source_provisioner
                    ));
                }

                let result = client
                    .transpile(documents, source_provisioner, target_provisioner)
                    .await?;
                println!(
                    "{}",
                    result
                        .result
                        .blocks
                        .into_iter()
                        .map(|b| b.code)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
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

                AgentCommands::run(agent_commands, config, false).await?;
            }
            Commands::Version => {
                println!(
                    "stakpak v{} (https://github.com/stakpak/cli)",
                    env!("CARGO_PKG_VERSION")
                );
            }
            Commands::Apply {
                flow_ref,
                dir,
                provisioner,
                // no_clone,
            } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;

                let flow_ref = get_flow_ref(&client, flow_ref).await?;
                let path_map = clone(&client, &flow_ref, dir.as_deref()).await?;

                if path_map.is_empty() {
                    return Err("No configurations found to apply".into());
                }

                let config_clone = config.clone();
                let client_clone = Client::new(&config_clone).map_err(|e| e.to_string())?;
                let flow_ref_clone = flow_ref.clone();
                let dir_clone = dir.clone();
                tokio::spawn(async move {
                    flow::sync(
                        &config_clone,
                        &client_clone,
                        &flow_ref_clone,
                        dir_clone.as_deref(),
                    )
                    .await
                });

                let agent_id = AgentID::KevinV1;

                let agent_input = match provisioner {
                    None => {
                        println!(
                            "Please specify a provisioner to apply with -p. Available provisioners:"
                        );
                        for provisioner in path_map.keys() {
                            println!("  {}", provisioner);
                        }
                        return Err("Must specify provisioner type to apply".to_string());
                    }
                    Some(provisioner) => {
                        let tasks = client
                            .get_agent_tasks(&provisioner, dir)
                            .await
                            .map_err(|e| e.to_string())?;

                        let task = tasks
                            .iter()
                            .find(|t| {
                                t.input.get_agent_id() == agent_id
                                    && t.provisioner == Some(provisioner.clone())
                            })
                            .ok_or("No matching task found")?;

                        task.input.clone()
                    }
                };

                let (agent_id, session, checkpoint) =
                    get_or_create_session(&client, agent_id, None, Some(agent_input.clone()))
                        .await?;

                let checkpoint_id = run_agent(
                    &config,
                    &client,
                    agent_id,
                    session,
                    checkpoint,
                    Some(agent_input),
                    true,
                    true,
                )
                .await?;

                // Write checkpoint ID to local file for resuming later
                std::fs::write(".stakpak_apply_checkpoint", checkpoint_id.to_string())
                    .map_err(|e| format!("Failed to write checkpoint file: {}", e))?;

                println!("[Saved checkpoint ID to .stakpak_apply_checkpoint]");
            }
        }
        Ok(())
    }
}
