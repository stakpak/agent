//! Request types for AI generation

use super::cache::PromptCacheRetention;
use super::{GenerateOptions, Message};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request for generating AI completions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// Model identifier (can be provider-prefixed like "openai/gpt-4")
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

    /// Custom telemetry metadata to be recorded on the span.
    ///
    /// When the `tracing` feature is enabled, these key-value pairs are
    /// recorded as span attributes with the `metadata.` prefix.
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::GenerateRequest;
    /// use std::collections::HashMap;
    ///
    /// let mut metadata = HashMap::new();
    /// metadata.insert("user.id".to_string(), "user-123".to_string());
    /// metadata.insert("session.id".to_string(), "session-456".to_string());
    ///
    /// let request = GenerateRequest {
    ///     telemetry_metadata: Some(metadata),
    ///     ..Default::default()
    /// };
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry_metadata: Option<HashMap<String, String>>,
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

    /// Controls how system messages are handled.
    /// - `system`: Pass as system-level instruction (default for non-reasoning models)
    /// - `developer`: Convert to developer message (default for reasoning models like o1/o3/o4)
    /// - `remove`: Remove system messages from the request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message_mode: Option<SystemMessageMode>,

    /// Whether to store the generation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    /// User identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Manual prompt cache key for better cache hit rates.
    ///
    /// OpenAI automatically caches prompts >= 1024 tokens. Use this to
    /// provide a stable identifier that helps optimize cache hits.
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::{OpenAIOptions, ProviderOptions};
    ///
    /// let opts = OpenAIOptions {
    ///     prompt_cache_key: Some("my-session-123".into()),
    ///     ..Default::default()
    /// };
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,

    /// Cache retention policy for prompt caching.
    ///
    /// - `InMemory` (default): Standard caching (~5-60 min depending on load)
    /// - `Extended24h`: Extended 24-hour caching (GPT-5.1+ only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<PromptCacheRetention>,
}

/// Controls how system messages are handled in OpenAI requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemMessageMode {
    /// Pass the system message as a system-level instruction
    System,
    /// Convert to developer message (for reasoning models)
    Developer,
    /// Remove the system message from the request
    Remove,
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

    /// Name of cached content to use (format: cachedContents/{id})
    ///
    /// Use Google's `GoogleAICacheManager` to create cached content,
    /// then reference it by name here.
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::{GoogleOptions, ProviderOptions};
    ///
    /// let opts = GoogleOptions {
    ///     cached_content: Some("cachedContents/abc123".into()),
    ///     ..Default::default()
    /// };
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content: Option<String>,
}

impl GenerateRequest {
    /// Create a new request with model and messages
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            options: GenerateOptions::default(),
            provider_options: None,
            telemetry_metadata: None,
        }
    }

    /// Set provider options
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Set telemetry metadata
    ///
    /// These key-value pairs will be recorded on the tracing span when
    /// the `tracing` feature is enabled.
    pub fn with_telemetry_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.telemetry_metadata = Some(metadata);
        self
    }
}
