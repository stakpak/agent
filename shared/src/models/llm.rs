use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum LLMMessageTypedContent {
    #[serde(alias = "text")]
    Text { text: String },
    #[serde(alias = "tool_use")]
    ToolCall {
        id: String,
        name: String,
        #[serde(alias = "args", alias = "input")]
        args: serde_json::Value,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMTokenUsage {
    pub completion_tokens: u32,
    pub prompt_tokens: u32,
    pub total_tokens: u32,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}
