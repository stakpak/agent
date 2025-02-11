use uuid::Uuid;

use crate::{
    client::{
        models::{AgentID, AgentInput, AgentSessionVisibility, AgentStatus, RunAgentInput},
        Client,
    },
    commands::agent::get_next_input,
    config::AppConfig,
    utils::output::setup_output_handler,
};

use super::{get_next_input_interactive, AgentOutputListener};

pub async fn run_agent(
    config: &AppConfig,
    client: &Client,
    agent_id: AgentID,
    checkpoint_id: Option<String>,
    input: Option<AgentInput>,
    short_circuit_actions: bool,
    interactive: bool,
) -> Result<Uuid, String> {
    let (agent_id, session, checkpoint) = match checkpoint_id {
        Some(checkpoint_id) => {
            let checkpoint_uuid = Uuid::parse_str(&checkpoint_id).map_err(|_| {
                format!(
                    "Invalid checkpoint ID '{}' - must be a valid UUID",
                    checkpoint_id
                )
            })?;

            let output = client.get_agent_checkpoint(checkpoint_uuid).await?;

            (
                output.output.get_agent_id(),
                output.session,
                output.checkpoint,
            )
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

            (agent_id, session.into(), checkpoint)
        }
    };

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
            print("[ ▄▀ Stakpaking... ]");
            let output = client.run_agent(&input).await?;
            print(&format!(
                "[Current Checkpoint {} (Agent Status: {})]",
                output.checkpoint.id, output.checkpoint.status
            ));
            let mut checkpoint_id = output.checkpoint.id;
            let listener = AgentOutputListener::new(config, output.session.id.to_string(), output);
            listener.start().await?;

            // Execute the sequence once before the loop
            let current_state = listener.get_current_state().await;
            input = get_next_input(&agent_id, &print, &current_state).await?;
            client.run_agent(&input).await?;

            loop {
                let current_state = listener.get_current_state().await;
                if checkpoint_id != current_state.checkpoint.id {
                    print("[ ▄▀ Stakpaking... ]");
                    checkpoint_id = current_state.checkpoint.id;
                    input = get_next_input(&agent_id, &print, &current_state).await?;
                    client.run_agent(&input).await?;
                }

                match current_state.checkpoint.status {
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
            }
        }
    }

    Ok(input.checkpoint_id)
}
