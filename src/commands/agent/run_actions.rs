use crate::{client::models::Action, config::AppConfig};

pub async fn run_actions(
    config: &AppConfig,
    session_id: String,
    action_queue: Vec<Action>,
    print: &impl Fn(&str),
    short_circuit_actions: bool,
    interactive: bool,
) -> Result<Vec<Action>, String> {
    let mut updated_actions = Vec::with_capacity(action_queue.len());
    for action in action_queue.into_iter().filter(|a| a.is_pending()) {
        let updated_action = action
            .run(config, session_id.clone(), print, interactive)
            .await?;

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
