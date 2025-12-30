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
    Custom(String),
}

impl ContextAware for LLMModel {
    fn context_info(&self) -> ModelContextInfo {
        match self {
            LLMModel::Anthropic(model) => model.context_info(),
            LLMModel::Gemini(model) => model.context_info(),
            LLMModel::OpenAI(model) => model.context_info(),
            LLMModel::Custom(_) => ModelContextInfo::default(),
        }
    }

    fn model_name(&self) -> String {
        match self {
            LLMModel::Anthropic(model) => model.model_name(),
            LLMModel::Gemini(model) => model.model_name(),
            LLMModel::OpenAI(model) => model.model_name(),
            LLMModel::Custom(model_name) => model_name.clone(),
        }
    }
}

#[derive(Debug)]
pub struct LLMProviderConfig {
    pub anthropic_config: Option<AnthropicConfig>,
    pub gemini_config: Option<GeminiConfig>,
    pub openai_config: Option<OpenAIConfig>,
}

impl From<String> for LLMModel {
    fn from(value: String) -> Self {
        if value.starts_with("claude-haiku-4-5") {
            LLMModel::Anthropic(AnthropicModel::Claude45Haiku)
        } else if value.starts_with("claude-sonnet-4-5") {
            LLMModel::Anthropic(AnthropicModel::Claude45Sonnet)
        } else if value.starts_with("claude-opus-4-5") {
            LLMModel::Anthropic(AnthropicModel::Claude45Opus)
        } else if value == "gemini-2.5-flash-lite" {
            LLMModel::Gemini(GeminiModel::Gemini25FlashLite)
        } else if value.starts_with("gemini-2.5-flash") {
            LLMModel::Gemini(GeminiModel::Gemini25Flash)
        } else if value.starts_with("gemini-2.5-pro") {
            LLMModel::Gemini(GeminiModel::Gemini25Pro)
        } else if value.starts_with("gemini-3-pro-preview") {
            LLMModel::Gemini(GeminiModel::Gemini3Pro)
        } else if value.starts_with("gemini-3-flash-preview") {
            LLMModel::Gemini(GeminiModel::Gemini3Flash)
        } else if value.starts_with("gpt-5-mini") {
            LLMModel::OpenAI(OpenAIModel::GPT5Mini)
        } else if value.starts_with("gpt-5") {
            LLMModel::OpenAI(OpenAIModel::GPT5)
        } else {
            LLMModel::Custom(value)
        }
    }
}

impl Display for LLMModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LLMModel::Anthropic(model) => write!(f, "{}", model),
            LLMModel::Gemini(model) => write!(f, "{}", model),
            LLMModel::OpenAI(model) => write!(f, "{}", model),
            LLMModel::Custom(model) => write!(f, "{}", model),
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
