//! Response types from AI providers

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Response from a generation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateResponse {
    /// Generated content
    pub content: Vec<ResponseContent>,
    /// Token usage statistics
    pub usage: Usage,
    /// Why generation finished
    pub finish_reason: FinishReason,
    /// Provider-specific metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl GenerateResponse {
    /// Get the text content from the response
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                ResponseContent::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get all tool calls from the response
    pub fn tool_calls(&self) -> Vec<&ToolCall> {
        self.content
            .iter()
            .filter_map(|c| match c {
                ResponseContent::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect()
    }
}

/// Content in a response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContent {
    /// Text content
    Text {
        /// The generated text
        text: String,
    },
    /// Tool/function call
    ToolCall(ToolCall),
}

/// A tool/function call in the response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this tool call
    pub id: String,
    /// Name of the function to call
    pub name: String,
    /// Arguments as JSON
    pub arguments: Value,
}

/// Token usage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt
    pub prompt_tokens: u32,
    /// Tokens in the completion
    pub completion_tokens: u32,
    /// Total tokens used
    pub total_tokens: u32,
}

/// Why generation finished
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural stop point
    Stop,
    /// Hit token limit
    Length,
    /// Content filtered
    ContentFilter,
    /// Tool call requested
    ToolCalls,
    /// Unknown/other reason
    Other,
}
