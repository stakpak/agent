use stakpak_shared::define_hook;
use stakpak_shared::hooks::{Hook, HookAction, HookContext, HookError, LifecycleEvent};

use crate::local::context_managers::task_board_context_manager::{
    TaskBoardContextManager, TaskBoardContextManagerOptions,
};
use crate::models::AgentState;

const SYSTEM_PROMPT: &str = include_str!("./system_prompt.txt");

pub struct TaskBoardContextHook {
    pub context_manager: TaskBoardContextManager,
}

pub struct TaskBoardContextHookOptions {
    /// How many recent assistant messages to keep untrimmed when context
    /// trimming is triggered. Only assistant (and tool) messages are trimmed;
    /// user and system messages are always preserved in full.
    pub keep_last_n_assistant_messages: Option<usize>,
    /// Fraction of the context window at which trimming triggers (e.g. 0.8 = 80%).
    pub context_budget_threshold: Option<f32>,
}

impl TaskBoardContextHook {
    pub fn new(options: TaskBoardContextHookOptions) -> Self {
        let context_manager = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: options.keep_last_n_assistant_messages.unwrap_or(50),
            context_budget_threshold: options.context_budget_threshold.unwrap_or(0.8),
        });

        Self { context_manager }
    }
}

define_hook!(
    TaskBoardContextHook,
    "task_board_context",
    async |&self, ctx: &mut HookContext<AgentState>, event: &LifecycleEvent| {
        if *event != LifecycleEvent::BeforeInference {
            return Ok(HookAction::Continue);
        }

        let model = ctx.state.active_model.clone();
        let max_output_tokens: u64 = 16000;

        // Subtract fixed overhead from context window so the trimmer budgets
        // only the space actually available for chat messages.
        // - System prompt: added after trimming (line 67+), not in message list
        // - max_output_tokens: reserved for the model's response
        let system_prompt_tokens =
            TaskBoardContextManager::estimate_tokens(&[stakai::Message::new(
                stakai::Role::System,
                SYSTEM_PROMPT,
            )]);
        let context_window = model
            .limit
            .context
            .saturating_sub(system_prompt_tokens + max_output_tokens);

        // Use budget-aware trimming with metadata from checkpoint.
        // Tool definitions are passed in so the context manager can account
        // for their token overhead internally.
        let (reduced_messages, updated_metadata) = self.context_manager.reduce_context_with_budget(
            ctx.state.messages.clone(),
            context_window,
            ctx.state.metadata.clone(),
            ctx.state.tools.as_deref(),
        );

        // Write updated metadata back to state for checkpoint persistence
        ctx.state.metadata = updated_metadata;

        let mut messages = Vec::new();
        messages.push(stakai::Message::new(stakai::Role::System, SYSTEM_PROMPT));
        messages.extend(reduced_messages);

        ctx.state.llm_input = Some(stakai::GenerateRequest {
            model,
            messages,
            options: stakai::GenerateOptions {
                max_tokens: Some(max_output_tokens as u32),
                tools: ctx.state.tools.clone(),
                ..Default::default()
            },
            provider_options: None,
            telemetry_metadata: None,
        });

        Ok(HookAction::Continue)
    }
);
