use crate::commands::agent::run::tui::send_input_event;
use futures_util::{Stream, StreamExt};
use stakpak_api::models::ApiStreamError;
use stakpak_shared::models::{
    integrations::openai::{
        ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage,
        FinishReason, FunctionCall, FunctionCallDelta, MessageContent, Role, ToolCall,
    },
    llm::{LLMModel, LLMTokenUsage},
};
use stakpak_tui::{InputEvent, LoadingOperation};
use uuid::Uuid;

pub async fn process_responses_stream(
    stream: impl Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>>,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<ChatCompletionResponse, ApiStreamError> {
    let mut stream = Box::pin(stream);

    let mut chat_completion_response = ChatCompletionResponse {
        id: "".to_string(),
        object: "".to_string(),
        created: 0,
        model: "".to_string(),
        choices: vec![],
        usage: LLMTokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            prompt_tokens_details: None,
        },
        system_fingerprint: None,
        metadata: None,
    };

    let mut llm_model: Option<LLMModel> = None;

    let mut chat_message = ChatMessage {
        role: Role::Assistant,
        content: None,
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
    };
    let message_id = Uuid::new_v4();

    // Start stream processing loading at the beginning
    send_input_event(
        input_tx,
        InputEvent::StartLoadingOperation(LoadingOperation::StreamProcessing),
    )
    .await?;

    while let Some(response) = stream.next().await {
        match &response {
            Ok(response) => {
                // Handle usage first - it can come in any event, including those with no content
                if let Some(usage) = &response.usage {
                    chat_completion_response.usage = usage.clone();

                    // Send usage to TUI for display immediately when we receive it
                    send_input_event(input_tx, InputEvent::StreamUsage(usage.clone())).await?;
                }

                let delta = &response.choices[0].delta;
                if !response.model.is_empty() {
                    let current_model: LLMModel = response.model.clone().into();
                    match &llm_model {
                        Some(model) => {
                            if *model != current_model {
                                llm_model = Some(current_model.clone());
                                send_input_event(input_tx, InputEvent::StreamModel(current_model))
                                    .await?;
                            }
                        }
                        None => {
                            llm_model = Some(current_model.clone());
                            send_input_event(input_tx, InputEvent::StreamModel(current_model))
                                .await?;
                        }
                    }
                }

                chat_completion_response = ChatCompletionResponse {
                    id: response.id.clone(),
                    object: response.object.clone(),
                    created: response.created,
                    model: llm_model
                        .clone()
                        .map(|model| model.to_string())
                        .unwrap_or_default(),
                    choices: vec![],
                    usage: chat_completion_response.usage.clone(),
                    system_fingerprint: None,
                    metadata: None,
                };

                if let Some(content) = &delta.content {
                    chat_message.content =
                        Some(MessageContent::String(match chat_message.content {
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
                            // Check if this delta has an ID
                            let has_id = delta_tool_call
                                .id
                                .as_ref()
                                .map(|id| !id.is_empty())
                                .unwrap_or(false);

                            // Try to find existing tool call by id first
                            let existing_index = delta_tool_call
                                .id
                                .as_ref()
                                .filter(|id| !id.is_empty())
                                .and_then(|id| tool_calls_vec.iter().position(|tc| tc.id == *id));

                            // If we have an ID, only match by ID (don't fall back to index)
                            // If no ID, fall back to index matching for backwards compatibility
                            let existing_tool_call = match existing_index {
                                Some(idx) => tool_calls_vec.get_mut(idx),
                                None if !has_id => tool_calls_vec.get_mut(delta_tool_call.index),
                                None => None, // Has ID but no match - will create new tool call
                            };

                            match existing_tool_call {
                                Some(tool_call) => {
                                    let delta_func = delta_tool_call.function.as_ref().unwrap_or(
                                        &FunctionCallDelta {
                                            name: None,
                                            arguments: None,
                                        },
                                    );
                                    // Update name if provided and current name is empty
                                    if let Some(name) = delta_func.name.as_deref() {
                                        if tool_call.function.name.is_empty() {
                                            tool_call.function.name = name.to_string();
                                        }
                                    }
                                    // Append arguments
                                    tool_call.function.arguments =
                                        tool_call.function.arguments.clone()
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
                                            arguments: delta_tool_call
                                                .function
                                                .as_ref()
                                                .and_then(|f| f.arguments.clone())
                                                .unwrap_or_default(),
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                // End stream processing loading when error occurs
                let _ = send_input_event(
                    input_tx,
                    InputEvent::EndLoadingOperation(LoadingOperation::StreamProcessing),
                )
                .await;
                return Err(e.clone());
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

    // End stream processing loading when stream completes
    send_input_event(
        input_tx,
        InputEvent::EndLoadingOperation(LoadingOperation::StreamProcessing),
    )
    .await?;

    Ok(chat_completion_response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use stakpak_shared::models::integrations::openai::{
        ChatCompletionStreamChoice, ChatCompletionStreamResponse, ChatMessageDelta, ToolCallDelta,
    };

    fn create_stream_response_with_tool_call(
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    ) -> ChatCompletionStreamResponse {
        ChatCompletionStreamResponse {
            id: "test".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "test-model".to_string(),
            choices: vec![ChatCompletionStreamChoice {
                index: 0,
                delta: ChatMessageDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![ToolCallDelta {
                        index,
                        id,
                        r#type: Some("function".to_string()),
                        function: Some(FunctionCallDelta { name, arguments }),
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn test_consecutive_tool_calls_with_same_index_different_ids() {
        // This test verifies the bug fix: consecutive tool calls with same index
        // but different IDs should create separate tool calls, not merge arguments
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        // Simulate two tool calls with index 0 but different IDs (Stakai behavior)
        let responses = vec![
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-1".to_string()),
                Some("function_a".to_string()),
                Some("{\"arg\":".to_string()),
            )),
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-1".to_string()),
                None,
                Some("\"value1\"}".to_string()),
            )),
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-2".to_string()),
                Some("function_b".to_string()),
                Some("{\"arg\":".to_string()),
            )),
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-2".to_string()),
                None,
                Some("\"value2\"}".to_string()),
            )),
        ];

        let test_stream = stream::iter(responses);

        // Spawn a task to drain the receiver so it doesn't block
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();

        // Should have 2 separate tool calls, not 1 with merged arguments
        assert_eq!(tool_calls.len(), 2, "Should have 2 separate tool calls");
        assert_eq!(tool_calls[0].id, "tool-1");
        assert_eq!(tool_calls[0].function.name, "function_a");
        assert_eq!(tool_calls[0].function.arguments, "{\"arg\":\"value1\"}");
        assert_eq!(tool_calls[1].id, "tool-2");
        assert_eq!(tool_calls[1].function.name, "function_b");
        assert_eq!(tool_calls[1].function.arguments, "{\"arg\":\"value2\"}");
    }

    #[tokio::test]
    async fn test_tool_calls_fallback_to_index_when_no_id() {
        // This test verifies backwards compatibility: when no ID is provided,
        // tool calls should be matched by index
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        // Simulate tool calls using index-based matching (OpenAI behavior)
        let responses = vec![
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-1".to_string()),
                Some("function_a".to_string()),
                Some("{\"arg\":".to_string()),
            )),
            Ok(create_stream_response_with_tool_call(
                0,
                None, // No ID, should fall back to index
                None,
                Some("\"value1\"}".to_string()),
            )),
        ];

        let test_stream = stream::iter(responses);

        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "tool-1");
        assert_eq!(tool_calls[0].function.arguments, "{\"arg\":\"value1\"}");
    }

    #[tokio::test]
    async fn test_tool_calls_with_incrementing_indices() {
        // Test standard behavior with incrementing indices
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses = vec![
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-1".to_string()),
                Some("func_a".to_string()),
                Some("{\"a\":1}".to_string()),
            )),
            Ok(create_stream_response_with_tool_call(
                1,
                Some("tool-2".to_string()),
                Some("func_b".to_string()),
                Some("{\"b\":2}".to_string()),
            )),
        ];

        let test_stream = stream::iter(responses);

        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();

        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].id, "tool-1");
        assert_eq!(tool_calls[0].function.name, "func_a");
        assert_eq!(tool_calls[1].id, "tool-2");
        assert_eq!(tool_calls[1].function.name, "func_b");
    }

    #[tokio::test]
    async fn test_tool_call_name_and_arguments_in_separate_chunks() {
        // Test that name and arguments coming in separate chunks are merged correctly
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses = vec![
            // First chunk: ID and name only
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-1".to_string()),
                Some("my_function".to_string()),
                None,
            )),
            // Second chunk: same ID, arguments only
            Ok(create_stream_response_with_tool_call(
                0,
                Some("tool-1".to_string()),
                None,
                Some("{\"key\":\"value\"}".to_string()),
            )),
        ];

        let test_stream = stream::iter(responses);

        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        let tool_calls = response.choices[0].message.tool_calls.as_ref().unwrap();

        // Should have 1 tool call with both name and arguments
        assert_eq!(tool_calls.len(), 1, "Should have 1 merged tool call");
        assert_eq!(tool_calls[0].id, "tool-1");
        assert_eq!(tool_calls[0].function.name, "my_function");
        assert_eq!(tool_calls[0].function.arguments, "{\"key\":\"value\"}");
    }
}
