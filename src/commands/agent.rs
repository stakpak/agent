use std::str::FromStr;

use clap::Subcommand;
use tokio::process;
use uuid::Uuid;

use crate::{
    client::{
        models::{
            Action, ActionStatus, AgentID, AgentInput, AgentOutput, AgentSessionVisibility,
            AgentStatus, RunAgentInput,
        },
        Client,
    },
    config::AppConfig,
};

#[derive(Subcommand)]
pub enum AgentCommands {
    /// List agent sessions
    List,

    /// Get agent checkpoint details
    Get {
        /// Checkpoint ID to inspect
        checkpoint_id: String,
    },

    /// Run the Stakpak Agent
    Run {
        /// Resume Agent session from checkpoint ID
        #[arg(long, short)]
        checkpoint_id: Option<String>,
    },
}

impl AgentCommands {
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        match self {
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
                        println!("    Status: {}", checkpoint.status);
                        println!("    Execution Depth: {}", checkpoint.execution_depth);
                        println!("    Created: {}", checkpoint.created_at);
                    }
                    println!();
                }
            }
            AgentCommands::Run { checkpoint_id } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;
                let checkpoint_id = match checkpoint_id {
                    Some(checkpoint_id) => Uuid::parse_str(&checkpoint_id).map_err(|_| {
                        format!(
                            "Invalid checkpoint ID '{}' - must be a valid UUID",
                            checkpoint_id
                        )
                    })?,
                    None => {
                        let session = client
                            .create_agent_session(
                                AgentID::NorbertV1,
                                AgentSessionVisibility::Private,
                            )
                            .await?;

                        session
                            .checkpoints
                            .first()
                            .ok_or("No checkpoint found in new session")?
                            .id
                    }
                };

                let mut input = RunAgentInput {
                    checkpoint_id,
                    input: AgentInput::NorbertV1 {
                        user_prompt: None,
                        action_queue: None,
                        scratchpad: None,
                    },
                };

                loop {
                    println!("[Thinking...]");
                    let output = client.run_agent(input).await?;
                    println!(
                        "[Current Checkpoint {} (Agent Status: {})]",
                        output.checkpoint.id, output.checkpoint.status
                    );

                    let next_agent_input = match output.output {
                        AgentOutput::NorbertV1 {
                            message,
                            action_queue,
                            action_history: _,
                            scratchpad: _,
                            user_prompt: _,
                        } => {
                            if let Some(message) = message {
                                println!("\n{}", message);
                            }

                            let mut updated_actions = Vec::with_capacity(action_queue.len());
                            for action in action_queue.into_iter().filter(|a| a.is_pending()) {
                                updated_actions.push(action.run().await?);
                            }

                            AgentInput::NorbertV1 {
                                user_prompt: None,
                                action_queue: Some(updated_actions),
                                scratchpad: None,
                            }
                        }
                    };

                    match output.checkpoint.status {
                        AgentStatus::Complete => {
                            println!("[Mission Accomplished]");
                            break;
                        }
                        AgentStatus::Failed => {
                            println!("[Mission Failed :'(]");
                            break;
                        }
                        _ => {}
                    };

                    input = RunAgentInput {
                        checkpoint_id: output.checkpoint.id,
                        input: next_agent_input,
                    };
                }
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
    pub fn get_status(&self) -> &ActionStatus {
        match self {
            Action::AskUser { status, .. } => status,
            Action::RunCommand { status, .. } => status,
        }
    }
    pub fn is_pending(&self) -> bool {
        match self.get_status() {
            ActionStatus::PendingHumanApproval => true,
            ActionStatus::Pending => true,
            ActionStatus::Succeeded => false,
            ActionStatus::Failed => false,
            ActionStatus::Aborted => false,
        }
    }
    pub async fn run(self) -> Result<Action, String> {
        match self {
            Action::AskUser { id, args, .. } => {
                println!("\n[Action Description: {}]", args.description);
                println!("[Reasoning]");
                for line in args.reasoning.lines() {
                    println!("  {}", line);
                }

                let total_questions = args.questions.len();
                let mut answers = Vec::new();

                for (i, question) in args.questions.iter().enumerate() {
                    println!("\n[Question {}/{}] {}", i + 1, total_questions, question);
                    println!("(Press Enter twice to finish this answer)");

                    let mut lines = Vec::new();
                    loop {
                        let mut line = String::new();
                        std::io::stdin()
                            .read_line(&mut line)
                            .map_err(|e| format!("Failed to read input: {}", e))?;

                        let line = line.trim_end();
                        if line.is_empty() {
                            break;
                        }
                        lines.push(line.to_string());
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
                println!("\n[Action Description: {}]", args.description);
                println!("[Reasoning]");
                for line in args.reasoning.lines() {
                    println!("  {}", line);
                }
                println!("\n[WARNING] About to execute the following command:");
                println!(">{}", args.command);

                println!("Please confirm [yes/edit/skip] (skip):");
                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| format!("Failed to read input: {}", e))?;
                let confirmation = input.trim().to_lowercase();

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
                    println!("> ");
                    let mut edited_cmd = String::new();
                    std::io::stdin()
                        .read_line(&mut edited_cmd)
                        .map_err(|e| format!("Failed to read input: {}", e))?;
                    edited_cmd.trim().to_string()
                } else {
                    args.command.clone()
                };

                let output = match process::Command::new("sh")
                    .arg("-c")
                    .arg(&command)
                    .output()
                    .await
                    .map_err(|e| format!("Failed to execute command: {}", e))
                {
                    Ok(output) => {
                        let exit_code = output.status.code().unwrap_or(1);
                        if exit_code == 0 {
                            let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();

                            // Truncate long output
                            const MAX_OUTPUT_LENGTH: usize = 4000;
                            if stdout.len() > MAX_OUTPUT_LENGTH {
                                let offset = MAX_OUTPUT_LENGTH / 2;
                                stdout = format!(
                                    "{}\n...truncated...\n{}",
                                    &stdout[..offset],
                                    &stdout[stdout.len() - offset..]
                                );
                            }

                            println!("{}", stdout);

                            Ok(Action::RunCommand {
                                id,
                                status: ActionStatus::Succeeded,
                                args,
                                exit_code: Some(exit_code),
                                output: Some(stdout),
                            })
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                            println!("{}", stderr);

                            Ok(Action::RunCommand {
                                id,
                                status: ActionStatus::Failed,
                                args,
                                exit_code: Some(exit_code),
                                output: Some(stderr),
                            })
                        }
                    }
                    Err(e) => Err(e),
                }?;

                Ok(output)
            }
        }
    }
}
