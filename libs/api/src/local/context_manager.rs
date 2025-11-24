use stakpak_shared::models::integrations::openai::ChatMessage;

pub fn project_messages(messages: Vec<ChatMessage>) -> String {
    messages
        .into_iter()
        .map(|m| format!("{}: {}", m.role, m.content.unwrap().to_string()))
        .reduce(|a, b| a + "\n" + &b)
        .unwrap()
}
