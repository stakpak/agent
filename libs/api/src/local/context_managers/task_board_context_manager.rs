use super::common::{HistoryProcessingOptions, history_to_text, messages_to_history};
use stakpak_shared::models::{
    integrations::openai::{ChatMessage, Role},
    llm::{LLMMessage, LLMMessageContent},
};

pub struct TaskBoardContextManager {
    options: HistoryProcessingOptions,
}

impl super::ContextManager for TaskBoardContextManager {
    fn reduce_context(&self, messages: Vec<ChatMessage>) -> Vec<LLMMessage> {
        let history = messages_to_history(&messages, &self.options);
        let context_content = history_to_text(&history);

        vec![LLMMessage {
            role: Role::User.to_string(),
            content: LLMMessageContent::String(context_content),
        }]
    }
}

pub struct TaskBoardContextManagerOptions {
    pub history_action_message_size_limit: usize,
    pub history_action_message_keep_last_n: usize,
    pub history_action_result_keep_last_n: usize,
}

impl TaskBoardContextManager {
    pub fn new(options: TaskBoardContextManagerOptions) -> Self {
        Self {
            options: HistoryProcessingOptions {
                history_action_message_size_limit: options.history_action_message_size_limit,
                history_action_message_keep_last_n: options.history_action_message_keep_last_n,
                history_action_result_keep_last_n: options.history_action_result_keep_last_n,
                truncation_hint: "consult the task board cards instead".to_string(),
            },
        }
    }
}
