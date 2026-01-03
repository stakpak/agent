//! Anthropic-specific types

use serde::{Deserialize, Serialize};

/// Authentication type for Anthropic
#[derive(Debug, Clone)]
pub enum AnthropicAuth {
    /// API key authentication (x-api-key header)
    ApiKey(String),
    /// OAuth 2.0 authentication (Bearer token)
    OAuth {
        /// Access token
        access_token: String,
    },
}

impl AnthropicAuth {
    /// Create API key authentication
    pub fn api_key(key: impl Into<String>) -> Self {
        Self::ApiKey(key.into())
    }

    /// Create OAuth authentication
    pub fn oauth(access_token: impl Into<String>) -> Self {
        Self::OAuth {
            access_token: access_token.into(),
        }
    }

    /// Check if credentials are empty
    pub fn is_empty(&self) -> bool {
        match self {
            Self::ApiKey(key) => key.is_empty(),
            Self::OAuth { access_token } => access_token.is_empty(),
        }
    }

    /// Get the authorization header value
    pub fn to_header(&self) -> (&'static str, String) {
        match self {
            Self::ApiKey(key) => ("x-api-key", key.clone()),
            Self::OAuth { access_token } => ("authorization", format!("Bearer {}", access_token)),
        }
    }
}

/// Configuration for Anthropic provider
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// Authentication (API key or OAuth)
    pub auth: AnthropicAuth,
    /// Base URL (default: https://api.anthropic.com/v1)
    pub base_url: String,
    /// Anthropic API version (default: 2023-06-01)
    pub anthropic_version: String,
    /// Beta features to enable (e.g., ["prompt-caching-2024-07-31"])
    pub beta_features: Vec<String>,
}

/// Beta header for OAuth authentication
/// Required headers for Claude Pro/Max OAuth tokens to work:
/// - oauth-2025-04-20: REQUIRED - enables OAuth authentication support
/// - claude-code-20250219: Required for Claude Code product access (OAuth tokens are restricted to this)
/// - interleaved-thinking-2025-05-14: Extended thinking support
/// - fine-grained-tool-streaming-2025-05-14: Tool streaming support
pub const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20,claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14";

/// System prompt prefix required for Claude Code OAuth tokens
/// OAuth tokens from Claude Pro/Max subscriptions are restricted to "Claude Code" product.
/// This exact prefix MUST be the first system block with ephemeral cache control
/// for the API to accept requests to advanced models like Opus/Sonnet.
pub const CLAUDE_CODE_SYSTEM_PREFIX: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

impl AnthropicConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            auth: AnthropicAuth::api_key(api_key),
            base_url: "https://api.anthropic.com/v1/".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            beta_features: vec![],
        }
    }

    /// Create new config with OAuth access token
    pub fn with_oauth(access_token: impl Into<String>) -> Self {
        Self {
            auth: AnthropicAuth::oauth(access_token),
            base_url: "https://api.anthropic.com/v1/".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            beta_features: vec![OAUTH_BETA_HEADER.to_string()],
        }
    }

    /// Create new config with authentication
    pub fn with_auth(auth: AnthropicAuth) -> Self {
        let beta_features = match &auth {
            AnthropicAuth::OAuth { .. } => vec![OAUTH_BETA_HEADER.to_string()],
            AnthropicAuth::ApiKey(_) => vec![],
        };

        Self {
            auth,
            base_url: "https://api.anthropic.com/v1/".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            beta_features,
        }
    }

    /// Get API key (for backward compatibility)
    /// Returns empty string for OAuth auth
    #[deprecated(note = "Use auth field directly instead")]
    pub fn api_key(&self) -> &str {
        match &self.auth {
            AnthropicAuth::ApiKey(key) => key,
            AnthropicAuth::OAuth { .. } => "",
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

/// Anthropic messages request
#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<AnthropicMessageContent>,
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
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContent {
    Text {
        text: String,
    },
    Image {
        source: AnthropicSource,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<AnthropicMessageContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Anthropic message content (can be string or array of content blocks)
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum AnthropicMessageContent {
    String(String),
    Blocks(Vec<AnthropicContent>),
    /// System blocks with cache control support (for OAuth Claude Code prefix)
    SystemBlocks(Vec<AnthropicSystemBlock>),
}

/// System block with cache control support
/// Used for OAuth requests that need the Claude Code prefix with ephemeral caching
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

/// Cache control configuration for prompt caching
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicCacheControl {
    #[serde(rename = "type")]
    pub type_: String,
}

impl AnthropicMessageContent {
    /// Build system content based on authentication type
    ///
    /// For OAuth: Returns SystemBlocks with Claude Code prefix and cache control
    /// For API key: Returns simple String format
    pub fn build_system(auth: &AnthropicAuth, user_system: Option<String>) -> Option<Self> {
        match auth {
            AnthropicAuth::OAuth { .. } => {
                let mut blocks = vec![AnthropicSystemBlock::with_ephemeral_cache(
                    CLAUDE_CODE_SYSTEM_PREFIX,
                )];
                if let Some(system) = user_system {
                    blocks.push(AnthropicSystemBlock::with_ephemeral_cache(system));
                }
                Some(Self::SystemBlocks(blocks))
            }
            AnthropicAuth::ApiKey(_) => user_system.map(Self::String),
        }
    }
}

impl AnthropicSystemBlock {
    /// Create a new system block with ephemeral cache control
    pub fn with_ephemeral_cache(text: impl Into<String>) -> Self {
        Self {
            type_: "text".to_string(),
            text: text.into(),
            cache_control: Some(AnthropicCacheControl {
                type_: "ephemeral".to_string(),
            }),
        }
    }

    /// Create a new system block without cache control
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            type_: "text".to_string(),
            text: text.into(),
            cache_control: None,
        }
    }
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
#[derive(Debug, Deserialize, Clone)]
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
