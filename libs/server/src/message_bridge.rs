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
    use serde_json::json;
    use stakai::{ContentPart, Message, MessageContent, Role};

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

    #[test]
    fn preserves_tool_result_tool_call_id_through_storage_adapter() {
        let messages = vec![Message::new(
            Role::Tool,
            MessageContent::Parts(vec![ContentPart::tool_result(
                "toolu_01Abc123".to_string(),
                json!("result payload"),
            )]),
        )];

        let chat = stakai_to_chat(&messages);
        assert_eq!(chat.len(), 1);
        assert_eq!(chat[0].tool_call_id.as_deref(), Some("toolu_01Abc123"));

        let back = chat_to_stakai(chat);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, Role::Tool);

        match &back[0].content {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    ContentPart::ToolResult { tool_call_id, .. } => {
                        assert_eq!(tool_call_id, "toolu_01Abc123");
                    }
                    other => panic!("expected ToolResult part, got {other:?}"),
                }
            }
            other => panic!("expected parts content, got {other:?}"),
        }
    }
}
