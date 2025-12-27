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
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
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

/// OpenAI usage statistics
#[derive(Debug, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// OpenAI streaming chunk
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChatDelta,
    pub finish_reason: Option<String>,
}

/// OpenAI delta content
#[derive(Debug, Deserialize)]
pub struct ChatDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCallDelta>>,
}

/// OpenAI tool call delta (for streaming)
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
pub struct OpenAIFunctionCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}
