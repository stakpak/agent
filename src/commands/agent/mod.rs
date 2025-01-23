use clap::Subcommand;
use std::{str::FromStr, sync::Arc};
use tokio::{process, sync::Mutex};
use tokio_process_stream::{Item, ProcessLineStream};
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{
    client::{
        models::{Action, ActionStatus, AgentID, AgentInput},
        Client,
    },
    config::AppConfig,
    utils::socket::SocketClient,
};

mod get_next_input;
pub use get_next_input::*;

mod run_actions;
pub use run_actions::*;

mod run_agent;
pub use run_agent::*;

#[derive(Subcommand)]
pub enum AgentCommands {
    /// List agent sessions
    List,

    /// Get agent checkpoint details
    Get {
        /// Checkpoint ID to inspect
        checkpoint_id: String,
    },

    /// List available agents and what they do
    Agents,

    /// Run the Stakpak Agent
    Run {
        /// Add user prompt to stir the agent
        user_prompt: Option<String>,
        /// Resume Agent session from checkpoint ID
        #[arg(long, short)]
        checkpoint_id: Option<String>,
        /// Agent ID to use (norbert:v1, dave:v1)
        #[arg(long, short)]
        agent_id: AgentID,
    },
}

impl AgentCommands {
    pub async fn run(self, config: AppConfig, short_circuit_actions: bool) -> Result<(), String> {
        match self {
            AgentCommands::Agents => {
                println!();
                println!("norbert:v1");
                println!(
                    "\tAn agent that deploys production-ready applications using virtual machines"
                );
                println!("\tand managed databases. Handles configuration of systemd services,");
                println!("\tTLS certificates, DNS records, and secrets management.");
                println!();
                println!();
                println!("dave:v1");
                println!("\tAn agent that containerizes applications using Docker, creating");
                println!("\tproduction-ready container images and configurations.");
                println!();
            }
            AgentCommands::List => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
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
            AgentCommands::Run {
                user_prompt,
                agent_id,
                checkpoint_id,
            } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let socket_client = Arc::new(Mutex::new(
                    SocketClient::connect(&config)
                        .await
                        .map_err(|e| e.to_string())?,
                ));

                let mut input = AgentInput::new(&agent_id);
                input.set_user_prompt(user_prompt);

                run_agent(
                    &client,
                    socket_client,
                    agent_id,
                    checkpoint_id,
                    Some(input),
                    short_circuit_actions,
                )
                .await?;
            }
            AgentCommands::Get { checkpoint_id } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let checkpoint_uuid = Uuid::from_str(&checkpoint_id).map_err(|e| e.to_string())?;
                let output = client.get_agent_checkpoint(checkpoint_uuid).await?;
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            }
        }
        Ok(())
    }
}

impl Action {
    pub async fn run(self, print: &impl Fn(&str)) -> Result<Action, String> {
        match self {
            Action::AskUser { id, args, .. } => {
                print(
                    format!(
                        "\n[Action] (Ctrl+P & Enter to re-prompt the agent)\n  {}",
                        args.description,
                    )
                    .as_str(),
                );
                print("[Reasoning]");
                for line in args.reasoning.lines() {
                    print(format!("  {}", line).as_str());
                }

                let total_questions = args.questions.len();
                let mut answers = Vec::new();

                for (i, question) in args.questions.iter().enumerate() {
                    print(
                        format!("\n[Question {}/{}] {}", i + 1, total_questions, question).as_str(),
                    );
                    print("(Press Enter twice to finish this answer)");

                    let mut lines = Vec::new();
                    loop {
                        let mut line = String::new();
                        match std::io::stdin().read_line(&mut line) {
                            Ok(_) => {
                                let line = line.trim_end();
                                if line.is_empty() {
                                    break;
                                }
                                if line == "\x10" {
                                    // Ctrl+P
                                    return Err("re-prompt".to_string());
                                }
                                print(line);
                                lines.push(line.to_string());
                            }
                            Err(e) => return Err(format!("Failed to read input: {}", e)),
                        }
                    }
                    answers.push(lines.join("\n"));
                }

                Ok(Action::AskUser {
                    id,
                    status: ActionStatus::Succeeded,
                    args,
                    answers,
                })
            }
            Action::RunCommand { id, args, .. } => {
                print(
                    format!(
                        "\n[Action] (Ctrl+P & Enter to re-prompt the agent)\n  {}",
                        args.description,
                    )
                    .as_str(),
                );
                print("[Reasoning]");
                for line in args.reasoning.lines() {
                    print(format!("  {}", line).as_str());
                }
                print("\n[WARNING] About to execute the following command:");
                print(format!(">{}", args.command).as_str());

                print("Please confirm [yes/edit/skip] (skip):");
                let mut input = String::new();
                match std::io::stdin().read_line(&mut input) {
                    Ok(_) => {
                        if input.trim() == "\x10" {
                            // Ctrl+P
                            return Err("re-prompt".to_string());
                        }
                    }
                    Err(e) => return Err(format!("Failed to read input: {}", e)),
                }
                let confirmation = input.trim().to_lowercase();
                print(confirmation.as_str());

                if confirmation == "skip" {
                    return Ok(Action::RunCommand {
                        id,
                        status: ActionStatus::Aborted,
                        args,
                        exit_code: None,
                        output: Some("Command execution skipped by user".to_string()),
                    });
                }

                let command = if confirmation == "edit" {
                    print("> ");
                    let mut edited_cmd = String::new();
                    match std::io::stdin().read_line(&mut edited_cmd) {
                        Ok(_) => {
                            if edited_cmd.trim() == "\x10" {
                                // Ctrl+P
                                return Err("re-prompt".to_string());
                            }
                            edited_cmd.trim().to_string()
                        }
                        Err(e) => return Err(format!("Failed to read input: {}", e)),
                    }
                } else {
                    args.command.clone()
                };

                let mut cmd = process::Command::new("sh");
                cmd.arg("-c").arg(&command);

                let mut output_lines = Vec::new();
                let mut process_stream = ProcessLineStream::try_from(cmd)
                    .map_err(|e| format!("Failed to create process stream: {}", e))?;
                let mut exit_code = -1;

                while let Some(item) = process_stream.next().await {
                    match item {
                        Item::Stdout(line) | Item::Stderr(line) => {
                            print(line.as_str());
                            output_lines.push(line.to_string());
                        }
                        Item::Done(exit_status) => {
                            exit_code = match exit_status {
                                Ok(status) => status.code().unwrap_or(-1),
                                Err(e) => {
                                    print(format!("Error: {}", e).as_str());
                                    -1
                                }
                            };
                        }
                    }
                }

                let mut output = output_lines.join("\n");

                const MAX_OUTPUT_LENGTH: usize = 4000;
                // Truncate long output
                if output.len() > MAX_OUTPUT_LENGTH {
                    let offset = MAX_OUTPUT_LENGTH / 2;
                    output = format!(
                        "{}\n...truncated...\n{}",
                        &output[..offset],
                        &output[output.len() - offset..]
                    );
                }

                let status = if exit_code == 0 {
                    ActionStatus::Succeeded
                } else {
                    ActionStatus::Failed
                };

                Ok(Action::RunCommand {
                    id,
                    status,
                    args,
                    exit_code: Some(exit_code),
                    output: Some(output),
                })
            }
        }
    }
}
