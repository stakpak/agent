//! Streaming types for AI generation

use super::{FinishReason, Usage};
use crate::error::Result;
use futures::Stream;
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "tracing")]
use tracing::Span;

#[cfg(feature = "tracing")]
use crate::tracing as gen_ai_tracing;

/// Accumulated tool call during streaming
#[cfg(feature = "tracing")]
#[derive(Debug, Clone)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// A stream of generation events
#[pin_project]
pub struct GenerateStream {
    #[pin]
    inner: Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>,
    /// Optional span for tracing - usage is recorded when Finish event is received
    #[cfg(feature = "tracing")]
    span: Option<Span>,
    /// Accumulated text content for tracing
    #[cfg(feature = "tracing")]
    accumulated_text: String,
    /// Accumulated tool calls for tracing
    #[cfg(feature = "tracing")]
    accumulated_tool_calls: Vec<AccumulatedToolCall>,
}

impl GenerateStream {
    /// Create a new stream from a boxed stream
    pub fn new(stream: Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>) -> Self {
        Self {
            inner: stream,
            #[cfg(feature = "tracing")]
            span: None,
            #[cfg(feature = "tracing")]
            accumulated_text: String::new(),
            #[cfg(feature = "tracing")]
            accumulated_tool_calls: Vec::new(),
        }
    }

    /// Create a new stream with an associated tracing span
    ///
    /// When the stream emits a `Finish` event, token usage and response content
    /// will be recorded on the span automatically.
    #[cfg(feature = "tracing")]
    pub fn with_span(
        stream: Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>,
        span: Span,
    ) -> Self {
        Self {
            inner: stream,
            span: Some(span),
            accumulated_text: String::new(),
            accumulated_tool_calls: Vec::new(),
        }
    }
}

impl Stream for GenerateStream {
    type Item = Result<StreamEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let poll_result = this.inner.poll_next(cx);

        #[cfg(feature = "tracing")]
        if let Poll::Ready(Some(Ok(ref event))) = poll_result {
            // Accumulate content for tracing
            match event {
                StreamEvent::TextDelta { delta, .. } => {
                    this.accumulated_text.push_str(delta);
                }
                StreamEvent::ToolCallStart { id, name } => {
                    this.accumulated_tool_calls.push(AccumulatedToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: String::new(),
                    });
                }
                StreamEvent::ToolCallDelta { id, delta } => {
                    if let Some(tc) = this
                        .accumulated_tool_calls
                        .iter_mut()
                        .find(|tc| &tc.id == id)
                    {
                        tc.arguments.push_str(delta);
                    }
                }
                StreamEvent::ToolCallEnd {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    // Update the tool call with final name and arguments
                    if let Some(tc) = this
                        .accumulated_tool_calls
                        .iter_mut()
                        .find(|tc| &tc.id == id)
                    {
                        tc.name = name.clone();
                        tc.arguments = arguments.to_string();
                    } else {
                        // Tool call wasn't started, add it now
                        this.accumulated_tool_calls.push(AccumulatedToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: arguments.to_string(),
                        });
                    }
                }
                StreamEvent::Finish { usage, reason } => {
                    // Record usage and completion on span
                    if let Some(span) = this.span {
                        let _guard = span.enter();

                        span.record("gen_ai.usage.input_tokens", usage.prompt_tokens as i64);
                        span.record("gen_ai.usage.output_tokens", usage.completion_tokens as i64);

                        // Non-standard: Cache token metrics (not part of OTel GenAI semantic conventions)
                        if let Some(cache_read) = usage.cache_read_tokens() {
                            span.record("gen_ai.usage.cache_read_input_tokens", cache_read as i64);
                        }
                        if let Some(cache_write) = usage.cache_write_tokens() {
                            span.record(
                                "gen_ai.usage.cache_write_input_tokens",
                                cache_write as i64,
                            );
                        }

                        // finish_reasons is an array per OTel spec
                        let finish_reason = format!("{:?}", reason.unified);
                        let finish_reasons_json =
                            serde_json::to_string(&vec![&finish_reason]).unwrap_or_default();
                        span.record(
                            "gen_ai.response.finish_reasons",
                            finish_reasons_json.as_str(),
                        );

                        // Record response content as span attribute
                        let tool_calls: Vec<gen_ai_tracing::ToolCallInfo> = this
                            .accumulated_tool_calls
                            .iter()
                            .map(|tc| gen_ai_tracing::ToolCallInfo {
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            })
                            .collect();

                        gen_ai_tracing::record_streamed_response(
                            this.accumulated_text,
                            &tool_calls,
                            &finish_reason,
                        );
                    }
                }
                _ => {}
            }
        }

        poll_result
    }
}

/// Events emitted during streaming generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Stream started
    Start {
        /// Unique ID for this generation
        id: String,
    },

    /// Text content delta
    TextDelta {
        /// Generation ID
        id: String,
        /// Text delta to append
        delta: String,
    },

    /// Reasoning content delta (extended thinking for Anthropic, reasoning for OpenAI)
    ReasoningDelta {
        /// Generation ID
        id: String,
        /// Reasoning delta to append
        delta: String,
    },

    /// Tool call started
    ToolCallStart {
        /// Tool call ID
        id: String,
        /// Function name
        name: String,
    },

    /// Tool call arguments delta
    ToolCallDelta {
        /// Tool call ID
        id: String,
        /// Arguments delta (partial JSON)
        delta: String,
    },

    /// Tool call completed
    ToolCallEnd {
        /// Tool call ID
        id: String,
        /// Complete function name
        name: String,
        /// Complete arguments as JSON
        arguments: Value,
        /// Opaque provider-specific metadata (e.g., Gemini thought_signature)
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },

    /// Generation finished
    Finish {
        /// Token usage
        usage: Usage,
        /// Why it finished
        reason: FinishReason,
    },

    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

impl StreamEvent {
    /// Create a start event
    pub fn start(id: impl Into<String>) -> Self {
        Self::Start { id: id.into() }
    }

    /// Create a text delta event
    pub fn text_delta(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self::TextDelta {
            id: id.into(),
            delta: delta.into(),
        }
    }

    /// Create a reasoning delta event (extended thinking for Anthropic, reasoning for OpenAI)
    pub fn reasoning_delta(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self::ReasoningDelta {
            id: id.into(),
            delta: delta.into(),
        }
    }

    /// Create a tool call start event
    pub fn tool_call_start(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self::ToolCallStart {
            id: id.into(),
            name: name.into(),
        }
    }

    /// Create a tool call delta event
    pub fn tool_call_delta(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self::ToolCallDelta {
            id: id.into(),
            delta: delta.into(),
        }
    }

    /// Create a tool call end event
    pub fn tool_call_end(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self::ToolCallEnd {
            id: id.into(),
            name: name.into(),
            arguments,
            metadata: None,
        }
    }

    /// Create a tool call end event with metadata
    pub fn tool_call_end_with_metadata(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: Value,
        metadata: Option<Value>,
    ) -> Self {
        Self::ToolCallEnd {
            id: id.into(),
            name: name.into(),
            arguments,
            metadata,
        }
    }

    /// Create a finish event
    pub fn finish(usage: Usage, reason: FinishReason) -> Self {
        Self::Finish { usage, reason }
    }

    /// Create an error event
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}
