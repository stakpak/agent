//! Shell Session Trait Definition
//!
//! Defines the common interface for both local and remote persistent shell sessions.

use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;

/// Errors that can occur during shell session operations
#[derive(Debug, Error)]
pub enum ShellSessionError {
    #[error("Session not alive: {0}")]
    SessionDead(String),

    #[error("Command execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Command timed out after {0:?}")]
    Timeout(Duration),

    #[error("Failed to spawn shell: {0}")]
    SpawnFailed(String),

    #[error("PTY error: {0}")]
    PtyError(String),

    #[error("SSH error: {0}")]
    SshError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Session closed")]
    SessionClosed,

    #[error("Marker detection failed: {0}")]
    MarkerDetectionFailed(String),

    #[error("Channel send error: {0}")]
    ChannelError(String),
}

/// A chunk of output from streaming command execution
#[derive(Debug, Clone)]
pub struct OutputChunk {
    /// The text content of this chunk
    pub text: String,
    /// Whether this is the final chunk (command completed)
    pub is_final: bool,
}

/// Type alias for the streaming output receiver
pub type OutputReceiver = mpsc::Receiver<OutputChunk>;

/// Type alias for the streaming output sender
pub type OutputSender = mpsc::Sender<OutputChunk>;

/// Output from a command execution
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// The stdout/stderr output from the command
    pub output: String,

    /// Exit code if available (may not be available for all session types)
    pub exit_code: Option<i32>,

    /// Duration the command took to execute
    pub duration: Duration,
}

impl CommandOutput {
    /// Check if the command succeeded (exit code 0 or not available)
    pub fn success(&self) -> bool {
        self.exit_code.map(|c| c == 0).unwrap_or(true)
    }
}

/// Common interface for persistent shell sessions
///
/// Both local (PTY-based) and remote (SSH-based) sessions implement this trait,
/// allowing uniform handling of shell commands regardless of execution location.
#[async_trait]
pub trait ShellSession: Send + Sync {
    /// Execute a command in the persistent shell session
    ///
    /// The command is executed in the context of the persistent shell, meaning:
    /// - Environment variables set by previous commands are available
    /// - Working directory changes persist
    /// - Shell aliases and functions are available
    ///
    /// # Arguments
    /// * `command` - The shell command to execute
    /// * `timeout` - Optional timeout for command execution
    ///
    /// # Returns
    /// * `Ok(CommandOutput)` - Command output and metadata
    /// * `Err(ShellSessionError)` - If execution fails
    async fn execute(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<CommandOutput, ShellSessionError>;

    /// Execute a command with streaming output
    ///
    /// Similar to `execute`, but returns a channel receiver that yields output chunks
    /// as they become available. This enables real-time output streaming to the UI.
    ///
    /// # Arguments
    /// * `command` - The shell command to execute
    /// * `timeout` - Optional timeout for command execution
    ///
    /// # Returns
    /// * `Ok((OutputReceiver, JoinHandle))` - Receiver for output chunks and task handle
    /// * `Err(ShellSessionError)` - If execution fails to start
    async fn execute_streaming(
        &self,
        command: &str,
        timeout: Option<Duration>,
    ) -> Result<
        (
            OutputReceiver,
            tokio::task::JoinHandle<Result<CommandOutput, ShellSessionError>>,
        ),
        ShellSessionError,
    >;

    /// Check if the session is still alive and responsive
    ///
    /// This performs a lightweight check (e.g., sending a simple echo command)
    /// to verify the shell process is still running.
    async fn is_alive(&self) -> bool;

    /// Get the unique identifier for this session
    fn session_id(&self) -> &str;

    /// Get a human-readable description of the session
    ///
    /// For local sessions: "local:shell-abc123"
    /// For remote sessions: "remote:user@host:shell-def456"
    fn description(&self) -> String;

    /// Close the session and clean up resources
    ///
    /// After calling this, the session should not be used again.
    async fn close(&mut self) -> Result<(), ShellSessionError>;

    /// Get the current working directory of the shell session
    ///
    /// Returns None if unable to determine (e.g., session dead)
    async fn get_cwd(&self) -> Option<String> {
        match self.execute("pwd", Some(Duration::from_secs(5))).await {
            Ok(output) => Some(output.output.trim().to_string()),
            Err(_) => None,
        }
    }
}
