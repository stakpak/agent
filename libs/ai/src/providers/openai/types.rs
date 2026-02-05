//! OpenAI-specific types

use serde::{Deserialize, Serialize};

/// Configuration for OpenAI provider
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    /// API key
    pub api_key: String,
    /// Base URL (default: https://api.openai.com/v1)
    pub base_url: String,
    /// Organization ID (optional)
    pub organization: Option<String>,
}

impl OpenAIConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            organization: None,
        }
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Set organization
    pub fn with_organization(mut self, org: impl Into<String>) -> Self {
        self.organization = Some(org.into());
        self
    }
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self::new(std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| String::new()))
    }
}

/// OpenAI chat completion request
#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

/// Options for streaming responses
#[derive(Debug, Serialize)]
pub struct StreamOptions {
    /// Include usage statistics in the streaming response
    pub include_usage: bool,
}

/// OpenAI chat message
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// OpenAI tool call
#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: OpenAIFunctionCall,
}

/// OpenAI function call
#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String, // JSON string
}

/// OpenAI chat completion response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: ChatUsage,
}

/// OpenAI chat choice
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

/// OpenAI prompt token details
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct PromptTokensDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u32>,
}

/// OpenAI completion token details
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CompletionTokensDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_prediction_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_prediction_tokens: Option<u32>,
}

/// OpenAI usage statistics
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

/// OpenAI streaming chunk
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
}

/// OpenAI chunk choice
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChatDelta,
    pub finish_reason: Option<String>,
}

/// OpenAI delta content
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCallDelta>>,
}

/// OpenAI tool call delta (for streaming)
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct OpenAIToolCallDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<OpenAIFunctionCallDelta>,
}

/// OpenAI function call delta
#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAIFunctionCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ============================================================================
// Responses API Types
// ============================================================================

/// OpenAI Responses API request
#[derive(Debug, Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    /// Input items - uses Value because format varies by item type
    pub input: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

/// Reasoning configuration for reasoning models
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReasoningConfig {
    pub effort: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Input content types for Responses API
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum InputContent {
    #[serde(rename = "input_text")]
    Text { text: String },
    #[serde(rename = "input_image")]
    Image { image_url: String, detail: String },
}

/// Output content types for Responses API (in assistant messages)
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum AssistantOutputContent {
    #[serde(rename = "output_text")]
    Text {
        text: String,
        #[serde(default)]
        annotations: Vec<serde_json::Value>,
    },
}

/// Responses API non-streaming response
#[derive(Debug, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: String,
    pub created_at: u64,
    pub model: String,
    pub output: Vec<ResponsesOutputItem>,
    pub usage: ResponsesUsage,
    #[serde(default)]
    pub status: String,
}

/// Output item in Responses API response
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ResponsesOutputItem {
    #[serde(rename = "message")]
    Message {
        id: String,
        role: String,
        content: Vec<ResponsesOutputContent>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        id: String,
        content: Vec<ReasoningContent>,
    },
}

/// Output content in Responses API message
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ResponsesOutputContent {
    #[serde(rename = "output_text")]
    Text { text: String },
}

/// Reasoning content types
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ReasoningContent {
    #[serde(rename = "reasoning_text")]
    Text { text: String },
    #[serde(rename = "reasoning.encrypted_content")]
    Encrypted { id: String, data: String },
}

/// Usage statistics for Responses API
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ResponsesUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens_details: Option<ResponsesInputTokensDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_details: Option<ResponsesOutputTokensDetails>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ResponsesInputTokensDetails {
    #[serde(default)]
    pub cached_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ResponsesOutputTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: u32,
}
