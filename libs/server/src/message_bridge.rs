use stakpak_shared::models::{
    integrations::openai::ChatMessage,
    llm::LLMMessage,
    stakai_adapter::{from_stakai_message, to_stakai_message},
};

/// Storage adapter: local/session storage currently persists ChatMessage payloads.
///
/// Runtime/API boundaries should stay StakAI-native. These adapters exist only at
/// the persistence edge until storage is migrated to `Vec<stakai::Message>`.
pub fn chat_to_stakai(messages: Vec<ChatMessage>) -> Vec<stakai::Message> {
    messages
        .into_iter()
        .map(LLMMessage::from)
        .map(|message| to_stakai_message(&message))
        .collect()
}

pub fn stakai_to_chat(messages: &[stakai::Message]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(from_stakai_message)
        .map(ChatMessage::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakai::{Message, Role};

    #[test]
    fn converts_stakai_to_chat_and_back() {
        let messages = vec![
            Message::new(Role::System, "system"),
            Message::new(Role::User, "hello"),
            Message::new(Role::Assistant, "hi"),
        ];

        let chat = stakai_to_chat(&messages);
        let back = chat_to_stakai(chat);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, Role::System);
        assert_eq!(back[1].role, Role::User);
        assert_eq!(back[2].role, Role::Assistant);
        assert_eq!(back[1].text(), Some("hello".to_string()));
    }
}
