use serde::{Deserialize, Serialize};
use stakpak_agent_core::ToolApprovalPolicy;
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AutoApproveOverride {
    /// "all" | "none"
    Mode(String),
    /// Explicit allowlist for auto-approval.
    AllowList(Vec<String>),
}

/// Per-request run overrides merged with `AppState` defaults.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct RunOverrides {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub auto_approve: Option<AutoApproveOverride>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub max_turns: Option<usize>,
}

impl RunOverrides {
    pub fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.auto_approve.is_none()
            && self.system_prompt.is_none()
            && self.max_turns.is_none()
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
