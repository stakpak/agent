//! Response types from AI providers

use super::cache::CacheWarning;
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
    /// Provider-specific metadata as raw JSON
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Warnings generated during request processing (e.g., cache validation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<ResponseWarning>>,
}

/// Warning generated during request processing
///
/// Warnings are non-fatal issues that occurred during request processing.
/// The request was still sent, but some settings may have been ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseWarning {
    /// Type of warning (e.g., "cache_validation")
    pub warning_type: String,
    /// Human-readable warning message
    pub message: String,
}

impl ResponseWarning {
    /// Create a new response warning
    pub fn new(warning_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            warning_type: warning_type.into(),
            message: message.into(),
        }
    }
}

impl From<CacheWarning> for ResponseWarning {
    fn from(warning: CacheWarning) -> Self {
        Self {
            warning_type: warning.warning_type.to_string(),
            message: warning.message,
        }
    }
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

    /// Get the reasoning content from the response (extended thinking for Anthropic, reasoning for OpenAI)
    pub fn reasoning(&self) -> Option<String> {
        let reasoning: Vec<String> = self
            .content
            .iter()
            .filter_map(|c| match c {
                ResponseContent::Reasoning { reasoning } => Some(reasoning.clone()),
                _ => None,
            })
            .collect();

        if reasoning.is_empty() {
            None
        } else {
            Some(reasoning.join(""))
        }
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

    /// Check if there are any warnings
    pub fn has_warnings(&self) -> bool {
        self.warnings
            .as_ref()
            .map(|w| !w.is_empty())
            .unwrap_or(false)
    }

    /// Get warnings as a slice
    pub fn warnings(&self) -> &[ResponseWarning] {
        self.warnings.as_deref().unwrap_or(&[])
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
    /// Reasoning content (extended thinking for Anthropic, reasoning for OpenAI)
    Reasoning {
        /// The reasoning text
        reasoning: String,
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
    /// Opaque provider-specific metadata (e.g., Gemini thought_signature).
    /// Must be preserved and echoed back in subsequent requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// Input token details with cache information
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputTokenDetails {
    /// Total input tokens (including cache tokens)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// Tokens not from cache (regular input tokens)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_cache: Option<u32>,
    /// Tokens read from cache
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u32>,
    /// Tokens written to cache
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u32>,
}

/// Output token details
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputTokenDetails {
    /// Total output tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// Text tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<u32>,
    /// Reasoning tokens (extended thinking for Anthropic, reasoning for OpenAI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Total input tokens (for backward compatibility)
    pub prompt_tokens: u32,
    /// Total completion tokens (for backward compatibility)
    pub completion_tokens: u32,
    /// Total tokens used (for backward compatibility)
    pub total_tokens: u32,
    /// Detailed input token breakdown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_token_details: Option<InputTokenDetails>,
    /// Detailed output token breakdown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_token_details: Option<OutputTokenDetails>,
    /// Raw provider-specific usage data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

impl Usage {
    /// Create a new usage with the given prompt and completion tokens
    pub fn new(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            input_token_details: None,
            output_token_details: None,
            raw: None,
        }
    }

    pub fn with_details(
        input_tokens: InputTokenDetails,
        output_tokens: OutputTokenDetails,
        raw: Option<Value>,
    ) -> Self {
        let prompt_tokens = input_tokens.total.unwrap_or(0);
        let completion_tokens = output_tokens.total.unwrap_or(0);
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            input_token_details: Some(input_tokens),
            output_token_details: Some(output_tokens),
            raw,
        }
    }

    /// Get cache read tokens (convenience method)
    pub fn cache_read_tokens(&self) -> Option<u32> {
        self.input_token_details.as_ref().and_then(|d| d.cache_read)
    }

    /// Get cache write tokens (convenience method)
    pub fn cache_write_tokens(&self) -> Option<u32> {
        self.input_token_details
            .as_ref()
            .and_then(|d| d.cache_write)
    }

    /// Get reasoning tokens (convenience method)
    pub fn reasoning_tokens(&self) -> Option<u32> {
        self.output_token_details.as_ref().and_then(|d| d.reasoning)
    }
}

/// Unified finish reason for cross-provider consistency
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReasonKind {
    /// Model generated stop sequence
    Stop,
    /// Model generated maximum number of tokens
    Length,
    /// Content filter violation stopped the model
    ContentFilter,
    /// Model triggered tool calls
    ToolCalls,
    /// Model stopped because of an error
    Error,
    /// Model stopped for other reasons
    Other,
}

/// Why generation finished - includes both unified and raw reason
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishReason {
    /// Unified finish reason for cross-provider consistency
    pub unified: FinishReasonKind,
    /// Raw finish reason from the provider
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl FinishReason {
    /// Create a new finish reason with only unified value
    pub fn new(unified: FinishReasonKind) -> Self {
        Self { unified, raw: None }
    }

    /// Create a new finish reason with both unified and raw values
    pub fn with_raw(unified: FinishReasonKind, raw: impl Into<String>) -> Self {
        Self {
            unified,
            raw: Some(raw.into()),
        }
    }

    /// Convenience constructors
    pub fn stop() -> Self {
        Self::new(FinishReasonKind::Stop)
    }

    pub fn length() -> Self {
        Self::new(FinishReasonKind::Length)
    }

    pub fn content_filter() -> Self {
        Self::new(FinishReasonKind::ContentFilter)
    }

    pub fn tool_calls() -> Self {
        Self::new(FinishReasonKind::ToolCalls)
    }

    pub fn error() -> Self {
        Self::new(FinishReasonKind::Error)
    }

    pub fn other() -> Self {
        Self::new(FinishReasonKind::Other)
    }
}

impl Default for FinishReason {
    fn default() -> Self {
        Self::new(FinishReasonKind::Other)
    }
}

// Allow comparing FinishReason with FinishReasonKind directly
impl PartialEq<FinishReasonKind> for FinishReason {
    fn eq(&self, other: &FinishReasonKind) -> bool {
        self.unified == *other
    }
}
