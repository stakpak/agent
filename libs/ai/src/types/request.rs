//! Request types for AI generation

use super::{GenerateOptions, Message};
use serde::{Deserialize, Serialize};

/// Request for generating AI completions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// Model identifier (can be provider-prefixed like "openai:gpt-4")
    #[serde(skip)]
    pub model: String,

    /// Conversation messages
    pub messages: Vec<Message>,

    /// Generation options (temperature, max_tokens, etc.)
    #[serde(flatten)]
    pub options: GenerateOptions,

    /// Provider-specific options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// Provider-specific options enum
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum ProviderOptions {
    /// Anthropic-specific options
    Anthropic(AnthropicOptions),

    /// OpenAI-specific options
    OpenAI(OpenAIOptions),

    /// Google/Gemini-specific options
    Google(GoogleOptions),
}

/// Anthropic-specific provider options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicOptions {
    /// Extended thinking configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingOptions>,

    /// Effort level for thinking (high, medium, low) - for Claude Opus 4.5
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
}

/// Anthropic thinking/extended reasoning options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingOptions {
    /// Budget tokens for thinking (must be >= 1024)
    pub budget_tokens: u32,
}

impl ThinkingOptions {
    /// Create thinking config with budget
    pub fn new(budget_tokens: u32) -> Self {
        Self {
            budget_tokens: budget_tokens.max(1024),
        }
    }
}

/// OpenAI-specific provider options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIOptions {
    /// Reasoning effort for o1/o3/o4 models
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,

    /// Reasoning summary mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<ReasoningSummary>,

    /// Whether to store the generation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    /// User identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// Reasoning effort level (shared across providers)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

/// Reasoning summary mode (OpenAI-specific)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSummary {
    Auto,
    Detailed,
}

/// Google/Gemini-specific provider options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoogleOptions {
    /// Thinking budget in tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
}

impl GenerateRequest {
    /// Create a new request with model and messages
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            options: GenerateOptions::default(),
            provider_options: None,
        }
    }

    /// Set provider options
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }
}
