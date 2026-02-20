use crate::error::AgentError;
use async_trait::async_trait;
use stakai::{Message, Model};

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<Message>,
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub truncated: bool,
}

#[async_trait]
pub trait CompactionEngine: Send + Sync {
    async fn compact(
        &self,
        messages: Vec<Message>,
        model: &Model,
    ) -> Result<CompactionResult, AgentError>;
}

#[derive(Debug, Default)]
pub struct PassthroughCompactionEngine;

#[async_trait]
impl CompactionEngine for PassthroughCompactionEngine {
    async fn compact(
        &self,
        messages: Vec<Message>,
        _model: &Model,
    ) -> Result<CompactionResult, AgentError> {
        let token_like_count = messages
            .iter()
            .filter_map(stakai::Message::text)
            .map(|text| text.split_whitespace().count())
            .sum();

        Ok(CompactionResult {
            messages,
            tokens_before: token_like_count,
            tokens_after: token_like_count,
            truncated: false,
        })
    }
}
