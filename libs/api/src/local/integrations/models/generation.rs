use serde::{Deserialize, Serialize};
use stakpak_shared::models::llm::LLMTokenUsage;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Prompt {
    pub content: String,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModifiedBlockOrigin {
    pub block_id: Uuid,
    pub block_uri: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EditInfo {
    pub reasoning: String,
    pub document_uri: String,
    pub old_str: String,
    pub new_str: String,
}
