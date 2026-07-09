#[allow(dead_code)]
pub struct SimpleContextManager;

impl super::ContextManager for SimpleContextManager {
    fn reduce_context(&self, messages: Vec<stakai::Message>) -> Vec<stakai::Message> {
        if messages.is_empty() {
            return vec![];
        }

        let mut context = Vec::new();

        // 1. Flatten history (all messages except the last one)
        if messages.len() > 1 {
            let history_content = messages[..messages.len() - 1]
                .iter()
                .map(|m| {
                    format!(
                        "{}: {}",
                        super::common::role_label(m.role),
                        m.text().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            context.push(stakai::Message::new(stakai::Role::User, history_content));
        }

        // 2. Preserve the last message (with images)
        if let Some(last_message) = messages.last() {
            context.push(last_message.clone());
        }

        context
    }
}
