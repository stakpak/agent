use stakpak_shared::models::{
    integrations::openai::{ChatMessage, Role},
    llm::{LLMMessage, LLMMessageContent},
};

pub trait ContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage>;
}

pub struct SimpleContextManager;

impl ContextManager for SimpleContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        let mut context = Vec::new();
        context.push(LLMMessage {
            role: Role::User.to_string(),
            content: LLMMessageContent::String(
                messages
                    .into_iter()
                    .map(|m| format!("{}: {}", m.role, m.content.unwrap().to_string()))
                    .reduce(|a, b| a + "\n" + &b)
                    .unwrap(),
            ),
        });
        context
    }
}
