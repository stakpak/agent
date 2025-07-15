use crate::config::AppConfig;
use clap::Subcommand;
use stakpak_api::{Client, ClientConfig};
use std::str::FromStr;
use uuid::Uuid;

pub mod run;

#[derive(Subcommand, PartialEq)]
pub enum AgentCommands {
    /// List agent sessions
    List,

    /// Get agent checkpoint details
    Get {
        /// Checkpoint ID to inspect
        checkpoint_id: String,
    },
}

impl AgentCommands {
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
            AgentCommands::List => {
                let client = Client::new(&ClientConfig {
                    api_key: config.api_key,
                    api_endpoint: config.api_endpoint,
                })
                .map_err(|e| e.to_string())?;
                let sessions = client.list_agent_sessions().await?;
                for session in sessions {
                    println!("Session ID: {}", session.id);
                    println!("Agent ID: {:?}", session.agent_id);
                    println!("Visibility: {:?}", session.visibility);
                    println!("Created: {}", session.created_at);
                    println!("Checkpoints:");
                    for checkpoint in session.checkpoints {
                        println!("  - ID: {}", checkpoint.id);
                        if let Some(parent) = checkpoint.parent {
                            println!("    Parent: {}", parent.id);
                        }
                        println!("    Status: {}", checkpoint.status);
                        println!("    Execution Depth: {}", checkpoint.execution_depth);
                        println!("    Created: {}", checkpoint.created_at);
                    }
                    println!();
                }
            }
            AgentCommands::Get { checkpoint_id } => {
                let client = Client::new(&ClientConfig {
                    api_key: config.api_key,
                    api_endpoint: config.api_endpoint,
                })
                .map_err(|e| e.to_string())?;
                let checkpoint_uuid = Uuid::from_str(&checkpoint_id).map_err(|e| e.to_string())?;
                let output = client.get_agent_checkpoint(checkpoint_uuid).await?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output).unwrap_or_default()
                );
            }
        }
        Ok(())
    }
}
