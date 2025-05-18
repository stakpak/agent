use uuid::Uuid;

use crate::client::{
    models::{
        AgentCheckpointListItem, AgentID, AgentInput, AgentSessionListItem, AgentSessionVisibility,
    },
    Client,
};

pub async fn get_or_create_session(
    client: &Client,
    agent_id: AgentID,
    checkpoint_id: Option<String>,
    input: Option<AgentInput>,
) -> Result<(AgentID, AgentSessionListItem, AgentCheckpointListItem), String> {
    match checkpoint_id {
        Some(checkpoint_id) => {
            let checkpoint_uuid = Uuid::parse_str(&checkpoint_id).map_err(|_| {
                format!(
                    "Invalid checkpoint ID '{}' - must be a valid UUID",
                    checkpoint_id
                )
            })?;

            let output = client.get_agent_checkpoint(checkpoint_uuid).await?;

            Ok((
                output.output.get_agent_id(),
                output.session,
                output.checkpoint,
            ))
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

            Ok((agent_id, session.into(), checkpoint))
        }
    }
}
