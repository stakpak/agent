//! Passthrough context hook - enables prompt caching by preserving message structure
//!
//! This hook passes conversation messages through without compression, which enables
//! Anthropic's prompt caching. Each turn appends to the conversation rather than
//! rewriting it, creating a stable prefix that can be cached.

use crate::local::context_managers::ContextManager;
use crate::local::context_managers::passthrough_context_manager::PassthroughContextManager;
use crate::local::{ModelOptions, ModelSet};
use crate::models::AgentState;
use stakpak_shared::define_hook;
use stakpak_shared::hooks::{Hook, HookAction, HookContext, HookError, LifecycleEvent};
use stakpak_shared::models::integrations::openai::Role;
use stakpak_shared::models::llm::{LLMInput, LLMMessage, LLMMessageContent};

const SYSTEM_PROMPT: &str = include_str!("./system_prompt.txt");

pub struct PassthroughContextHook {
    pub model_set: ModelSet,
    pub context_manager: PassthroughContextManager,
}

pub struct PassthroughContextHookOptions {
    pub model_options: ModelOptions,
}

impl PassthroughContextHook {
    pub fn new(options: PassthroughContextHookOptions) -> Self {
        let model_set: ModelSet = options.model_options.into();

        Self {
            model_set,
            context_manager: PassthroughContextManager,
        }
    }
}

define_hook!(
    PassthroughContextHook,
    "passthrough_context",
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

        // System prompt
        messages.push(LLMMessage {
            role: Role::System.to_string(),
            content: LLMMessageContent::String(SYSTEM_PROMPT.to_string()),
        });

        // Pass through conversation messages without compression
        // This preserves the real message structure for effective caching
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
