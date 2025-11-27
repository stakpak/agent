use serde::{Deserialize, Serialize};

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
