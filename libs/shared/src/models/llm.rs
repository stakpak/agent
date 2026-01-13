use crate::models::{
    integrations::{anthropic::AnthropicModel, gemini::GeminiModel, openai::OpenAIModel},
    model_pricing::{ContextAware, ModelContextInfo},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;

// =============================================================================
// Provider Configuration
// =============================================================================

/// Unified provider configuration enum
///
/// All provider configurations are stored in a HashMap<String, ProviderConfig>
/// where the key is the provider name (e.g., "openai", "anthropic", "litellm").
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
/// [profiles.myprofile.providers.litellm]
/// type = "custom"
/// api_endpoint = "http://localhost:4000"
/// api_key = "sk-litellm"
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
    /// Custom OpenAI-compatible provider (LiteLLM, Ollama, etc.)
    ///
    /// The provider key in the config becomes the model prefix.
    /// For example, if configured as `providers.litellm`, use models as:
    /// - `litellm/claude-opus` - passes `claude-opus` to the API
    /// - `litellm/anthropic/claude-opus` - passes `anthropic/claude-opus` to the API
    ///   (useful for LiteLLM which expects provider prefixes)
    ///
    /// # Example TOML
    /// ```toml
    /// [profiles.myprofile.providers.litellm]
    /// type = "custom"
    /// api_endpoint = "http://localhost:4000"
    /// api_key = "sk-litellm"
    ///
    /// # Then use models as:
    /// smart_model = "litellm/anthropic/claude-opus"
    /// eco_model = "litellm/openai/gpt-4-turbo"
    /// ```
    Custom {
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
        /// API endpoint URL (required for custom providers)
        /// Use the base URL as required by your provider (e.g., "http://localhost:4000")
        api_endpoint: String,
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
        }
    }

    /// Get the API key if set
    pub fn api_key(&self) -> Option<&str> {
        match self {
            ProviderConfig::OpenAI { api_key, .. } => api_key.as_deref(),
            ProviderConfig::Anthropic { api_key, .. } => api_key.as_deref(),
            ProviderConfig::Gemini { api_key, .. } => api_key.as_deref(),
            ProviderConfig::Custom { api_key, .. } => api_key.as_deref(),
        }
    }

    /// Get the API endpoint if set
    pub fn api_endpoint(&self) -> Option<&str> {
        match self {
            ProviderConfig::OpenAI { api_endpoint, .. } => api_endpoint.as_deref(),
            ProviderConfig::Anthropic { api_endpoint, .. } => api_endpoint.as_deref(),
            ProviderConfig::Gemini { api_endpoint, .. } => api_endpoint.as_deref(),
            ProviderConfig::Custom { api_endpoint, .. } => Some(api_endpoint.as_str()),
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
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum LLMModel {
    Anthropic(AnthropicModel),
    Gemini(GeminiModel),
    OpenAI(OpenAIModel),
    /// Custom provider with explicit provider name and model.
    ///
    /// Used for custom OpenAI-compatible providers like LiteLLM, Ollama, etc.
    /// The provider name matches the key in the `providers` HashMap config.
    ///
    /// # Examples
    /// - `litellm/claude-opus` → `provider: "litellm"`, `model: "claude-opus"`
    /// - `litellm/anthropic/claude-opus` → `provider: "litellm"`, `model: "anthropic/claude-opus"`
    /// - `ollama/llama3` → `provider: "ollama"`, `model: "llama3"`
    Custom {
        /// Provider name matching the key in providers config (e.g., "litellm", "ollama")
        provider: String,
        /// Model name/path to pass to the provider API (can include nested prefixes)
        model: String,
    },
}

impl ContextAware for LLMModel {
    fn context_info(&self) -> ModelContextInfo {
        match self {
            LLMModel::Anthropic(model) => model.context_info(),
            LLMModel::Gemini(model) => model.context_info(),
            LLMModel::OpenAI(model) => model.context_info(),
            LLMModel::Custom { .. } => ModelContextInfo::default(),
        }
    }

    fn model_name(&self) -> String {
        match self {
            LLMModel::Anthropic(model) => model.model_name(),
            LLMModel::Gemini(model) => model.model_name(),
            LLMModel::OpenAI(model) => model.model_name(),
            LLMModel::Custom { provider, model } => format!("{}/{}", provider, model),
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

impl From<String> for LLMModel {
    /// Parse a model string into an LLMModel.
    ///
    /// # Format
    /// - `provider/model` - Explicit provider prefix
    /// - `provider/nested/model` - Provider with nested model path (e.g., for LiteLLM)
    /// - `model-name` - Auto-detect provider from model name
    ///
    /// # Examples
    /// - `"litellm/anthropic/claude-opus"` → Custom { provider: "litellm", model: "anthropic/claude-opus" }
    /// - `"anthropic/claude-opus-4-5"` → Anthropic(Claude45Opus) (built-in provider)
    /// - `"claude-opus-4-5"` → Anthropic(Claude45Opus) (auto-detected)
    /// - `"ollama/llama3"` → Custom { provider: "ollama", model: "llama3" }
    fn from(value: String) -> Self {
        // Check for explicit provider/model format (e.g., "litellm/anthropic/claude-opus")
        // split_once takes only the first segment as provider, rest is the model path
        if let Some((provider, model)) = value.split_once('/') {
            // Check if it's a known built-in provider with explicit prefix
            match provider {
                "anthropic" => return Self::from_model_name(model),
                "openai" => return Self::from_model_name(model),
                "google" | "gemini" => return Self::from_model_name(model),
                // Unknown provider = custom provider (model can contain additional slashes)
                _ => {
                    return LLMModel::Custom {
                        provider: provider.to_string(),
                        model: model.to_string(), // Preserves nested paths like "anthropic/claude-opus"
                    };
                }
            }
        }

        // Fall back to auto-detection by model name prefix
        Self::from_model_name(&value)
    }
}

impl LLMModel {
    /// Parse model name without provider prefix
    fn from_model_name(model: &str) -> Self {
        if model.starts_with("claude-haiku-4-5") {
            LLMModel::Anthropic(AnthropicModel::Claude45Haiku)
        } else if model.starts_with("claude-sonnet-4-5") {
            LLMModel::Anthropic(AnthropicModel::Claude45Sonnet)
        } else if model.starts_with("claude-opus-4-5") {
            LLMModel::Anthropic(AnthropicModel::Claude45Opus)
        } else if model == "gemini-2.5-flash-lite" {
            LLMModel::Gemini(GeminiModel::Gemini25FlashLite)
        } else if model.starts_with("gemini-2.5-flash") {
            LLMModel::Gemini(GeminiModel::Gemini25Flash)
        } else if model.starts_with("gemini-2.5-pro") {
            LLMModel::Gemini(GeminiModel::Gemini25Pro)
        } else if model.starts_with("gemini-3-pro-preview") {
            LLMModel::Gemini(GeminiModel::Gemini3Pro)
        } else if model.starts_with("gemini-3-flash-preview") {
            LLMModel::Gemini(GeminiModel::Gemini3Flash)
        } else if model.starts_with("gpt-5-mini") {
            LLMModel::OpenAI(OpenAIModel::GPT5Mini)
        } else if model.starts_with("gpt-5") {
            LLMModel::OpenAI(OpenAIModel::GPT5)
        } else {
            // Unknown model without provider prefix - treat as custom with "custom" provider
            LLMModel::Custom {
                provider: "custom".to_string(),
                model: model.to_string(),
            }
        }
    }

    /// Get the provider name for this model
    pub fn provider_name(&self) -> &str {
        match self {
            LLMModel::Anthropic(_) => "anthropic",
            LLMModel::Gemini(_) => "google",
            LLMModel::OpenAI(_) => "openai",
            LLMModel::Custom { provider, .. } => provider,
        }
    }

    /// Get just the model name without provider prefix
    pub fn model_id(&self) -> String {
        match self {
            LLMModel::Anthropic(m) => m.to_string(),
            LLMModel::Gemini(m) => m.to_string(),
            LLMModel::OpenAI(m) => m.to_string(),
            LLMModel::Custom { model, .. } => model.clone(),
        }
    }
}

impl Display for LLMModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LLMModel::Anthropic(model) => write!(f, "{}", model),
            LLMModel::Gemini(model) => write!(f, "{}", model),
            LLMModel::OpenAI(model) => write!(f, "{}", model),
            LLMModel::Custom { provider, model } => write!(f, "{}/{}", provider, model),
        }
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
    pub model: LLMModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub tools: Option<Vec<LLMTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<LLMProviderOptions>,
}

#[derive(Debug)]
pub struct LLMStreamInput {
    pub model: LLMModel,
    pub messages: Vec<LLMMessage>,
    pub max_tokens: u32,
    pub stream_channel_tx: tokio::sync::mpsc::Sender<GenerationDelta>,
    pub tools: Option<Vec<LLMTool>>,
    pub provider_options: Option<LLMProviderOptions>,
}

impl From<&LLMStreamInput> for LLMInput {
    fn from(value: &LLMStreamInput) -> Self {
        LLMInput {
            model: value.model.clone(),
            messages: value.messages.clone(),
            max_tokens: value.max_tokens,
            tools: value.tools.clone(),
            provider_options: value.provider_options.clone(),
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_model_from_known_anthropic_model() {
        let model = LLMModel::from("claude-opus-4-5-20251101".to_string());
        assert!(matches!(
            model,
            LLMModel::Anthropic(AnthropicModel::Claude45Opus)
        ));
    }

    #[test]
    fn test_llm_model_from_known_openai_model() {
        let model = LLMModel::from("gpt-5".to_string());
        assert!(matches!(model, LLMModel::OpenAI(OpenAIModel::GPT5)));
    }

    #[test]
    fn test_llm_model_from_known_gemini_model() {
        let model = LLMModel::from("gemini-2.5-flash".to_string());
        assert!(matches!(
            model,
            LLMModel::Gemini(GeminiModel::Gemini25Flash)
        ));
    }

    #[test]
    fn test_llm_model_from_custom_provider_with_slash() {
        let model = LLMModel::from("litellm/claude-opus-4-5".to_string());
        match model {
            LLMModel::Custom { provider, model } => {
                assert_eq!(provider, "litellm");
                assert_eq!(model, "claude-opus-4-5");
            }
            _ => panic!("Expected Custom model"),
        }
    }

    #[test]
    fn test_llm_model_from_ollama_provider() {
        let model = LLMModel::from("ollama/llama3".to_string());
        match model {
            LLMModel::Custom { provider, model } => {
                assert_eq!(provider, "ollama");
                assert_eq!(model, "llama3");
            }
            _ => panic!("Expected Custom model"),
        }
    }

    #[test]
    fn test_llm_model_explicit_anthropic_prefix() {
        // Explicit anthropic/ prefix should still parse to Anthropic variant
        let model = LLMModel::from("anthropic/claude-opus-4-5".to_string());
        assert!(matches!(
            model,
            LLMModel::Anthropic(AnthropicModel::Claude45Opus)
        ));
    }

    #[test]
    fn test_llm_model_explicit_openai_prefix() {
        let model = LLMModel::from("openai/gpt-5".to_string());
        assert!(matches!(model, LLMModel::OpenAI(OpenAIModel::GPT5)));
    }

    #[test]
    fn test_llm_model_explicit_google_prefix() {
        let model = LLMModel::from("google/gemini-2.5-flash".to_string());
        assert!(matches!(
            model,
            LLMModel::Gemini(GeminiModel::Gemini25Flash)
        ));
    }

    #[test]
    fn test_llm_model_explicit_gemini_prefix() {
        // gemini/ alias should also work
        let model = LLMModel::from("gemini/gemini-2.5-flash".to_string());
        assert!(matches!(
            model,
            LLMModel::Gemini(GeminiModel::Gemini25Flash)
        ));
    }

    #[test]
    fn test_llm_model_unknown_model_becomes_custom() {
        let model = LLMModel::from("some-random-model".to_string());
        match model {
            LLMModel::Custom { provider, model } => {
                assert_eq!(provider, "custom");
                assert_eq!(model, "some-random-model");
            }
            _ => panic!("Expected Custom model"),
        }
    }

    #[test]
    fn test_llm_model_display_anthropic() {
        let model = LLMModel::Anthropic(AnthropicModel::Claude45Sonnet);
        let s = model.to_string();
        assert!(s.contains("claude"));
    }

    #[test]
    fn test_llm_model_display_custom() {
        let model = LLMModel::Custom {
            provider: "litellm".to_string(),
            model: "claude-opus".to_string(),
        };
        assert_eq!(model.to_string(), "litellm/claude-opus");
    }

    #[test]
    fn test_llm_model_provider_name() {
        assert_eq!(
            LLMModel::Anthropic(AnthropicModel::Claude45Sonnet).provider_name(),
            "anthropic"
        );
        assert_eq!(
            LLMModel::OpenAI(OpenAIModel::GPT5).provider_name(),
            "openai"
        );
        assert_eq!(
            LLMModel::Gemini(GeminiModel::Gemini25Flash).provider_name(),
            "google"
        );
        assert_eq!(
            LLMModel::Custom {
                provider: "litellm".to_string(),
                model: "test".to_string()
            }
            .provider_name(),
            "litellm"
        );
    }

    #[test]
    fn test_llm_model_model_id() {
        let model = LLMModel::Custom {
            provider: "litellm".to_string(),
            model: "claude-opus-4-5".to_string(),
        };
        assert_eq!(model.model_id(), "claude-opus-4-5");
    }

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
}
