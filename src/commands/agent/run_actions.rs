use crate::client::models::Action;

pub async fn run_actions(
    action_queue: Vec<Action>,
    short_circuit_actions: bool,
    print: &impl Fn(&str),
) -> Result<Vec<Action>, String> {
    let mut updated_actions = Vec::with_capacity(action_queue.len());
    for action in action_queue.into_iter().filter(|a| a.is_pending()) {
        let updated_action = action.run(print).await?;

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
