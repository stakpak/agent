//! Stakpak-specific types

use serde::{Deserialize, Serialize};

/// Configuration for Stakpak inference provider
///
/// Note: This is distinct from `stakpak_api::StakpakConfig` which is used
/// for the Stakpak API client (sessions, billing, etc.).
#[derive(Debug, Clone)]
pub struct StakpakProviderConfig {
    /// API key
    pub api_key: String,
    /// Base URL (default: https://apiv2.stakpak.dev)
    pub base_url: String,
    /// User-Agent header (e.g., "Stakpak/1.0.0")
    pub user_agent: Option<String>,
}

impl StakpakProviderConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://apiv2.stakpak.dev".to_string(),
            user_agent: None,
        }
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Set User-Agent header
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }
}

impl Default for StakpakProviderConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("STAKPAK_API_KEY").unwrap_or_else(|_| String::new()),
            base_url: "https://apiv2.stakpak.dev".to_string(),
            user_agent: None,
        }
    }
}

/// Stakpak chat completion response
#[derive(Debug, Deserialize)]
pub struct StakpakResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<StakpakChoice>,
    pub usage: StakpakUsage,
}

/// Stakpak choice
#[derive(Debug, Deserialize)]
pub struct StakpakChoice {
    pub message: StakpakMessage,
    pub finish_reason: Option<String>,
}

/// Stakpak message
#[derive(Debug, Serialize, Deserialize)]
pub struct StakpakMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<StakpakToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Stakpak tool call
#[derive(Debug, Serialize, Deserialize)]
pub struct StakpakToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: StakpakFunctionCall,
}

/// Stakpak function call
#[derive(Debug, Serialize, Deserialize)]
pub struct StakpakFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Stakpak usage statistics
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct StakpakUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<StakpakPromptTokensDetails>,
}

/// Stakpak prompt token details (includes Anthropic cache fields)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct StakpakPromptTokensDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<u32>,
}
