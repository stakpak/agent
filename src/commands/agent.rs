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
        /// Add user prompt to stir the agent
        user_prompt: Option<String>,
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
                checkpoint_id,
            } => {
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
                        user_prompt,
                        action_queue: None,
                        action_history: None,
                        scratchpad: None,
                    },
                };

                loop {
                    println!("[Thinking...]");
                    let output = client.run_agent(&input).await?;
                    println!(
                        "[Current Checkpoint {} (Agent Status: {})]",
                        output.checkpoint.id, output.checkpoint.status
                    );

                    let next_input = match output.output {
                        AgentOutput::NorbertV1 {
                            message,
                            action_queue,
                            action_history,
                            scratchpad: _,
                            user_prompt: _,
                        } => {
                            if let Some(message) = message {
                                println!("\n{}", message);
                            }

                            match run_actions(action_queue).await {
                                Ok(updated_actions) => RunAgentInput {
                                    checkpoint_id: output.checkpoint.id,
                                    input: AgentInput::NorbertV1 {
                                        user_prompt: None,
                                        action_queue: Some(updated_actions),
                                        action_history: None,
                                        scratchpad: None,
                                    },
                                },
                                Err(e) if e == "re-prompt" => {
                                    println!("Please re-prompt the agent:");
                                    let mut user_prompt_input = String::new();
                                    std::io::stdin()
                                        .read_line(&mut user_prompt_input)
                                        .map_err(|e| e.to_string())?;

                                    let parent_checkpoint_id = match output.checkpoint.parent {
                                        Some(parent) => parent.id,
                                        None => {
                                            return Err(format!(
                                                "Checkpoint {} parent id not found!",
                                                output.checkpoint.id
                                            ))
                                        }
                                    };

                                    println!("\nRetrying from checkpoint {}", parent_checkpoint_id);

                                    let parent_run_data =
                                        client.get_agent_checkpoint(parent_checkpoint_id).await?;
                                    let AgentOutput::NorbertV1 {
                                        action_queue: parent_action_queue,
                                        ..
                                    } = parent_run_data.output;

                                    let updated_actions = parent_action_queue
                                        .into_iter()
                                        .map(|action| {
                                            match action_history
                                                .iter()
                                                .find(|a| a.get_id() == action.get_id())
                                            {
                                                Some(updated_action) => updated_action.clone(),
                                                None => action,
                                            }
                                        })
                                        .collect();

                                    RunAgentInput {
                                        checkpoint_id: parent_checkpoint_id,
                                        input: AgentInput::NorbertV1 {
                                            user_prompt: Some(user_prompt_input.trim().to_string()),
                                            action_queue: Some(updated_actions),
                                            action_history: None,
                                            scratchpad: None,
                                        },
                                    }
                                }
                                Err(e) => return Err(e),
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

                    input = next_input;
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

async fn run_actions(action_queue: Vec<Action>) -> Result<Vec<Action>, String> {
    let mut updated_actions = Vec::with_capacity(action_queue.len());
    for action in action_queue.into_iter().filter(|a| a.is_pending()) {
        updated_actions.push(action.run().await?);
    }
    Ok(updated_actions)
}

impl Action {
    pub async fn run(self) -> Result<Action, String> {
        match self {
            Action::AskUser { id, args, .. } => {
                println!(
                    "\n[Action Description: {}] (Ctrl+P & Enter to re-prompt the agent)",
                    args.description
                );
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
                println!(
                    "\n[Action Description: {}] (Ctrl+P & Enter to re-prompt the agent)",
                    args.description
                );
                println!("[Reasoning]");
                for line in args.reasoning.lines() {
                    println!("  {}", line);
                }
                println!("\n[WARNING] About to execute the following command:");
                println!(">{}", args.command);

                println!("Please confirm [yes/edit/skip] (skip):");
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
