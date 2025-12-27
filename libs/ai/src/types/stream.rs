//! Streaming types for AI generation

use super::{FinishReason, Usage};
use crate::error::Result;
use futures::Stream;
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A stream of generation events
#[pin_project]
pub struct GenerateStream {
    #[pin]
    inner: Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>,
}

impl GenerateStream {
    /// Create a new stream from a boxed stream
    pub fn new(stream: Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>) -> Self {
        Self { inner: stream }
    }
}

impl Stream for GenerateStream {
    type Item = Result<StreamEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.inner.poll_next(cx)
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
