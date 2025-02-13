use uuid::Uuid;

use crate::{
    client::{
        models::{
            AgentCheckpointListItem, AgentID, AgentInput, AgentSessionListItem, AgentStatus,
            RunAgentInput,
        },
        Client,
    },
    commands::agent::get_next_input,
    config::AppConfig,
    utils::output::setup_output_handler,
};

use super::{get_next_input_interactive, AgentOutputListener};

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
