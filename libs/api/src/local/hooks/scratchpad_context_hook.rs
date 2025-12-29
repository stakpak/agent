use stakpak_shared::define_hook;
use stakpak_shared::hooks::{Hook, HookAction, HookContext, HookError, LifecycleEvent};
use stakpak_shared::models::integrations::openai::{AgentModel, Role};
use stakpak_shared::models::llm::{LLMInput, LLMMessage, LLMMessageContent, LLMModel};

use crate::local::context_managers::ContextManager;
use crate::models::AgentState;

pub struct ContextHook {
    pub context_manager: Box<dyn ContextManager>,
    pub smart_model: (LLMModel, String),
    pub eco_model: (LLMModel, String),
    pub recovery_model: (LLMModel, String),
}

pub struct ContextHookOptions {
    pub context_manager: Box<dyn ContextManager>,
    pub smart_model: (LLMModel, String),
    pub eco_model: (LLMModel, String),
    pub recovery_model: (LLMModel, String),
}

impl ContextHook {
    pub fn new(options: ContextHookOptions) -> Self {
        Self {
            context_manager: options.context_manager,
            smart_model: options.smart_model,
            eco_model: options.eco_model,
            recovery_model: options.recovery_model,
        }
    }
}

define_hook!(
    ContextHook,
    "context",
    async |&self, ctx: &mut HookContext<AgentState>, event: &LifecycleEvent| {
        if *event != LifecycleEvent::BeforeInference {
            return Ok(HookAction::Continue);
        }

        let (model, system_prompt) = match ctx.state.agent_model {
            AgentModel::Smart => self.smart_model.clone(),
            AgentModel::Eco => self.eco_model.clone(),
            AgentModel::Recovery => self.recovery_model.clone(),
        };

        let mut messages = vec![LLMMessage {
            role: Role::System.to_string(),
            content: LLMMessageContent::String(system_prompt),
        }];
        messages.extend(
            self.context_manager
                .reduce_context(ctx.state.messages.clone()),
        );

        let tools = ctx
            .state
            .tools
            .clone()
            .map(|t| t.into_iter().map(Into::into).collect());

        ctx.state.llm_input = Some(LLMInput {
            model,
            messages,
            max_tokens: 16000,
            tools,
            provider_options: None, // TODO: Pass provider options from context if needed
        });

        Ok(HookAction::Continue)
    }
);
