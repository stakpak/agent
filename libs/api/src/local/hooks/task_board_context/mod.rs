use stakpak_shared::define_hook;
use stakpak_shared::hooks::{Hook, HookAction, HookContext, HookError, LifecycleEvent};
use stakpak_shared::models::integrations::openai::Role;
use stakpak_shared::models::llm::{LLMInput, LLMMessage, LLMMessageContent};

use crate::local::context_managers::ContextManager;
use crate::local::context_managers::task_board_context_manager::{
    TaskBoardContextManager, TaskBoardContextManagerOptions,
};
use crate::local::{ModelOptions, ModelSet};
use crate::models::AgentState;

const SYSTEM_PROMPT: &str = include_str!("./system_prompt.txt");

pub struct TaskBoardContextHook {
    pub model_set: ModelSet,
    pub context_manager: TaskBoardContextManager,
}

pub struct TaskBoardContextHookOptions {
    pub model_options: ModelOptions,
    pub history_action_message_size_limit: Option<usize>,
    pub history_action_message_keep_last_n: Option<usize>,
    pub history_action_result_keep_last_n: Option<usize>,
}

impl TaskBoardContextHook {
    pub fn new(options: TaskBoardContextHookOptions) -> Self {
        let model_set: ModelSet = options.model_options.into();

        let context_manager = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            history_action_message_size_limit: options
                .history_action_message_size_limit
                .unwrap_or(100),
            history_action_message_keep_last_n: options
                .history_action_message_keep_last_n
                .unwrap_or(1),
            history_action_result_keep_last_n: options
                .history_action_result_keep_last_n
                .unwrap_or(50),
        });

        Self {
            model_set,
            context_manager,
        }
    }
}

define_hook!(
    TaskBoardContextHook,
    "task_board_context",
    async |&self, ctx: &mut HookContext<AgentState>, event: &LifecycleEvent| {
        if *event != LifecycleEvent::BeforeInference {
            return Ok(HookAction::Continue);
        }

        let model = self.model_set.get_model(&ctx.state.agent_model);

        let tools = ctx
            .state
            .tools
            .clone()
            .map(|t| t.into_iter().map(Into::into).collect());

        let mut messages = Vec::new();
        messages.push(LLMMessage {
            role: Role::System.to_string(),
            content: LLMMessageContent::String(SYSTEM_PROMPT.to_string()),
        });
        messages.extend(
            self.context_manager
                .reduce_context(ctx.state.messages.clone()),
        );

        ctx.state.llm_input = Some(LLMInput {
            model,
            messages,
            max_tokens: 16000,
            tools,
            provider_options: None,
            headers: None,
        });

        Ok(HookAction::Continue)
    }
);
