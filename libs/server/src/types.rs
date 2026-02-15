use stakpak_agent_core::AgentCommand;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct SessionHandle {
    pub command_tx: mpsc::Sender<AgentCommand>,
    pub cancel: CancellationToken,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle").finish_non_exhaustive()
    }
}

impl SessionHandle {
    pub fn new(command_tx: mpsc::Sender<AgentCommand>, cancel: CancellationToken) -> Self {
        Self { command_tx, cancel }
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
