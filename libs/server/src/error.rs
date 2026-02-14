use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionManagerError {
    #[error("session is already running")]
    SessionAlreadyRunning,

    #[error("session is still starting")]
    SessionStarting,

    #[error("session is not running")]
    SessionNotRunning,

    #[error("run mismatch: active={active_run_id}, requested={requested_run_id}")]
    RunMismatch {
        active_run_id: Uuid,
        requested_run_id: Uuid,
    },

    #[error("actor startup failed: {0}")]
    ActorStartupFailed(String),

    #[error("run command channel is closed")]
    CommandChannelClosed,
}
