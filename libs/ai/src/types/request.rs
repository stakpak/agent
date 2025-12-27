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
}

impl GenerateRequest {
    /// Create a new request with model and messages
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            options: GenerateOptions::default(),
        }
    }
}
