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
        if messages.is_empty() {
            return vec![];
        }

        let mut context = Vec::new();

        // 1. Flatten history (all messages except the last one)
        if messages.len() > 1 {
            let history_content = messages[..messages.len() - 1]
                .iter()
                .map(|m| format!("{}: {}", m.role, m.content.clone().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("\n");

            context.push(LLMMessage {
                role: Role::User.to_string(),
                content: LLMMessageContent::String(history_content),
            });
        }

        // 2. Preserve the last message (with images)
        if let Some(last_message) = messages.last() {
            context.push(LLMMessage::from(last_message.clone()));
        }

        context
    }
}
