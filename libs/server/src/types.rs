use stakpak_agent_core::ToolApprovalPolicy;
pub use stakpak_shared::models::overrides::{AutoApproveOverride, RunOverrides};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct SessionHandle {
    pub command_tx: mpsc::Sender<stakpak_agent_core::AgentCommand>,
    pub cancel: CancellationToken,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle").finish_non_exhaustive()
    }
}

impl SessionHandle {
    pub fn new(
        command_tx: mpsc::Sender<stakpak_agent_core::AgentCommand>,
        cancel: CancellationToken,
    ) -> Self {
        Self { command_tx, cancel }
    }
}

#[derive(Clone)]
pub struct RunConfig {
    pub model: stakai::Model,
    pub inference: Arc<stakai::Inference>,
    pub tool_approval_policy: ToolApprovalPolicy,
    pub system_prompt: Option<String>,
    pub max_turns: usize,
}

impl std::fmt::Debug for RunConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunConfig")
            .field("model", &self.model)
            .field("tool_approval_policy", &self.tool_approval_policy)
            .field("system_prompt", &self.system_prompt)
            .field("max_turns", &self.max_turns)
            .field("inference", &"<opaque>")
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub enum SessionRuntimeState {
    #[default]
    Idle,
    Starting {
        run_id: Uuid,
    },
    Running {
        run_id: Uuid,
        handle: SessionHandle,
    },
    Failed {
        last_error: String,
    },
}

impl SessionRuntimeState {
    pub fn run_id(&self) -> Option<Uuid> {
        match self {
            SessionRuntimeState::Starting { run_id }
            | SessionRuntimeState::Running { run_id, .. } => Some(*run_id),
            SessionRuntimeState::Idle | SessionRuntimeState::Failed { .. } => None,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            SessionRuntimeState::Starting { .. } | SessionRuntimeState::Running { .. }
        )
    }
}
