use crate::models::{
    integrations::{
        anthropic::{AnthropicConfig, AnthropicModel},
        gemini::{GeminiConfig, GeminiModel},
        openai::{OpenAIConfig, OpenAIModel},
    },
    model_pricing::{ContextAware, ModelContextInfo},
};
use serde::{Deserialize, Serialize};
use std::fmt::Display;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum LLMModel {
    Anthropic(AnthropicModel),
    Gemini(GeminiModel),
    OpenAI(OpenAIModel),
    /// Custom provider with explicit provider name and model
    /// Used for custom OpenAI-compatible providers like LiteLLM, Ollama, etc.
    Custom {
        /// Provider name (e.g., "litellm", "ollama")
        provider: String,
        /// Model name to pass to the provider
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

/// Configuration for a custom OpenAI-compatible provider
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomProviderConfig {
    /// Unique name for this provider (used in model strings like "litellm/claude-opus")
    pub name: String,
    /// API endpoint URL (e.g., "http://localhost:4000")
    pub api_endpoint: String,
    /// API key (optional, some providers don't require auth)
    pub api_key: Option<String>,
}

#[derive(Debug)]
pub struct LLMProviderConfig {
    pub anthropic_config: Option<AnthropicConfig>,
    pub gemini_config: Option<GeminiConfig>,
    pub openai_config: Option<OpenAIConfig>,
    pub custom_providers: Option<Vec<CustomProviderConfig>>,
}

impl From<String> for LLMModel {
    fn from(value: String) -> Self {
        // First check for explicit provider/model format (e.g., "litellm/claude-opus-4-5")
        if let Some((provider, model)) = value.split_once('/') {
            // Check if it's a known built-in provider with explicit prefix
            match provider {
                "anthropic" => return Self::from_model_name(model),
                "openai" => return Self::from_model_name(model),
                "google" | "gemini" => return Self::from_model_name(model),
                // Unknown provider = custom provider
                _ => {
                    return LLMModel::Custom {
                        provider: provider.to_string(),
                        model: model.to_string(),
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

    #[test]
    fn test_custom_provider_config_creation() {
        let config = CustomProviderConfig {
            name: "litellm".to_string(),
            api_endpoint: "http://localhost:4000".to_string(),
            api_key: Some("sk-1234".to_string()),
        };
        assert_eq!(config.name, "litellm");
        assert_eq!(config.api_endpoint, "http://localhost:4000");
        assert_eq!(config.api_key, Some("sk-1234".to_string()));
    }

    #[test]
    fn test_custom_provider_config_without_key() {
        let config = CustomProviderConfig {
            name: "ollama".to_string(),
            api_endpoint: "http://localhost:11434/v1".to_string(),
            api_key: None,
        };
        assert_eq!(config.name, "ollama");
        assert!(config.api_key.is_none());
    }
}
