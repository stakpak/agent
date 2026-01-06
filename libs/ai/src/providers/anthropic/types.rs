//! Anthropic-specific types

use crate::types::CacheControl;
use serde::{Deserialize, Serialize};

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
            base_url: "https://api.anthropic.com/v1/".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            beta_features: vec![],
        }
    }

    /// Set base URL
    /// Normalizes the URL by stripping `/messages` suffix if present,
    /// since the provider appends the endpoint path automatically.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let mut url = base_url.into();
        // Strip /messages suffix if user provided full endpoint URL
        if url.ends_with("/messages") {
            url = url.trim_end_matches("/messages").to_string();
        } else if url.ends_with("/messages/") {
            url = url.trim_end_matches("/messages/").to_string();
        }
        // Ensure URL ends with /
        if !url.ends_with('/') {
            url.push('/');
        }
        self.base_url = url;
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

/// Anthropic cache control (for prompt caching)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicCacheControl {
    /// Cache type (currently only "ephemeral")
    #[serde(rename = "type")]
    pub type_: String,
    /// Optional TTL (e.g., "1h")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

impl From<&CacheControl> for AnthropicCacheControl {
    fn from(cache: &CacheControl) -> Self {
        match cache {
            CacheControl::Ephemeral { ttl } => Self {
                type_: "ephemeral".to_string(),
                ttl: ttl.clone(),
            },
        }
    }
}

impl AnthropicCacheControl {
    /// Create ephemeral cache control
    pub fn ephemeral() -> Self {
        Self {
            type_: "ephemeral".to_string(),
            ttl: None,
        }
    }

    /// Create ephemeral cache control with TTL
    pub fn ephemeral_with_ttl(ttl: impl Into<String>) -> Self {
        Self {
            type_: "ephemeral".to_string(),
            ttl: Some(ttl.into()),
        }
    }
}

/// Anthropic system content block (with cache control support)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicSystemBlock {
    /// Type (always "text" for system messages)
    #[serde(rename = "type")]
    pub type_: String,
    /// The text content
    pub text: String,
    /// Optional cache control
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

impl AnthropicSystemBlock {
    /// Create a new system block with text
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            type_: "text".to_string(),
            text: text.into(),
            cache_control: None,
        }
    }

    /// Add cache control to this block
    pub fn with_cache_control(mut self, cache_control: AnthropicCacheControl) -> Self {
        self.cache_control = Some(cache_control);
        self
    }
}

/// Anthropic system content (can be string or array of blocks)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicSystemContent {
    /// Simple string (no cache control)
    String(String),
    /// Array of blocks (supports cache control)
    Blocks(Vec<AnthropicSystemBlock>),
}

/// Anthropic messages request
#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<AnthropicSystemContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

/// Thinking/reasoning configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicThinkingConfig {
    #[serde(rename = "type")]
    pub type_: String,
    pub budget_tokens: u32,
}

/// Anthropic message
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicMessageContent,
}

/// Anthropic response
#[derive(Debug, Serialize, Deserialize)]
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

/// Anthropic content block (with cache control support)
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContent {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    Image {
        source: AnthropicSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    Thinking {
        thinking: String,
        signature: String,
        // Note: thinking blocks cannot have cache_control directly
    },
    RedactedThinking {
        data: String,
        // Note: redacted thinking blocks cannot have cache_control directly
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<AnthropicMessageContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
}

/// Anthropic message content (can be string or array of content blocks)
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum AnthropicMessageContent {
    String(String),
    Blocks(Vec<AnthropicContent>),
}

/// Anthropic source (for images/PDFs)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicSource {
    #[serde(rename = "type")]
    pub type_: String, // "base64"
    pub media_type: String,
    pub data: String,
}

/// Anthropic usage statistics
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AnthropicError>,
}

/// Anthropic streaming delta
#[derive(Debug, Deserialize)]
pub struct AnthropicDelta {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub text: Option<String>,
    pub thinking: Option<String>,
    pub _signature: Option<String>,
    pub partial_json: Option<String>,
    pub _stop_reason: Option<String>,
    pub _stop_sequence: Option<String>,
}

/// Anthropic error details
#[derive(Debug, Deserialize)]
pub struct AnthropicError {
    pub message: String,
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
