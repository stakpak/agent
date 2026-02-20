use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error(transparent)]
    Approval(#[from] crate::approval::ApprovalError),

    #[error(transparent)]
    Checkpoint(#[from] crate::checkpoint::CheckpointError),

    #[error(transparent)]
    StreamAssembly(#[from] crate::stream::StreamAssemblyError),

    #[error("inference failed: {0}")]
    Inference(String),

    #[error("hook failed: {0}")]
    Hook(String),

    #[error("compaction failed: {0}")]
    Compaction(String),

    #[error("tool execution failed: {0}")]
    ToolExecution(String),

    #[error("invalid command: {0}")]
    InvalidCommand(String),

    #[error("run cancelled")]
    Cancelled,
}
