use stakpak_shared::models::{integrations::openai::ChatMessage, llm::LLMMessage};

pub trait ContextManager: Send + Sync {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage>;
}

pub mod file_scratchpad_context_manager;
pub mod scratchpad_context_manager;
pub mod simple_context_manager;
pub mod task_board_context_manager;
