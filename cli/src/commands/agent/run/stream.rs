use crate::commands::agent::run::tui::send_input_event;
use futures_util::{Stream, StreamExt};
use stakai::{ContentPart, Message, MessageContent, Model, StreamEvent, ToolCall};
use stakpak_api::models::{
    AgentStreamEvent, ApiStreamError, CompletionResponse, stakai_usage_to_llm_usage,
};
use stakpak_shared::models::agent_runtime::ToolCallStreamInfo;
use stakpak_tui::{InputEvent, LoadingOperation};
use uuid::Uuid;

/// Accumulates streaming tool call events into complete tool calls.
pub struct ToolCallAccumulator {
    tool_calls: Vec<ToolCall>,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self {
            tool_calls: Vec::new(),
        }
    }

    /// Process a StakAI tool call stream event.
    pub fn process_event(&mut self, event: &StreamEvent) {
        let (id, event_name, event_arguments, event_delta, event_metadata) = match event {
            StreamEvent::ToolCallStart { id, name } => (id.as_str(), Some(name), None, None, None),
            StreamEvent::ToolCallDelta { id, delta } => {
                (id.as_str(), None, None, Some(delta), None)
            }
            StreamEvent::ToolCallEnd {
                id,
                name,
                arguments,
                metadata,
            } => (
                id.as_str(),
                Some(name),
                Some(arguments),
                None,
                metadata.as_ref(),
            ),
            _ => return,
        };

        match self.find_tool_call(id) {
            Some(tool_call) => {
                if let Some(name) = event_name
                    && tool_call.name.is_empty()
                {
                    tool_call.name = name.to_string();
                }
                if let Some(arguments) = event_arguments {
                    tool_call.arguments = arguments.clone();
                }
                if let Some(delta) = event_delta {
                    let mut raw = tool_call
                        .arguments
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| tool_call.arguments.to_string());
                    if raw == "null" {
                        raw.clear();
                    }
                    raw.push_str(delta);
                    tool_call.arguments = serde_json::Value::String(raw);
                }
                if let Some(metadata) = event_metadata {
                    tool_call.metadata = Some(metadata.clone());
                }
            }
            None => {
                self.tool_calls.push(ToolCall {
                    id: id.to_string(),
                    name: event_name.cloned().unwrap_or_default(),
                    arguments: event_arguments
                        .cloned()
                        .or_else(|| event_delta.cloned().map(serde_json::Value::String))
                        .unwrap_or_else(|| serde_json::Value::String(String::new())),
                    metadata: event_metadata.cloned(),
                });
            }
        }
    }

    fn find_tool_call(&mut self, id: &str) -> Option<&mut ToolCall> {
        self.tool_calls.iter_mut().find(|tc| tc.id == id)
    }

    /// Get the accumulated tool calls, filtering out empty placeholders.
    pub fn into_tool_calls(self) -> Vec<ToolCall> {
        self.tool_calls
            .into_iter()
            .filter(|tc| !tc.id.is_empty())
            .collect()
    }

    /// Get a snapshot of current streaming progress for each tool call.
    /// Used to send progress updates to the TUI during streaming.
    pub fn progress_snapshot(&self) -> Vec<ToolCallStreamInfo> {
        self.tool_calls
            .iter()
            .filter(|tc| !tc.id.is_empty())
            .map(|tc| {
                // Best-effort: try to extract "description" from partial JSON args
                let description = tc
                    .arguments
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let args_len = tc.arguments.to_string().len();

                ToolCallStreamInfo {
                    name: tc.name.clone(),
                    args_tokens: args_len / 4, // rough chars-to-tokens estimate
                    description,
                }
            })
            .collect()
    }
}

pub(crate) fn tool_call_content_part(tool_call: ToolCall) -> ContentPart {
    let ToolCall {
        id,
        name,
        arguments,
        metadata,
    } = tool_call;
    let mut part = ContentPart::tool_call(id, name, arguments);
    if let ContentPart::ToolCall {
        metadata: part_metadata,
        ..
    } = &mut part
    {
        *part_metadata = metadata;
    }
    part
}

pub async fn process_responses_stream(
    stream: impl Stream<Item = Result<AgentStreamEvent, ApiStreamError>>,
    input_tx: &tokio::sync::mpsc::Sender<InputEvent>,
) -> Result<CompletionResponse, ApiStreamError> {
    let mut stream = Box::pin(stream);

    let mut completion_response = CompletionResponse {
        id: "".to_string(),
        created: 0,
        model: "".to_string(),
        message: Message::new(stakai::Role::Assistant, ""),
        usage: stakai::Usage::default(),
        metadata: None,
    };

    let mut current_model: Option<Model> = None;
    let mut text = String::new();
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
            Ok(response) => match response {
                AgentStreamEvent::Model(model) => {
                    let should_send = match &current_model {
                        Some(existing) => existing.id != model.id,
                        None => true,
                    };
                    if should_send {
                        current_model = Some(model.clone());
                        completion_response.model = model.id.clone();
                        send_input_event(input_tx, InputEvent::StreamModel(model.clone())).await?;
                    }
                }
                AgentStreamEvent::Metadata(metadata) => {
                    completion_response.metadata = Some(metadata.clone());
                }
                AgentStreamEvent::Event(event) => match event {
                    StreamEvent::Start { id } => {
                        completion_response.id = id.clone();
                    }
                    StreamEvent::TextDelta { delta, .. } => {
                        text.push_str(delta);
                        send_input_event(
                            input_tx,
                            InputEvent::StreamAssistantMessage(message_id, delta.clone()),
                        )
                        .await?;
                    }
                    StreamEvent::ToolCallStart { .. }
                    | StreamEvent::ToolCallDelta { .. }
                    | StreamEvent::ToolCallEnd { .. } => {
                        tool_call_accumulator.process_event(event);
                        let snapshot = tool_call_accumulator.progress_snapshot();
                        if !snapshot.is_empty() {
                            send_input_event(
                                input_tx,
                                InputEvent::StreamToolCallProgress(snapshot),
                            )
                            .await?;
                        }
                    }
                    StreamEvent::Finish { usage, .. } => {
                        completion_response.usage = usage.clone();
                        send_input_event(
                            input_tx,
                            InputEvent::StreamUsage(stakai_usage_to_llm_usage(usage)),
                        )
                        .await?;
                    }
                    StreamEvent::ReasoningDelta { .. } => {}
                    StreamEvent::Error { message } => {
                        let _ = send_input_event(
                            input_tx,
                            InputEvent::EndLoadingOperation(LoadingOperation::StreamProcessing),
                        )
                        .await;
                        return Err(ApiStreamError::Unknown(message.clone()));
                    }
                },
            },
            Err(e) => {
                send_input_event(
                    input_tx,
                    InputEvent::EndLoadingOperation(LoadingOperation::StreamProcessing),
                )
                .await?;
                return Err(e.clone());
            }
        }
    }

    let final_tool_calls = tool_call_accumulator.into_tool_calls();
    completion_response.message = if final_tool_calls.is_empty() {
        Message::new(stakai::Role::Assistant, text)
    } else {
        let mut parts = Vec::new();
        if !text.is_empty() {
            parts.push(ContentPart::text(text));
        }
        parts.extend(final_tool_calls.into_iter().map(tool_call_content_part));
        Message::new(stakai::Role::Assistant, MessageContent::Parts(parts))
    };

    send_input_event(
        input_tx,
        InputEvent::EndLoadingOperation(LoadingOperation::StreamProcessing),
    )
    .await?;

    Ok(completion_response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    fn event(event: StreamEvent) -> Result<AgentStreamEvent, ApiStreamError> {
        Ok(AgentStreamEvent::Event(event))
    }

    #[tokio::test]
    async fn accumulates_text_and_usage_from_stakai_events() {
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);
        let responses = vec![
            Ok(AgentStreamEvent::Model(Model::custom("test-model", "test"))),
            event(StreamEvent::Start {
                id: "gen-1".to_string(),
            }),
            event(StreamEvent::TextDelta {
                id: "gen-1".to_string(),
                delta: "Hello".to_string(),
            }),
            event(StreamEvent::TextDelta {
                id: "gen-1".to_string(),
                delta: " world".to_string(),
            }),
            event(StreamEvent::Finish {
                usage: stakai::Usage::new(100, 50),
                reason: stakai::FinishReason::stop(),
            }),
        ];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let response = process_responses_stream(test_stream, &input_tx)
            .await
            .unwrap();
        assert_eq!(response.id, "gen-1");
        assert_eq!(response.model, "test-model");
        assert_eq!(response.message.content.text().unwrap(), "Hello world");
        assert_eq!(response.usage.prompt_tokens, 100);
        assert_eq!(response.usage.completion_tokens, 50);
    }

    #[tokio::test]
    async fn accumulates_tool_call_events_by_id() {
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);
        let responses = vec![
            event(StreamEvent::ToolCallStart {
                id: "tool-1".to_string(),
                name: "my_function".to_string(),
            }),
            event(StreamEvent::ToolCallDelta {
                id: "tool-1".to_string(),
                delta: "{\"key\":".to_string(),
            }),
            event(StreamEvent::ToolCallDelta {
                id: "tool-1".to_string(),
                delta: "\"value\"}".to_string(),
            }),
        ];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let response = process_responses_stream(test_stream, &input_tx)
            .await
            .unwrap();
        let tool_calls: Vec<_> = response
            .message
            .parts()
            .into_iter()
            .filter_map(|part| match part {
                ContentPart::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => Some((id, name, arguments)),
                _ => None,
            })
            .collect();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].0, "tool-1");
        assert_eq!(tool_calls[0].1, "my_function");
        assert_eq!(
            tool_calls[0].2,
            serde_json::Value::String("{\"key\":\"value\"}".to_string())
        );
    }

    #[tokio::test]
    async fn preserves_tool_call_metadata_in_final_assistant_message() {
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(100);
        let metadata = serde_json::json!({"thought_signature": "sig-123"});
        let responses = vec![event(StreamEvent::ToolCallEnd {
            id: "tool-1".to_string(),
            name: "my_function".to_string(),
            arguments: serde_json::json!({"key": "value"}),
            metadata: Some(metadata.clone()),
        })];

        let test_stream = stream::iter(responses);
        tokio::spawn(async move { while input_rx.recv().await.is_some() {} });

        let response = process_responses_stream(test_stream, &input_tx)
            .await
            .unwrap();
        let tool_call_metadata = response
            .message
            .parts()
            .into_iter()
            .find_map(|part| match part {
                ContentPart::ToolCall { metadata, .. } => metadata,
                _ => None,
            });

        assert_eq!(tool_call_metadata, Some(metadata));
    }
}
