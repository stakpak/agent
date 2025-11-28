use stakpak_shared::models::{integrations::openai::ChatMessage, llm::LLMMessage};

pub trait ContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage>;
}

pub mod scratchpad_context_manager;
pub mod simple_context_manager;
