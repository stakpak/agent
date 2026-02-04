use crate::commands::agent::run::tui::send_input_event;
use futures_util::{Stream, StreamExt};
use stakpak_api::models::ApiStreamError;
use stakpak_shared::models::{
    integrations::openai::{
        ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage,
        FinishReason, FunctionCall, MessageContent, Role, ToolCall, ToolCallDelta,
    },
    llm::{LLMModel, LLMTokenUsage},
};
use stakpak_tui::{InputEvent, LoadingOperation};
use uuid::Uuid;

/// Accumulates streaming tool call deltas into complete tool calls.
///
/// Handles two different streaming behaviors:
/// - **ID-based matching**: When delta has an ID, match by ID only (used by Anthropic/StakAI)
/// - **Index-based matching**: When delta has no ID, fall back to index (used by OpenAI)
///
/// This distinction is important because some providers (like Anthropic via StakAI adapter)
/// send multiple tool calls with the same index but different IDs.
pub struct ToolCallAccumulator {
    tool_calls: Vec<ToolCall>,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self {
            tool_calls: Vec::new(),
        }
    }

    /// Process a tool call delta and accumulate it into the appropriate tool call.
    pub fn process_delta(&mut self, delta: &ToolCallDelta) {
        let delta_id = delta.id.as_deref().filter(|id| !id.is_empty());
        let delta_func = delta.function.as_ref();

        match self.find_tool_call(delta_id, delta.index) {
            Some(tool_call) => {
                // Update existing tool call
                if let Some(func) = delta_func {
                    if let Some(name) = func.name.as_deref()
                        && tool_call.function.name.is_empty()
                    {
                        tool_call.function.name = name.to_string();
                    }
                    if let Some(args) = &func.arguments {
                        tool_call.function.arguments.push_str(args);
                    }
                }
            }
            None => {
                // Create new tool call
                self.create_tool_call(delta);
            }
        }
    }

    /// Find an existing tool call by ID or index.
    /// Returns None if a new tool call should be created.
    fn find_tool_call(&mut self, id: Option<&str>, index: usize) -> Option<&mut ToolCall> {
        match id {
            // Has ID: only match by ID, never fall back to index
            Some(id) => self.tool_calls.iter_mut().find(|tc| tc.id == id),
            // No ID: fall back to index-based matching for backwards compatibility
            None => self.tool_calls.get_mut(index),
        }
    }

    /// Create a new tool call from a delta.
    fn create_tool_call(&mut self, delta: &ToolCallDelta) {
        // Pad with empty tool calls if needed (for sparse indices)
        while self.tool_calls.len() < delta.index {
            self.tool_calls.push(ToolCall {
                id: String::new(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: String::new(),
                    arguments: String::new(),
                },
            });
        }

        let func = delta.function.as_ref();
        self.tool_calls.push(ToolCall {
            id: delta.id.clone().unwrap_or_default(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: func.and_then(|f| f.name.clone()).unwrap_or_default(),
                arguments: func.and_then(|f| f.arguments.clone()).unwrap_or_default(),
            },
        });
    }

    /// Get the accumulated tool calls, filtering out empty placeholders.
    pub fn into_tool_calls(self) -> Vec<ToolCall> {
        self.tool_calls
            .into_iter()
            .filter(|tc| !tc.id.is_empty())
            .collect()
    }
}

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
    let mut response_metadata: Option<serde_json::Value> = None;

    let mut llm_model: Option<LLMModel> = None;

    let mut chat_message = ChatMessage {
        role: Role::Assistant,
        content: None,
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
        ..Default::default()
    };
    let message_id = Uuid::new_v4();
    let mut tool_call_accumulator = ToolCallAccumulator::new();

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
                if let Some(metadata) = &response.metadata {
                    response_metadata = Some(metadata.clone());
                }

                // Skip chunks with no choices (e.g., usage-only events)
                if response.choices.is_empty() {
                    continue;
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
                        tool_call_accumulator.process_delta(delta_tool_call);
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

    // Get accumulated tool calls (already filtered for empty IDs)
    let final_tool_calls = tool_call_accumulator.into_tool_calls();
    chat_message.tool_calls = if final_tool_calls.is_empty() {
        None
    } else {
        Some(final_tool_calls)
    };

    chat_completion_response.choices.push(ChatCompletionChoice {
        index: 0,
        message: chat_message.clone(),
        finish_reason: FinishReason::Stop,
        logprobs: None,
    });
    chat_completion_response.metadata = response_metadata;

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
        ChatCompletionStreamChoice, ChatCompletionStreamResponse, ChatMessageDelta,
        FunctionCallDelta,
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

    fn create_content_response(content: &str) -> ChatCompletionStreamResponse {
        ChatCompletionStreamResponse {
            id: "test".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "test-model".to_string(),
            choices: vec![ChatCompletionStreamChoice {
                index: 0,
                delta: ChatMessageDelta {
                    role: None,
                    content: Some(content.to_string()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            metadata: None,
        }
    }

    fn create_usage_only_response() -> ChatCompletionStreamResponse {
        ChatCompletionStreamResponse {
            id: "test".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "".to_string(),
            choices: vec![], // Empty choices — usage-only event
            usage: Some(LLMTokenUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
                prompt_tokens_details: None,
            }),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn test_empty_choices_does_not_panic() {
        // This is the exact scenario that caused the index-out-of-bounds panic:
        // Some providers send a final event with usage data but no choices.
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses = vec![
            Ok(create_content_response("Hello")),
            Ok(create_usage_only_response()), // Empty choices — was panicking
        ];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // Content should still be accumulated from the first chunk
        let content = response.choices[0]
            .message
            .content
            .as_ref()
            .unwrap()
            .to_string();
        assert_eq!(content, "Hello");
        // Usage from the empty-choices event should still be captured
        assert_eq!(response.usage.prompt_tokens, 100);
        assert_eq!(response.usage.completion_tokens, 50);
        assert_eq!(response.usage.total_tokens, 150);
    }

    #[tokio::test]
    async fn test_only_usage_events_no_content() {
        // Stream with only usage events and no content at all
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses = vec![Ok(create_usage_only_response())];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.choices.len(), 1);
        assert!(response.choices[0].message.content.is_none());
        assert!(response.choices[0].message.tool_calls.is_none());
        assert_eq!(response.usage.total_tokens, 150);
    }

    #[tokio::test]
    async fn test_multiple_empty_choices_interspersed() {
        // Multiple empty-choices events interspersed with content
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses = vec![
            Ok(create_usage_only_response()),
            Ok(create_content_response("Hello")),
            Ok(create_usage_only_response()),
            Ok(create_content_response(" World")),
            Ok(create_usage_only_response()),
        ];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        let content = response.choices[0]
            .message
            .content
            .as_ref()
            .unwrap()
            .to_string();
        assert_eq!(content, "Hello World");
    }

    #[tokio::test]
    async fn test_empty_stream() {
        // Completely empty stream — no events at all
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses: Vec<Result<ChatCompletionStreamResponse, ApiStreamError>> = vec![];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // Should have one choice with empty content
        assert_eq!(response.choices.len(), 1);
        assert!(response.choices[0].message.content.is_none());
        assert!(response.choices[0].message.tool_calls.is_none());
    }

    #[tokio::test]
    async fn test_content_accumulation_across_chunks() {
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses = vec![
            Ok(create_content_response("Hello")),
            Ok(create_content_response(", ")),
            Ok(create_content_response("world")),
            Ok(create_content_response("!")),
        ];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        let content = response.choices[0]
            .message
            .content
            .as_ref()
            .unwrap()
            .to_string();
        assert_eq!(content, "Hello, world!");
    }

    #[tokio::test]
    async fn test_stream_error_propagated() {
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);

        let responses: Vec<Result<ChatCompletionStreamResponse, ApiStreamError>> = vec![
            Ok(create_content_response("start")),
            Err(ApiStreamError::Unknown("connection lost".to_string())),
        ];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let result = process_responses_stream(test_stream, &input_tx).await;
        assert!(result.is_err());
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
