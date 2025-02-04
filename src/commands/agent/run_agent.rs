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

pub async fn run_agent(
    config: &AppConfig,
    client: &Client,
    agent_id: AgentID,
    checkpoint_id: Option<String>,
    input: Option<AgentInput>,
    short_circuit_actions: bool,
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

    loop {
        print("[ ▄▀ Stakpaking... ]");
        let output = client.run_agent(&input).await?;
        print(&format!(
            "[Current Checkpoint {} (Agent Status: {})]",
            output.checkpoint.id, output.checkpoint.status
        ));

        input = get_next_input(&agent_id, client, &print, &output, short_circuit_actions).await?;

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
    }

    Ok(input.checkpoint_id)
}
