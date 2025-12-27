//! Anthropic-specific types

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration for Anthropic provider
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// API key
    pub api_key: String,
    /// Base URL (default: https://api.anthropic.com/v1)
    pub base_url: String,
    /// Anthropic API version (default: 2023-06-01)
    pub anthropic_version: String,
    /// Beta features to enable (e.g., ["prompt-caching-2024-07-31"])
    pub beta_features: Vec<String>,
}

impl AnthropicConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            beta_features: vec![],
        }
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Set API version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.anthropic_version = version.into();
        self
    }

    /// Add beta feature
    pub fn with_beta_feature(mut self, feature: impl Into<String>) -> Self {
        self.beta_features.push(feature.into());
        self
    }
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self::new(std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| String::new()))
    }
}

/// Anthropic messages request
#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32, // Required by Anthropic
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

/// Thinking/reasoning configuration
#[derive(Debug, Serialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub type_: String, // "enabled"
    pub budget_tokens: u32,
}

/// Anthropic message
#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String, // "user" | "assistant"
    pub content: Value,
}

/// Anthropic response
#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub role: String,
    pub content: Vec<AnthropicContent>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: AnthropicUsage,
}

/// Anthropic content block
#[derive(Debug, Deserialize)]
pub struct AnthropicContent {
    #[serde(rename = "type")]
    pub type_: String, // "text" | "thinking" | "tool_use"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

/// Anthropic usage statistics
#[derive(Debug, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Anthropic streaming event
#[derive(Debug, Deserialize)]
pub struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<AnthropicResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_block: Option<AnthropicContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<AnthropicDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AnthropicUsage>,
}

/// Anthropic delta content
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AnthropicDelta {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_json: Option<String>,
}

/// Infer max_tokens based on model name
pub fn infer_max_tokens(model: &str) -> u32 {
    if model.contains("opus-4-5") || model.contains("sonnet-4") || model.contains("haiku-4") {
        64000
    } else if model.contains("opus-4") {
        32000
    } else if model.contains("3-5") {
        8192
    } else {
        4096
    }
}
