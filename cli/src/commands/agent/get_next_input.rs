use crate::client::{
    Client, SimpleLLMMessage, SimpleLLMRole,
    models::{AgentID, AgentInput, AgentOutput, RunAgentInput, RunAgentOutput},
};

use super::{run_interactive_actions, run_remote_actions};

pub async fn get_next_input_interactive(
    client: &Client,
    agent_id: &AgentID,
    print: &impl Fn(&str),
    output: &RunAgentOutput,
    short_circuit_actions: bool,
) -> Result<RunAgentInput, String> {
    if let AgentOutput::StuartV1 { messages, .. } = &output.output {
        if let Some(last_system_msg) = messages
            .iter()
            .rev()
            .find(|m| m.role == SimpleLLMRole::Assistant)
        {
            print(format!("\n{}", last_system_msg.content).as_str());
        }
    }

    match &output.output {
        AgentOutput::NorbertV1 { message, .. }
        | AgentOutput::DaveV1 { message, .. }
        | AgentOutput::DaveV2 { message, .. }
        | AgentOutput::KevinV1 { message, .. } => {
            if let Some(message) = message {
                print(format!("\n{}", message).as_str());
            }
        }
        _ => {}
    }

    match &output.output {
        AgentOutput::NorbertV1 {
            action_queue,
            action_history,
            ..
        }
        | AgentOutput::DaveV1 {
            action_queue,
            action_history,
            ..
        }
        | AgentOutput::DaveV2 {
            action_queue,
            action_history,
            ..
        }
        | AgentOutput::KevinV1 {
            action_queue,
            action_history,
            ..
        }
        | AgentOutput::StuartV1 {
            action_queue,
            action_history,
            ..
        } => {
            let result =
                match run_interactive_actions(action_queue.to_owned(), short_circuit_actions).await
                {
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
                            AgentID::DaveV2 => AgentInput::DaveV2 {
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
                            AgentID::StuartV1 => AgentInput::StuartV1 {
                                messages: None,
                                action_queue: Some(updated_actions),
                                action_history: None,
                                scratchpad: Box::new(None),
                            },
                        },
                    },
                    Err(e) if e == "re-prompt" => {
                        print("Please re-prompt the agent:");
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
                                ));
                            }
                        };

                        print(
                            format!("\nRetrying from checkpoint {}", parent_checkpoint_id).as_str(),
                        );

                        let parent_run_data =
                            client.get_agent_checkpoint(parent_checkpoint_id).await?;

                        let parent_action_queue = match parent_run_data.output {
                            AgentOutput::NorbertV1 { action_queue, .. } => action_queue,
                            AgentOutput::DaveV1 { action_queue, .. } => action_queue,
                            AgentOutput::DaveV2 { action_queue, .. } => action_queue,
                            AgentOutput::KevinV1 { action_queue, .. } => action_queue,
                            AgentOutput::StuartV1 { action_queue, .. } => action_queue,
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
                                AgentID::DaveV2 => AgentInput::DaveV2 {
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
                                AgentID::StuartV1 => AgentInput::StuartV1 {
                                    messages: Some(vec![SimpleLLMMessage {
                                        role: SimpleLLMRole::User,
                                        content: user_prompt_input.trim().to_string(),
                                    }]),
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

pub async fn get_next_input(
    agent_id: &AgentID,
    print: &impl Fn(&str),
    output: &RunAgentOutput,
) -> Result<RunAgentInput, String> {
    if let AgentOutput::StuartV1 { messages, .. } = &output.output {
        if let Some(last_system_msg) = messages
            .iter()
            .rev()
            .find(|m| m.role == SimpleLLMRole::Assistant)
        {
            print(format!("\n{}", last_system_msg.content).as_str());
        }
    }

    match &output.output {
        AgentOutput::NorbertV1 { message, .. }
        | AgentOutput::DaveV1 { message, .. }
        | AgentOutput::DaveV2 { message, .. }
        | AgentOutput::KevinV1 { message, .. } => {
            if let Some(message) = message {
                print(format!("\n{}", message).as_str());
            }
        }
        _ => {}
    }

    match &output.output {
        AgentOutput::NorbertV1 { action_queue, .. }
        | AgentOutput::DaveV1 { action_queue, .. }
        | AgentOutput::DaveV2 { action_queue, .. }
        | AgentOutput::KevinV1 { action_queue, .. }
        | AgentOutput::StuartV1 { action_queue, .. } => {
            let result = match run_remote_actions(action_queue.to_owned(), print).await {
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
                        AgentID::DaveV2 => AgentInput::DaveV2 {
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
                        AgentID::StuartV1 => AgentInput::StuartV1 {
                            messages: None,
                            action_queue: Some(updated_actions),
                            action_history: None,
                            scratchpad: Box::new(None),
                        },
                    },
                },
                Err(e) => return Err(e),
            };

            Ok(result)
        }
    }
}
