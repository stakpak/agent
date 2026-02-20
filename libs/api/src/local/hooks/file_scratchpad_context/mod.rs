use crate::local::context_managers::file_scratchpad_context_manager::{
    FileScratchpadContextManager, FileScratchpadContextManagerOptions,
};
use crate::models::AgentState;
use stakpak_shared::define_hook;
use stakpak_shared::hooks::{Hook, HookAction, HookContext, HookError, LifecycleEvent};
use stakpak_shared::models::integrations::openai::Role;
use stakpak_shared::models::llm::{LLMInput, LLMMessage, LLMMessageContent};

const SYSTEM_PROMPT: &str = include_str!("./system_prompt.txt");
const SCRATCHPAD_FILE: &str = ".stakpak/session/scratchpad.md";
const TODO_FILE: &str = ".stakpak/session/todo.md";

pub struct FileScratchpadContextHook {
    pub context_manager: FileScratchpadContextManager,
}

pub struct FileScratchpadContextHookOptions {
    pub scratchpad_path: Option<String>,
    pub todo_path: Option<String>,
    pub history_action_message_size_limit: Option<usize>,
    pub history_action_message_keep_last_n: Option<usize>,
    pub history_action_result_keep_last_n: Option<usize>,
    pub overwrite_if_different: Option<bool>,
}

impl FileScratchpadContextHook {
    pub fn new(options: FileScratchpadContextHookOptions) -> Self {
        let context_manager =
            FileScratchpadContextManager::new(FileScratchpadContextManagerOptions {
                scratchpad_file_path: options
                    .scratchpad_path
                    .unwrap_or(SCRATCHPAD_FILE.to_string())
                    .into(),
                todo_file_path: options.todo_path.unwrap_or(TODO_FILE.to_string()).into(),
                history_action_message_size_limit: options
                    .history_action_message_size_limit
                    .unwrap_or(100),
                history_action_message_keep_last_n: options
                    .history_action_message_keep_last_n
                    .unwrap_or(1),
                history_action_result_keep_last_n: options
                    .history_action_result_keep_last_n
                    .unwrap_or(50),
                overwrite_if_different: options.overwrite_if_different.unwrap_or(true),
            });

        Self { context_manager }
    }
}

define_hook!(
    FileScratchpadContextHook,
    "file_scratchpad_context",
    async |&self, ctx: &mut HookContext<AgentState>, event: &LifecycleEvent| {
        if *event != LifecycleEvent::BeforeInference {
            return Ok(HookAction::Continue);
        }

        let model = ctx.state.active_model.clone();

        let tools = ctx
            .state
            .tools
            .clone()
            .map(|t| t.into_iter().map(Into::into).collect());

        let cwd = std::env::current_dir().unwrap_or_default();
        let scratchpad_file_path =
            cwd.join(self.context_manager.get_scratchpad_path(ctx.session_id));
        let todo_file_path = cwd.join(self.context_manager.get_todo_path(ctx.session_id));
        let system_prompt = SYSTEM_PROMPT
            .replace(
                "{{SCRATCHPAD_PATH}}",
                &scratchpad_file_path.display().to_string(),
            )
            .replace("{{TODO_PATH}}", &todo_file_path.display().to_string());

        let mut messages = Vec::new();
        messages.push(LLMMessage {
            role: Role::System.to_string(),
            content: LLMMessageContent::String(system_prompt),
        });
        messages.extend(
            self.context_manager
                .reduce_context_with_session(ctx.state.messages.clone(), ctx.session_id),
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
