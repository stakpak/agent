//! LLM Provider and Model Configuration
//!
//! This module provides the configuration types for LLM providers and models.
//!
//! # Provider Configuration
//!
//! Providers are configured in a `providers` HashMap where the key becomes the
//! model prefix for routing requests to the correct provider.
//!
//! ## Built-in Providers
//!
//! - `openai` - OpenAI API
//! - `anthropic` - Anthropic API (supports OAuth via `access_token`)
//! - `gemini` - Google Gemini API
//! - `bedrock` - AWS Bedrock (uses AWS credential chain, no API key)
//!
//! For built-in providers, you can use the model name directly without a prefix:
//! - `claude-sonnet-4-5` → auto-detected as Anthropic
//! - `gpt-4` → auto-detected as OpenAI
//! - `gemini-2.5-pro` → auto-detected as Gemini
//!
//! ## Custom Providers
//!
//! Any OpenAI-compatible API can be configured using `type = "custom"`.
//! The provider key becomes the model prefix.
//!
//! # Model Routing
//!
//! Models can be specified with or without a provider prefix:
//!
//! - `claude-sonnet-4-5` → auto-detected as `anthropic` provider
//! - `anthropic/claude-sonnet-4-5` → explicit `anthropic` provider
//! - `offline/llama3` → routes to `offline` custom provider, sends `llama3` to API
//! - `custom/anthropic/claude-opus` → routes to `custom` provider,
//!   sends `anthropic/claude-opus` to the API
//!
//! # Example Configuration
//!
//! ```toml
//! [profiles.default]
//! provider = "local"
//! smart_model = "claude-sonnet-4-5"  # auto-detected as anthropic
//! eco_model = "offline/llama3"       # custom provider
//!
//! [profiles.default.providers.anthropic]
//! type = "anthropic"
//! # api_key from auth.toml or ANTHROPIC_API_KEY env var
//!
//! [profiles.default.providers.offline]
//! type = "custom"
//! api_endpoint = "http://localhost:11434/v1"
//! ```

use serde::{Deserialize, Serialize};
use stakai::Model;
use std::collections::HashMap;

// =============================================================================
// Provider Configuration
// =============================================================================

/// Unified provider configuration enum
///
/// All provider configurations are stored in a `HashMap<String, ProviderConfig>`
/// where the key is the provider name and becomes the model prefix for routing.
///
/// # Provider Key = Model Prefix
///
/// The key used in the HashMap becomes the prefix used in model names:
/// - Config key: `providers.offline`
/// - Model usage: `offline/llama3`
/// - Routing: finds `offline` provider, sends `llama3` to API
///
/// # Example TOML
/// ```toml
/// [profiles.myprofile.providers.openai]
/// type = "openai"
/// api_key = "sk-..."
///
/// [profiles.myprofile.providers.anthropic]
/// type = "anthropic"
/// api_key = "sk-ant-..."
/// access_token = "oauth-token"
///
/// [profiles.myprofile.providers.offline]
/// type = "custom"
/// api_endpoint = "http://localhost:11434/v1"
/// ```
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProviderConfig {
    /// OpenAI provider configuration
    OpenAI {
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_endpoint: Option<String>,
    },
    /// Anthropic provider configuration
    Anthropic {
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_endpoint: Option<String>,
        /// OAuth access token (for Claude subscription)
        #[serde(skip_serializing_if = "Option::is_none")]
        access_token: Option<String>,
    },
    /// Google Gemini provider configuration
    Gemini {
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_endpoint: Option<String>,
    },
    /// Custom OpenAI-compatible provider (Ollama, vLLM, etc.)
    ///
    /// The provider key in the config becomes the model prefix.
    /// For example, if configured as `providers.offline`, use models as:
    /// - `offline/llama3` - passes `llama3` to the API
    /// - `offline/anthropic/claude-opus` - passes `anthropic/claude-opus` to the API
    ///
    /// # Example TOML
    /// ```toml
    /// [profiles.myprofile.providers.offline]
    /// type = "custom"
    /// api_endpoint = "http://localhost:11434/v1"
    ///
    /// # Then use models as:
    /// smart_model = "offline/llama3"
    /// eco_model = "offline/phi3"
    /// ```
    Custom {
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        /// API endpoint URL (required for custom providers)
        /// Use the base URL as required by your provider (e.g., "http://localhost:11434/v1")
        api_endpoint: String,
    },
    /// Stakpak provider configuration
    ///
    /// Routes inference through Stakpak's unified API, which provides:
    /// - Access to multiple LLM providers via a single endpoint
    /// - Usage tracking and billing
    /// - Session management and checkpoints
    ///
    /// # Example TOML
    /// ```toml
    /// [profiles.myprofile.providers.stakpak]
    /// type = "stakpak"
    /// api_key = "your-stakpak-api-key"
    /// api_endpoint = "https://apiv2.stakpak.dev"  # optional, this is the default
    ///
    /// # Then use models as:
    /// smart_model = "stakpak/anthropic/claude-sonnet-4-5-20250929"
    /// ```
    Stakpak {
        /// Stakpak API key (required)
        api_key: String,
        /// API endpoint URL (default: https://apiv2.stakpak.dev)
        #[serde(skip_serializing_if = "Option::is_none")]
        api_endpoint: Option<String>,
    },
    /// AWS Bedrock provider configuration
    ///
    /// Uses AWS credential chain for authentication (no API key needed).
    /// Supports env vars, shared credentials, SSO, and instance roles.
    ///
    /// # Example TOML
    /// ```toml
    /// [profiles.myprofile.providers.amazon-bedrock]
    /// type = "amazon-bedrock"
    /// region = "us-east-1"
    /// profile_name = "my-aws-profile"  # optional
    ///
    /// # Then use models as (friendly aliases work):
    /// model = "amazon-bedrock/claude-sonnet-4-5"
    /// ```
    #[serde(rename = "amazon-bedrock")]
    Bedrock {
        /// AWS region (e.g., "us-east-1")
        region: String,
        /// Optional AWS named profile (from ~/.aws/config)
        #[serde(skip_serializing_if = "Option::is_none")]
        profile_name: Option<String>,
    },
}

impl ProviderConfig {
    /// Get the provider type name
    pub fn provider_type(&self) -> &'static str {
        match self {
            ProviderConfig::OpenAI { .. } => "openai",
            ProviderConfig::Anthropic { .. } => "anthropic",
            ProviderConfig::Gemini { .. } => "gemini",
            ProviderConfig::Custom { .. } => "custom",
            ProviderConfig::Stakpak { .. } => "stakpak",
            ProviderConfig::Bedrock { .. } => "amazon-bedrock",
        }
    }

    /// Get the API key if set
    pub fn api_key(&self) -> Option<&str> {
        match self {
            ProviderConfig::OpenAI { api_key, .. } => api_key.as_deref(),
            ProviderConfig::Anthropic { api_key, .. } => api_key.as_deref(),
            ProviderConfig::Gemini { api_key, .. } => api_key.as_deref(),
            ProviderConfig::Custom { api_key, .. } => api_key.as_deref(),
            ProviderConfig::Stakpak { api_key, .. } => Some(api_key.as_str()),
            ProviderConfig::Bedrock { .. } => None, // AWS credential chain, no API key
        }
    }

    /// Get the API endpoint if set
    pub fn api_endpoint(&self) -> Option<&str> {
        match self {
            ProviderConfig::OpenAI { api_endpoint, .. } => api_endpoint.as_deref(),
            ProviderConfig::Anthropic { api_endpoint, .. } => api_endpoint.as_deref(),
            ProviderConfig::Gemini { api_endpoint, .. } => api_endpoint.as_deref(),
            ProviderConfig::Custom { api_endpoint, .. } => Some(api_endpoint.as_str()),
            ProviderConfig::Stakpak { api_endpoint, .. } => api_endpoint.as_deref(),
            ProviderConfig::Bedrock { .. } => None, // No custom endpoint in config
        }
    }

    /// Get the access token (Anthropic only)
    pub fn access_token(&self) -> Option<&str> {
        match self {
            ProviderConfig::Anthropic { access_token, .. } => access_token.as_deref(),
            _ => None,
        }
    }

    /// Create an OpenAI provider config
    pub fn openai(api_key: Option<String>) -> Self {
        ProviderConfig::OpenAI {
            api_key,
            api_endpoint: None,
        }
    }

    /// Create an Anthropic provider config
    pub fn anthropic(api_key: Option<String>, access_token: Option<String>) -> Self {
        ProviderConfig::Anthropic {
            api_key,
            api_endpoint: None,
            access_token,
        }
    }

    /// Create a Gemini provider config
    pub fn gemini(api_key: Option<String>) -> Self {
        ProviderConfig::Gemini {
            api_key,
            api_endpoint: None,
        }
    }

    /// Create a custom provider config
    pub fn custom(api_endpoint: String, api_key: Option<String>) -> Self {
        ProviderConfig::Custom {
            api_key,
            api_endpoint,
        }
    }

    /// Create a Stakpak provider config
    pub fn stakpak(api_key: String, api_endpoint: Option<String>) -> Self {
        ProviderConfig::Stakpak {
            api_key,
            api_endpoint,
        }
    }

    /// Create a Bedrock provider config
    pub fn bedrock(region: String, profile_name: Option<String>) -> Self {
        ProviderConfig::Bedrock {
            region,
            profile_name,
        }
    }

    /// Get the AWS region (Bedrock only)
    pub fn region(&self) -> Option<&str> {
        match self {
            ProviderConfig::Bedrock { region, .. } => Some(region.as_str()),
            _ => None,
        }
    }

    /// Get the AWS profile name (Bedrock only)
    pub fn profile_name(&self) -> Option<&str> {
        match self {
            ProviderConfig::Bedrock { profile_name, .. } => profile_name.as_deref(),
            _ => None,
        }
    }
}

/// Aggregated provider configuration for LLM operations
///
/// This struct holds all configured providers, keyed by provider name.
#[derive(Debug, Clone, Default)]
pub struct LLMProviderConfig {
    /// All provider configurations (key = provider name)
    pub providers: HashMap<String, ProviderConfig>,
}

impl LLMProviderConfig {
    /// Create a new empty provider config
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Add a provider configuration
    pub fn add_provider(&mut self, name: impl Into<String>, config: ProviderConfig) {
        self.providers.insert(name.into(), config);
    }

    /// Get a provider configuration by name
    pub fn get_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    /// Check if any providers are configured
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

/// Provider-specific options for LLM requests
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LLMProviderOptions {
    /// Anthropic-specific options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<LLMAnthropicOptions>,

    /// OpenAI-specific options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<LLMOpenAIOptions>,

    /// Google/Gemini-specific options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google: Option<LLMGoogleOptions>,
}

/// Anthropic-specific options
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LLMAnthropicOptions {
    /// Extended thinking configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<LLMThinkingOptions>,
}

/// Thinking/reasoning options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LLMThinkingOptions {
    /// Budget tokens for thinking (must be >= 1024)
    pub budget_tokens: u32,
}

impl LLMThinkingOptions {
    pub fn new(budget_tokens: u32) -> Self {
        Self {
            budget_tokens: budget_tokens.max(1024),
        }
    }
}

/// OpenAI-specific options
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LLMOpenAIOptions {
    /// Reasoning effort for o1/o3/o4 models ("low", "medium", "high")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

/// Google/Gemini-specific options
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LLMGoogleOptions {
    /// Thinking budget in tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LLMInput {
    pub model: Model,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub tools: Option<Vec<LLMTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<LLMProviderOptions>,
    /// Custom headers to pass to the inference provider
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug)]
pub struct LLMStreamInput {
    pub model: Model,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
    pub tools: Option<Vec<LLMTool>>,
    pub provider_options: Option<LLMProviderOptions>,
    /// Custom headers to pass to the inference provider
    pub headers: Option<std::collections::HashMap<String, String>>,
}

impl From<&LLMStreamInput> for LLMInput {
    fn from(value: &LLMStreamInput) -> Self {
        LLMInput {
            model: value.model.clone(),
            messages: value.messages.clone(),
            max_tokens: value.max_tokens,
            tools: value.tools.clone(),
            provider_options: value.provider_options.clone(),
            headers: value.headers.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct LLMMessage {
    pub role: String,
    pub content: LLMMessageContent,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SimpleLLMMessage {
    #[serde(rename = "role")]
    pub role: SimpleLLMRole,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum SimpleLLMRole {
    User,
    Assistant,
}

impl std::fmt::Display for SimpleLLMRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimpleLLMRole::User => write!(f, "user"),
            SimpleLLMRole::Assistant => write!(f, "assistant"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum LLMMessageContent {
    String(String),
    List(Vec<LLMMessageTypedContent>),
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for LLMMessageContent {
    fn to_string(&self) -> String {
        match self {
            LLMMessageContent::String(s) => s.clone(),
            LLMMessageContent::List(l) => l
                .iter()
                .map(|c| match c {
                    LLMMessageTypedContent::Text { text } => text.clone(),
                    LLMMessageTypedContent::ToolCall { .. } => String::new(),
                    LLMMessageTypedContent::ToolResult { content, .. } => content.clone(),
                    LLMMessageTypedContent::Image { .. } => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

impl From<String> for LLMMessageContent {
    fn from(value: String) -> Self {
        LLMMessageContent::String(value)
    }
}

impl Default for LLMMessageContent {
    fn default() -> Self {
        LLMMessageContent::String(String::new())
    }
}

impl LLMMessageContent {
    /// Convert into a Vec of typed content parts.
    /// A `String` variant is returned as a single `Text` part (empty strings yield an empty vec).
    pub fn into_parts(self) -> Vec<LLMMessageTypedContent> {
        match self {
            LLMMessageContent::List(parts) => parts,
            LLMMessageContent::String(s) if s.is_empty() => vec![],
            LLMMessageContent::String(s) => vec![LLMMessageTypedContent::Text { text: s }],
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum LLMMessageTypedContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolCall {
        id: String,
        name: String,
        #[serde(alias = "input")]
        args: serde_json::Value,
        /// Opaque provider-specific metadata (e.g., Gemini thought_signature).
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    #[serde(rename = "image")]
    Image { source: LLMMessageImageSource },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMMessageImageSource {
    #[serde(rename = "type")]
    pub r#type: String,
    pub media_type: String,
    pub data: String,
}

impl Default for LLMMessageTypedContent {
    fn default() -> Self {
        LLMMessageTypedContent::Text {
            text: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMChoice {
    pub finish_reason: Option<String>,
    pub index: u32,
    pub message: LLMMessage,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMCompletionResponse {
    pub model: String,
    pub object: String,
    pub choices: Vec<LLMChoice>,
    pub created: u64,
    pub usage: Option<LLMTokenUsage>,
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMStreamDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMStreamChoice {
    pub finish_reason: Option<String>,
    pub index: u32,
    pub message: Option<LLMMessage>,
    pub delta: LLMStreamDelta,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMCompletionStreamResponse {
    pub model: String,
    pub object: String,
    pub choices: Vec<LLMStreamChoice>,
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LLMTokenUsage>,
    pub id: String,
    pub citations: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LLMTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LLMTokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    InputTokens,
    OutputTokens,
    CacheReadInputTokens,
    CacheWriteInputTokens,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct PromptTokensDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<u32>,
}

impl PromptTokensDetails {
    /// Returns an iterator over the token types and their values
    pub fn iter(&self) -> impl Iterator<Item = (TokenType, u32)> {
        [
            (TokenType::InputTokens, self.input_tokens.unwrap_or(0)),
            (TokenType::OutputTokens, self.output_tokens.unwrap_or(0)),
            (
                TokenType::CacheReadInputTokens,
                self.cache_read_input_tokens.unwrap_or(0),
            ),
            (
                TokenType::CacheWriteInputTokens,
                self.cache_write_input_tokens.unwrap_or(0),
            ),
        ]
        .into_iter()
    }
}

impl std::ops::Add for PromptTokensDetails {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            input_tokens: Some(self.input_tokens.unwrap_or(0) + rhs.input_tokens.unwrap_or(0)),
            output_tokens: Some(self.output_tokens.unwrap_or(0) + rhs.output_tokens.unwrap_or(0)),
            cache_read_input_tokens: Some(
                self.cache_read_input_tokens.unwrap_or(0)
                    + rhs.cache_read_input_tokens.unwrap_or(0),
            ),
            cache_write_input_tokens: Some(
                self.cache_write_input_tokens.unwrap_or(0)
                    + rhs.cache_write_input_tokens.unwrap_or(0),
            ),
        }
    }
}

impl std::ops::AddAssign for PromptTokensDetails {
    fn add_assign(&mut self, rhs: Self) {
        self.input_tokens = Some(self.input_tokens.unwrap_or(0) + rhs.input_tokens.unwrap_or(0));
        self.output_tokens = Some(self.output_tokens.unwrap_or(0) + rhs.output_tokens.unwrap_or(0));
        self.cache_read_input_tokens = Some(
            self.cache_read_input_tokens.unwrap_or(0) + rhs.cache_read_input_tokens.unwrap_or(0),
        );
        self.cache_write_input_tokens = Some(
            self.cache_write_input_tokens.unwrap_or(0) + rhs.cache_write_input_tokens.unwrap_or(0),
        );
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum GenerationDelta {
    Content { content: String },
    Thinking { thinking: String },
    ToolUse { tool_use: GenerationDeltaToolUse },
    Usage { usage: LLMTokenUsage },
    Metadata { metadata: serde_json::Value },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenerationDeltaToolUse {
    pub id: Option<String>,
    pub name: Option<String>,
    pub input: Option<String>,
    pub index: usize,
    /// Opaque provider-specific metadata (e.g., Gemini thought_signature)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // ProviderConfig Tests
    // =========================================================================

    #[test]
    fn test_provider_config_openai_serialization() {
        let config = ProviderConfig::OpenAI {
            api_key: Some("sk-test".to_string()),
            api_endpoint: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"type\":\"openai\""));
        assert!(json.contains("\"api_key\":\"sk-test\""));
        assert!(!json.contains("api_endpoint")); // Should be skipped when None
    }

    #[test]
    fn test_provider_config_openai_with_endpoint() {
        let config = ProviderConfig::OpenAI {
            api_key: Some("sk-test".to_string()),
            api_endpoint: Some("https://custom.openai.com/v1".to_string()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"api_endpoint\":\"https://custom.openai.com/v1\""));
    }

    #[test]
    fn test_provider_config_anthropic_serialization() {
        let config = ProviderConfig::Anthropic {
            api_key: Some("sk-ant-test".to_string()),
            api_endpoint: None,
            access_token: Some("oauth-token".to_string()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"type\":\"anthropic\""));
        assert!(json.contains("\"api_key\":\"sk-ant-test\""));
        assert!(json.contains("\"access_token\":\"oauth-token\""));
    }

    #[test]
    fn test_provider_config_gemini_serialization() {
        let config = ProviderConfig::Gemini {
            api_key: Some("gemini-key".to_string()),
            api_endpoint: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"type\":\"gemini\""));
        assert!(json.contains("\"api_key\":\"gemini-key\""));
    }

    #[test]
    fn test_provider_config_custom_serialization() {
        let config = ProviderConfig::Custom {
            api_key: Some("sk-custom".to_string()),
            api_endpoint: "http://localhost:4000".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"type\":\"custom\""));
        assert!(json.contains("\"api_endpoint\":\"http://localhost:4000\""));
        assert!(json.contains("\"api_key\":\"sk-custom\""));
    }

    #[test]
    fn test_provider_config_custom_without_key() {
        let config = ProviderConfig::Custom {
            api_key: None,
            api_endpoint: "http://localhost:11434/v1".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"type\":\"custom\""));
        assert!(json.contains("\"api_endpoint\""));
        assert!(!json.contains("api_key")); // Should be skipped when None
    }

    #[test]
    fn test_provider_config_deserialization_openai() {
        let json = r#"{"type":"openai","api_key":"sk-test"}"#;
        let config: ProviderConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, ProviderConfig::OpenAI { .. }));
        assert_eq!(config.api_key(), Some("sk-test"));
    }

    #[test]
    fn test_provider_config_deserialization_anthropic() {
        let json = r#"{"type":"anthropic","api_key":"sk-ant","access_token":"oauth"}"#;
        let config: ProviderConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, ProviderConfig::Anthropic { .. }));
        assert_eq!(config.api_key(), Some("sk-ant"));
        assert_eq!(config.access_token(), Some("oauth"));
    }

    #[test]
    fn test_provider_config_deserialization_gemini() {
        let json = r#"{"type":"gemini","api_key":"gemini-key"}"#;
        let config: ProviderConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, ProviderConfig::Gemini { .. }));
        assert_eq!(config.api_key(), Some("gemini-key"));
    }

    #[test]
    fn test_provider_config_deserialization_custom() {
        let json =
            r#"{"type":"custom","api_endpoint":"http://localhost:4000","api_key":"sk-custom"}"#;
        let config: ProviderConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, ProviderConfig::Custom { .. }));
        assert_eq!(config.api_key(), Some("sk-custom"));
        assert_eq!(config.api_endpoint(), Some("http://localhost:4000"));
    }

    #[test]
    fn test_provider_config_helper_methods() {
        let openai = ProviderConfig::openai(Some("sk-openai".to_string()));
        assert_eq!(openai.provider_type(), "openai");
        assert_eq!(openai.api_key(), Some("sk-openai"));

        let anthropic =
            ProviderConfig::anthropic(Some("sk-ant".to_string()), Some("oauth".to_string()));
        assert_eq!(anthropic.provider_type(), "anthropic");
        assert_eq!(anthropic.access_token(), Some("oauth"));

        let gemini = ProviderConfig::gemini(Some("gemini-key".to_string()));
        assert_eq!(gemini.provider_type(), "gemini");

        let custom = ProviderConfig::custom(
            "http://localhost:4000".to_string(),
            Some("sk-custom".to_string()),
        );
        assert_eq!(custom.provider_type(), "custom");
        assert_eq!(custom.api_endpoint(), Some("http://localhost:4000"));
    }

    #[test]
    fn test_llm_provider_config_new() {
        let config = LLMProviderConfig::new();
        assert!(config.is_empty());
    }

    #[test]
    fn test_llm_provider_config_add_and_get() {
        let mut config = LLMProviderConfig::new();
        config.add_provider(
            "openai",
            ProviderConfig::openai(Some("sk-test".to_string())),
        );
        config.add_provider(
            "anthropic",
            ProviderConfig::anthropic(Some("sk-ant".to_string()), None),
        );

        assert!(!config.is_empty());
        assert!(config.get_provider("openai").is_some());
        assert!(config.get_provider("anthropic").is_some());
        assert!(config.get_provider("unknown").is_none());
    }

    #[test]
    fn test_provider_config_toml_parsing() {
        // Test parsing a HashMap of providers from TOML-like JSON
        let json = r#"{
            "openai": {"type": "openai", "api_key": "sk-openai"},
            "anthropic": {"type": "anthropic", "api_key": "sk-ant", "access_token": "oauth"},
            "litellm": {"type": "custom", "api_endpoint": "http://localhost:4000", "api_key": "sk-litellm"}
        }"#;

        let providers: HashMap<String, ProviderConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(providers.len(), 3);

        assert!(matches!(
            providers.get("openai"),
            Some(ProviderConfig::OpenAI { .. })
        ));
        assert!(matches!(
            providers.get("anthropic"),
            Some(ProviderConfig::Anthropic { .. })
        ));
        assert!(matches!(
            providers.get("litellm"),
            Some(ProviderConfig::Custom { .. })
        ));
    }

    // =========================================================================
    // Bedrock ProviderConfig Tests
    // =========================================================================

    #[test]
    fn test_provider_config_bedrock_serialization() {
        let config = ProviderConfig::Bedrock {
            region: "us-east-1".to_string(),
            profile_name: Some("my-profile".to_string()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"type\":\"amazon-bedrock\""));
        assert!(json.contains("\"region\":\"us-east-1\""));
        assert!(json.contains("\"profile_name\":\"my-profile\""));
    }

    #[test]
    fn test_provider_config_bedrock_serialization_without_profile() {
        let config = ProviderConfig::Bedrock {
            region: "us-west-2".to_string(),
            profile_name: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"type\":\"amazon-bedrock\""));
        assert!(json.contains("\"region\":\"us-west-2\""));
        assert!(!json.contains("profile_name")); // Should be skipped when None
    }

    #[test]
    fn test_provider_config_bedrock_deserialization() {
        let json = r#"{"type":"amazon-bedrock","region":"us-east-1","profile_name":"prod"}"#;
        let config: ProviderConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, ProviderConfig::Bedrock { .. }));
        assert_eq!(config.region(), Some("us-east-1"));
        assert_eq!(config.profile_name(), Some("prod"));
    }

    #[test]
    fn test_provider_config_bedrock_deserialization_minimal() {
        let json = r#"{"type":"amazon-bedrock","region":"eu-west-1"}"#;
        let config: ProviderConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, ProviderConfig::Bedrock { .. }));
        assert_eq!(config.region(), Some("eu-west-1"));
        assert_eq!(config.profile_name(), None);
    }

    #[test]
    fn test_provider_config_bedrock_no_api_key() {
        let config = ProviderConfig::bedrock("us-east-1".to_string(), None);
        assert_eq!(config.api_key(), None); // Bedrock uses AWS credential chain
        assert_eq!(config.api_endpoint(), None); // No custom endpoint
    }

    #[test]
    fn test_provider_config_bedrock_helper_methods() {
        let bedrock = ProviderConfig::bedrock("us-east-1".to_string(), Some("prod".to_string()));
        assert_eq!(bedrock.provider_type(), "amazon-bedrock");
        assert_eq!(bedrock.region(), Some("us-east-1"));
        assert_eq!(bedrock.profile_name(), Some("prod"));
        assert_eq!(bedrock.api_key(), None);
        assert_eq!(bedrock.api_endpoint(), None);
        assert_eq!(bedrock.access_token(), None);
    }

    #[test]
    fn test_provider_config_bedrock_toml_roundtrip() {
        let config = ProviderConfig::Bedrock {
            region: "us-east-1".to_string(),
            profile_name: Some("my-profile".to_string()),
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: ProviderConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn test_provider_config_bedrock_toml_parsing() {
        let toml_str = r#"
            type = "amazon-bedrock"
            region = "us-east-1"
            profile_name = "production"
        "#;
        let config: ProviderConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            config,
            ProviderConfig::Bedrock {
                ref region,
                ref profile_name,
            } if region == "us-east-1" && profile_name.as_deref() == Some("production")
        ));
    }

    #[test]
    fn test_provider_config_bedrock_missing_region_fails() {
        let json = r#"{"type":"amazon-bedrock"}"#;
        let result: Result<ProviderConfig, _> = serde_json::from_str(json);
        assert!(result.is_err()); // region is required
    }

    #[test]
    fn test_provider_config_bedrock_in_providers_map() {
        let json = r#"{
            "anthropic": {"type": "anthropic", "api_key": "sk-ant"},
            "amazon-bedrock": {"type": "amazon-bedrock", "region": "us-east-1"}
        }"#;
        let providers: HashMap<String, ProviderConfig> = serde_json::from_str(json).unwrap();
        assert_eq!(providers.len(), 2);
        assert!(matches!(
            providers.get("amazon-bedrock"),
            Some(ProviderConfig::Bedrock { .. })
        ));
    }

    #[test]
    fn test_region_returns_none_for_non_bedrock() {
        let openai = ProviderConfig::openai(Some("key".to_string()));
        assert_eq!(openai.region(), None);

        let anthropic = ProviderConfig::anthropic(Some("key".to_string()), None);
        assert_eq!(anthropic.region(), None);
    }

    #[test]
    fn test_profile_name_returns_none_for_non_bedrock() {
        let openai = ProviderConfig::openai(Some("key".to_string()));
        assert_eq!(openai.profile_name(), None);
    }
}
