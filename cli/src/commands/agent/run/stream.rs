use crate::commands::agent::run::tui::send_input_event;
use futures_util::{Stream, StreamExt};
use stakpak_shared::models::integrations::openai::{
    ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage,
    FinishReason, FunctionCall, FunctionCallDelta, MessageContent, Role, ToolCall, Usage,
};
use stakpak_tui::InputEvent;
use uuid::Uuid;

pub async fn process_responses_stream(
    stream: impl Stream<Item = Result<ChatCompletionStreamResponse, String>>,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<ChatCompletionResponse, String> {
    let mut stream = Box::pin(stream);

    let mut chat_completion_response = ChatCompletionResponse {
        id: "".to_string(),
        object: "".to_string(),
        created: 0,
        model: "".to_string(),
        choices: vec![],
        usage: Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
        system_fingerprint: None,
    };

    let mut chat_message = ChatMessage {
        role: Role::Assistant,
        content: None,
        name: None,
        tool_calls: None,
        tool_call_id: None,
    };
    let message_id = Uuid::new_v4();

    while let Some(response) = stream.next().await {
        send_input_event(input_tx, InputEvent::Loading(true)).await?;
        if let Ok(response) = response {
            let delta = &response.choices[0].delta;

            chat_completion_response = ChatCompletionResponse {
                id: response.id.clone(),
                object: response.object.clone(),
                created: response.created,
                model: response.model.clone(),
                choices: vec![],
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                system_fingerprint: None,
            };

            if let Some(content) = &delta.content {
                chat_message.content = Some(MessageContent::String(match chat_message.content {
                    Some(MessageContent::String(old_content)) => old_content + content,
                    _ => content.clone(),
                }));

                send_input_event(
                    input_tx,
                    InputEvent::StreamAssistantMessage(message_id, content.clone()),
                )
                .await?;
            }

            if let Some(tool_calls) = &delta.tool_calls {
                for delta_tool_call in tool_calls {
                    if chat_message.tool_calls.is_none() {
                        chat_message.tool_calls = Some(vec![]);
                    }

                    let tool_calls_vec = chat_message.tool_calls.as_mut();
                    if let Some(tool_calls_vec) = tool_calls_vec {
                        match tool_calls_vec.get_mut(delta_tool_call.index) {
                            Some(tool_call) => {
                                let delta_func = delta_tool_call.function.as_ref().unwrap_or(
                                    &FunctionCallDelta {
                                        name: None,
                                        arguments: None,
                                    },
                                );
                                tool_call.function.arguments = tool_call.function.arguments.clone()
                                    + delta_func.arguments.as_deref().unwrap_or("");
                            }
                            None => {
                                // push empty tool calls until the index is reached
                                tool_calls_vec.extend(
                                    (tool_calls_vec.len()..delta_tool_call.index).map(|_| {
                                        ToolCall {
                                            id: "".to_string(),
                                            r#type: "function".to_string(),
                                            function: FunctionCall {
                                                name: "".to_string(),
                                                arguments: "".to_string(),
                                            },
                                        }
                                    }),
                                );

                                tool_calls_vec.push(ToolCall {
                                    id: delta_tool_call.id.clone().unwrap_or_default(),
                                    r#type: "function".to_string(),
                                    function: FunctionCall {
                                        name: delta_tool_call
                                            .function
                                            .as_ref()
                                            .unwrap_or(&FunctionCallDelta {
                                                name: None,
                                                arguments: None,
                                            })
                                            .name
                                            .as_deref()
                                            .unwrap_or("")
                                            .to_string(),
                                        arguments: "".to_string(),
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // filter out empty tool calls
    chat_message.tool_calls = Some(
        chat_message
            .tool_calls
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter(|tool_call| !tool_call.id.is_empty())
            .cloned()
            .collect::<Vec<ToolCall>>(),
    );

    chat_completion_response.choices.push(ChatCompletionChoice {
        index: 0,
        message: chat_message.clone(),
        finish_reason: FinishReason::Stop,
        logprobs: None,
    });

    Ok(chat_completion_response)
}
