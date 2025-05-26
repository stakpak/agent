use std::vec;

use uuid::Uuid;

use crate::{
    client::{
        Client,
        models::{
            AgentCheckpointListItem, AgentID, AgentInput, AgentSessionListItem, AgentStatus,
            RunAgentInput,
        },
    },
    commands::agent::get_next_input,
    config::AppConfig,
    utils::output::setup_output_handler,
};

use super::get_next_input_interactive;

#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    config: &AppConfig,
    client: &Client,
    agent_id: AgentID,
    session: AgentSessionListItem,
    checkpoint: AgentCheckpointListItem,
    input: Option<AgentInput>,
    short_circuit_actions: bool,
    interactive: bool,
) -> Result<Uuid, String> {
    let print = setup_output_handler(config, session.id.to_string()).await?;
    let mut input = RunAgentInput {
        checkpoint_id: checkpoint.id,
        input: match input {
            Some(input) => input,
            None => AgentInput::new(&agent_id),
        },
    };

    match interactive {
        true => loop {
            print("[ ▄▀ Stakpaking... ]");
            let output = client.run_agent(&input).await?;
            print(&format!(
                "[Current Checkpoint {} (Agent Status: {})]",
                output.checkpoint.id, output.checkpoint.status
            ));

            input = get_next_input_interactive(
                client,
                &agent_id,
                &print,
                &output,
                short_circuit_actions,
            )
            .await?;

            match output.checkpoint.status {
                AgentStatus::Complete => {
                    print("[Mission Accomplished]");
                    break;
                }
                AgentStatus::Failed => {
                    print("[Mission Failed :'(]");
                    break;
                }
                _ => {}
            };
        },
        false => {
            println!(
                "\x1b[33m[NOTE: This agent is in non-interactive mode. Use the \x1b[1;32m-i\x1b[33m flag to enable interactive mode.]\x1b[0m"
            );

            if let Some(flow_ref) = &session.flow_ref {
                let url = format!("{}?session_id={}", flow_ref.to_url(), session.id);
                println!("\x1b[1;34m{}\x1b[0m", "━".repeat(80));
                println!("\x1b[1;36mContinue the agent session in your browser:\x1b[0m");
                println!(
                    "\x1b]8;;{}\x1b\\\x1b[1;32m{}\x1b[0m\x1b]8;;\x1b\\",
                    url, url
                );
                println!("\x1b[1;34m{}\x1b[0m", "━".repeat(80));
            }

            let mut processed_outputs: Vec<crate::client::models::RunAgentOutput> = vec![];
            let mut input = input.clone();

            loop {
                let mut session = client.get_agent_session(session.id).await?;
                session
                    .checkpoints
                    .sort_by(|a, b| b.created_at.cmp(&a.created_at));
                let checkpoint = session.checkpoints.first().unwrap();

                if !processed_outputs
                    .iter()
                    .any(|x| x.checkpoint.id == checkpoint.id)
                {
                    print("[ ▄▀ Stakpaking... ]");
                    let output = client.get_agent_checkpoint(checkpoint.id).await?;
                    let next_input = get_next_input(&agent_id, &print, &output).await?;

                    if next_input != input {
                        input = next_input;
                        client.run_agent(&input).await?;
                    }

                    processed_outputs.push(output);
                }

                match checkpoint.status {
                    AgentStatus::Complete => {
                        print("[Mission Accomplished]");
                        break;
                    }
                    AgentStatus::Failed => {
                        print("[Mission Failed :'(]");
                        break;
                    }
                    _ => {}
                };

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }

    Ok(input.checkpoint_id)
}
