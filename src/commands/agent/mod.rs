use clap::Subcommand;
use futures_util::future::BoxFuture;
use regex::Regex;
use rust_socketio::{
    asynchronous::{Client as SocketClient, ClientBuilder},
    Payload,
};
use serde_json::{json, Value};
use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{process, sync::Mutex, time::sleep};
use tokio_process_stream::{Item, ProcessLineStream};
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{
    client::{
        models::{Action, ActionStatus, AgentID, AgentInput, RunAgentOutput},
        Client,
    },
    config::AppConfig,
};

mod get_next_input;
pub use get_next_input::*;

mod get_or_create_session;
pub use get_or_create_session::*;

mod run_actions;
pub use run_actions::*;

mod run_agent;
pub use run_agent::*;

use super::flow;

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
        #[arg(long, short, required_unless_present = "checkpoint_id")]
        agent_id: Option<AgentID>,
        /// Run in interactive mode
        #[arg(long, short, default_value_t = false)]
        interactive: bool,
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
                interactive,
            } => {
                let client = Client::new(&config).map_err(|e| e.to_string())?;

                let agent_id = match (checkpoint_id.clone(), agent_id) {
                    (Some(checkpoint_id), _) => {
                        let checkpoint_id =
                            Uuid::parse_str(&checkpoint_id).map_err(|e| e.to_string())?;

                        let checkpoint = client.get_agent_checkpoint(checkpoint_id).await?;
                        checkpoint.session.agent_id
                    }
                    (_, Some(agent_id)) => agent_id,
                    _ => return Err("Must provide either agent_id or checkpoint_id".into()),
                };

                let mut input = AgentInput::new(&agent_id);

                input.set_user_prompt(user_prompt);

                let (agent_id, session, checkpoint) =
                    get_or_create_session(&client, agent_id, checkpoint_id, Some(input.clone()))
                        .await?;

                if let Some(flow_ref) = &session.flow_ref {
                    let config_clone = config.clone();
                    let client_clone = Client::new(&config_clone).map_err(|e| e.to_string())?;
                    let flow_ref = flow_ref.clone();
                    tokio::spawn(async move {
                        flow::sync(&config_clone, &client_clone, &flow_ref, None).await
                    });
                }

                run_agent(
                    &config,
                    &client,
                    agent_id,
                    session,
                    checkpoint,
                    Some(input),
                    short_circuit_actions,
                    interactive,
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
    pub async fn run_interactive(self, print: &impl Fn(&str)) -> Result<Action, String> {
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

                let mut command = args.command.clone();

                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_err() {
                    return Err("Failed to read input".to_string());
                }

                // Check for Ctrl+P
                if input.trim() == "\x10" {
                    return Err("re-prompt".to_string());
                }

                let confirmation = input.trim().to_lowercase();

                match confirmation.as_str() {
                    "edit" => {
                        print("> ");
                        let mut edited_cmd = String::new();

                        if std::io::stdin().read_line(&mut edited_cmd).is_err() {
                            return Err("Failed to read input".to_string());
                        }

                        // Check for Ctrl+P in edit mode
                        if edited_cmd.trim() == "\x10" {
                            return Err("re-prompt".to_string());
                        }

                        command = edited_cmd.trim().to_string();
                    }
                    // Added to not drop the command value
                    "yes" => {}
                    _ => {
                        return Ok(Action::RunCommand {
                            id,
                            status: ActionStatus::Aborted,
                            args,
                            exit_code: None,
                            output: Some("Command execution skipped by user".to_string()),
                        })
                    }
                }

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
    pub async fn run(self, print: &impl Fn(&str)) -> Result<Action, String> {
        match self.clone() {
            Action::RunCommand {
                id, args, status, ..
            } => {
                if status == ActionStatus::PendingHumanApproval {
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

                    return Ok(self);
                }

                let mut cmd = process::Command::new("sh");
                cmd.arg("-c").arg(&args.command);

                let mut output_lines = Vec::new();
                let mut process_stream = ProcessLineStream::try_from(cmd)
                    .map_err(|e| format!("Failed to create process stream: {}", e))?;
                let mut exit_code = -1;

                let regex = Regex::new(r"\x1B\[[0-9;]*[mK]").unwrap();
                while let Some(item) = process_stream.next().await {
                    match item {
                        Item::Stdout(line) | Item::Stderr(line) => {
                            let clean_line = regex.replace_all(line.as_str(), "").to_string();
                            print(&clean_line);
                            output_lines.push(clean_line.to_string());
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
            _ => Ok(self),
        }
    }
}

struct AgentOutputListener<'a> {
    config: &'a AppConfig,
    session_id: String,
    output: Arc<Mutex<RunAgentOutput>>,
}

impl<'a> AgentOutputListener<'a> {
    fn new(config: &'a AppConfig, session_id: String, initial_state: RunAgentOutput) -> Self {
        let output_state = Arc::new(Mutex::new(initial_state));
        Self {
            config,
            session_id,
            output: output_state.clone(),
        }
    }

    async fn listener<
        T: Fn(Payload, SocketClient) -> BoxFuture<'static, ()> + 'static + Send + Sync,
    >(
        &self,
        event: String,
        callback: T,
    ) -> Result<(), String> {
        let socket_client = match ClientBuilder::new(self.config.api_endpoint.clone())
            .namespace("/v1/agents/sessions")
            .reconnect(true)
            .reconnect_delay(1000, 5000)
            .reconnect_on_disconnect(true)
            .opening_header(
                String::from("Authorization"),
                format!("Bearer {}", self.config.api_key.clone().unwrap_or_default()),
            )
            .on(event, callback)
            .connect()
            .await
        {
            Ok(client) => Arc::new(client),
            Err(e) => {
                return Err(format!("Failed to connect to server: {}", e));
            }
        };

        let subscription_complete = Arc::new(AtomicBool::new(false));

        for retry in 0.. {
            sleep(Duration::from_millis(200 * (retry + 1))).await;

            let subscription_complete_clone = Arc::clone(&subscription_complete);
            let ack_callback =
                move |_message: Payload, _socket: SocketClient| -> BoxFuture<'static, ()> {
                    let subscription_complete_clone = Arc::clone(&subscription_complete_clone);
                    Box::pin(async move {
                        subscription_complete_clone.store(true, Ordering::SeqCst);
                    })
                };

            if let Err(e) = socket_client
                .emit_with_ack(
                    "subscribe",
                    json!({ "session_id": self.session_id }),
                    Duration::from_secs(2),
                    ack_callback,
                )
                .await
            {
                if retry >= 9 {
                    return Err(format!("Failed to subscribe to session: {}", e));
                }
            }

            if subscription_complete.load(Ordering::SeqCst) {
                break;
            }

            if retry >= 5 {
                return Err("Failed to subscribe to session: Timed out".to_string());
            }
        }

        Ok(())
    }

    pub async fn start(&self) -> Result<(), String> {
        self.listen_for_status_updates().await?;
        Ok(())
    }

    async fn listen_for_status_updates(&self) -> Result<(), String> {
        let output_clone = Arc::clone(&self.output);
        self.listener(
            "status".to_string(),
            move |msg: Payload, _client: SocketClient| -> BoxFuture<'static, ()> {
                let output = output_clone.clone();
                Box::pin(async move {
                    if let Payload::Text(text) = msg {
                        if let Ok(status) = Self::parse_agent_output(text.first().unwrap()) {
                            let mut state = output.lock().await;
                            *state = status;
                        }
                    }
                })
            },
        )
        .await
        .map_err(|e| format!("Failed to listen for status updates: {}", e))
    }

    fn parse_agent_output(value: &Value) -> Result<RunAgentOutput, String> {
        serde_json::from_value(value.clone())
            .map_err(|e| format!("Failed to deserialize response: {}", e))
    }

    // New method to get the current action state
    pub async fn get_current_state(&self) -> RunAgentOutput {
        self.output.lock().await.clone()
    }
}
