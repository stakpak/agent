use clap::Subcommand;
use std::str::FromStr;
use tokio::process;
use tokio_process_stream::{Item, ProcessLineStream};
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{
    client::{
        models::{
            Action, ActionStatus, AgentID, AgentInput, AgentOutput, AgentSessionVisibility,
            AgentStatus, RunAgentInput, RunAgentOutput,
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

                let mut input = AgentInput::new(&agent_id);
                input.set_user_prompt(user_prompt);

                run_agent(
                    &client,
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

pub async fn run_agent(
    client: &Client,
    agent_id: AgentID,
    checkpoint_id: Option<String>,
    input: Option<AgentInput>,
    short_circuit_actions: bool,
) -> Result<Uuid, String> {
    let (agent_id, checkpoint) = match checkpoint_id {
        Some(checkpoint_id) => {
            let checkpoint_uuid = Uuid::parse_str(&checkpoint_id).map_err(|_| {
                format!(
                    "Invalid checkpoint ID '{}' - must be a valid UUID",
                    checkpoint_id
                )
            })?;

            let output = client.get_agent_checkpoint(checkpoint_uuid).await?;

            (output.output.get_agent_id(), output.checkpoint)
        }
        None => {
            let session = client
                .create_agent_session(
                    agent_id.clone(),
                    AgentSessionVisibility::Private,
                    input.clone(),
                )
                .await?;

            let checkpoint = session
                .checkpoints
                .first()
                .ok_or("No checkpoint found in new session")?
                .clone();

            (agent_id, checkpoint)
        }
    };

    let mut input = RunAgentInput {
        checkpoint_id: checkpoint.id,
        input: match input {
            Some(input) => input,
            None => AgentInput::new(&agent_id),
        },
    };

    loop {
        println!("[ ▄▀ Stakpaking... ]");
        let output = client.run_agent(&input).await?;
        println!(
            "[Current Checkpoint {} (Agent Status: {})]",
            output.checkpoint.id, output.checkpoint.status
        );

        input = get_next_input(&agent_id, client, &output, short_circuit_actions).await?;

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
    }

    Ok(input.checkpoint_id)
}

async fn run_actions(
    action_queue: Vec<Action>,
    short_circuit_actions: bool,
) -> Result<Vec<Action>, String> {
    let mut updated_actions = Vec::with_capacity(action_queue.len());
    for action in action_queue.into_iter().filter(|a| a.is_pending()) {
        let updated_action = action.run().await?;

        if short_circuit_actions {
            if let Action::RunCommand {
                exit_code: Some(code),
                ..
            } = &updated_action
            {
                if *code != 0 {
                    updated_actions.push(updated_action);
                    return Ok(updated_actions);
                }
            }
        }

        updated_actions.push(updated_action);
    }

    Ok(updated_actions)
}

async fn get_next_input(
    agent_id: &AgentID,
    client: &Client,
    output: &RunAgentOutput,
    short_circuit_actions: bool,
) -> Result<RunAgentInput, String> {
    match &output.output {
        AgentOutput::NorbertV1 {
            message,
            action_queue,
            action_history,
            ..
        }
        | AgentOutput::DaveV1 {
            message,
            action_queue,
            action_history,
            ..
        }
        | AgentOutput::KevinV1 {
            message,
            action_queue,
            action_history,
            ..
        } => {
            if let Some(message) = message {
                println!("\n{}", message);
            }

            let result = match run_actions(action_queue.to_owned(), short_circuit_actions).await {
                Ok(updated_actions) => RunAgentInput {
                    checkpoint_id: output.checkpoint.id,
                    input: match agent_id {
                        AgentID::NorbertV1 => AgentInput::NorbertV1 {
                            user_prompt: None,
                            action_queue: Some(updated_actions),
                            action_history: None,
                            scratchpad: Box::new(None),
                        },
                        AgentID::DaveV1 => AgentInput::DaveV1 {
                            user_prompt: None,
                            action_queue: Some(updated_actions),
                            action_history: None,
                            scratchpad: Box::new(None),
                        },
                        AgentID::KevinV1 => AgentInput::KevinV1 {
                            user_prompt: None,
                            action_queue: Some(updated_actions),
                            action_history: None,
                            scratchpad: Box::new(None),
                        },
                    },
                },
                Err(e) if e == "re-prompt" => {
                    println!("Please re-prompt the agent:");
                    let mut user_prompt_input = String::new();
                    std::io::stdin()
                        .read_line(&mut user_prompt_input)
                        .map_err(|e| e.to_string())?;

                    let parent_checkpoint_id = match &output.checkpoint.parent {
                        Some(parent) => parent.id,
                        None => {
                            return Err(format!(
                                "Checkpoint {} parent id not found!",
                                output.checkpoint.id
                            ))
                        }
                    };

                    println!("\nRetrying from checkpoint {}", parent_checkpoint_id);

                    let parent_run_data = client.get_agent_checkpoint(parent_checkpoint_id).await?;

                    let parent_action_queue = match parent_run_data.output {
                        AgentOutput::NorbertV1 { action_queue, .. } => action_queue,
                        AgentOutput::DaveV1 { action_queue, .. } => action_queue,
                        AgentOutput::KevinV1 { action_queue, .. } => action_queue,
                    };

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
                        input: match agent_id {
                            AgentID::NorbertV1 => AgentInput::NorbertV1 {
                                user_prompt: Some(user_prompt_input.trim().to_string()),
                                action_queue: Some(updated_actions),
                                action_history: None,
                                scratchpad: Box::new(None),
                            },
                            AgentID::DaveV1 => AgentInput::DaveV1 {
                                user_prompt: Some(user_prompt_input.trim().to_string()),
                                action_queue: Some(updated_actions),
                                action_history: None,
                                scratchpad: Box::new(None),
                            },
                            AgentID::KevinV1 => AgentInput::KevinV1 {
                                user_prompt: Some(user_prompt_input.trim().to_string()),
                                action_queue: Some(updated_actions),
                                action_history: None,
                                scratchpad: Box::new(None),
                            },
                        },
                    }
                }
                Err(e) => return Err(e),
            };

            Ok(result)
        }
    }
}

impl Action {
    pub async fn run(self) -> Result<Action, String> {
        match self {
            Action::AskUser { id, args, .. } => {
                println!(
                    "\n[Action] (Ctrl+P & Enter to re-prompt the agent)\n  {}",
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
                    "\n[Action] (Ctrl+P & Enter to re-prompt the agent)\n  {}",
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

                let mut cmd = process::Command::new("sh");
                cmd.arg("-c").arg(&command);

                let mut output_lines = Vec::new();
                let mut process_stream = ProcessLineStream::try_from(cmd)
                    .map_err(|e| format!("Failed to create process stream: {}", e))?;
                let mut exit_code = -1;

                while let Some(item) = process_stream.next().await {
                    match item {
                        Item::Stdout(line) | Item::Stderr(line) => {
                            println!("{}", line);
                            output_lines.push(line.to_string());
                        }
                        Item::Done(exit_status) => {
                            exit_code = match exit_status {
                                Ok(status) => status.code().unwrap_or(-1),
                                Err(e) => {
                                    println!("Error: {}", e);
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
