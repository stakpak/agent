//! Agent spawner for daemon triggers.
//!
//! Spawns the stakpak agent as a child process when a trigger fires,
//! capturing session and checkpoint information from the output.

use regex::Regex;
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Regex pattern to extract session ID from agent output.
/// Matches: "Session ID: {uuid}"
static SESSION_ID_REGEX: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"Session ID:\s*([0-9a-fA-F-]{36})").ok());

/// Regex pattern to extract checkpoint ID from agent output.
/// Matches: "stakpak -c {uuid}" in the resume command output
static CHECKPOINT_ID_REGEX: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"stakpak -c\s+([0-9a-fA-F-]{36})").ok());

/// Result of spawning and running the agent.
#[derive(Debug, Clone)]
pub struct AgentResult {
    /// Exit code of the agent process (None if killed/timed out).
    pub exit_code: Option<i32>,
    /// Session ID extracted from agent output.
    pub session_id: Option<String>,
    /// Last checkpoint ID extracted from agent output.
    pub checkpoint_id: Option<String>,
    /// Whether the agent was killed due to timeout.
    pub timed_out: bool,
    /// Combined stdout output from the agent.
    pub stdout: String,
    /// Combined stderr output from the agent.
    pub stderr: String,
}

impl AgentResult {
    /// Returns true if the agent completed successfully (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// Returns true if the agent failed (non-zero exit or timeout).
    pub fn failed(&self) -> bool {
        self.timed_out || matches!(self.exit_code, Some(code) if code != 0)
    }
}

/// Errors that can occur during agent spawning.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Failed to spawn agent process: {0}")]
    SpawnError(String),

    #[error("Failed to read agent output: {0}")]
    OutputError(String),

    #[error("Agent binary not found: {0}")]
    BinaryNotFound(String),
}

/// Configuration for spawning the agent.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// The assembled prompt to pass to the agent.
    pub prompt: String,
    /// Profile to use for agent invocation.
    pub profile: String,
    /// Maximum time to wait for the agent to complete.
    pub timeout: Duration,
    /// Working directory for the agent (optional).
    pub workdir: Option<String>,
    /// Enable Slack tools (experimental).
    pub enable_slack_tools: bool,
    /// Enable subagents.
    pub enable_subagents: bool,
}

/// Spawn the stakpak agent with the given configuration.
///
/// The agent is run in async mode (`-a`) to completion. Output is captured
/// and parsed for session ID and checkpoint ID.
///
/// # Arguments
/// * `config` - Configuration for spawning the agent
///
/// # Returns
/// * `Ok(AgentResult)` - Agent completed (possibly with timeout)
/// * `Err(AgentError)` - Failed to spawn or run the agent
pub async fn spawn_agent(config: SpawnConfig) -> Result<AgentResult, AgentError> {
    // Find the stakpak binary
    let binary = std::env::current_exe().map_err(|e| {
        AgentError::BinaryNotFound(format!("Failed to get current executable path: {}", e))
    })?;

    debug!(
        binary = %binary.display(),
        profile = %config.profile,
        timeout_secs = config.timeout.as_secs(),
        "Spawning agent"
    );

    // Build the command
    let mut cmd = Command::new(&binary);
    cmd.arg("-a") // async mode
        .arg("--profile")
        .arg(&config.profile)
        .arg(&config.prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Add optional flags
    if config.enable_slack_tools {
        cmd.arg("--enable-slack-tools");
    }
    if config.enable_subagents {
        cmd.arg("--enable-subagents");
    }

    // Set working directory if specified
    if let Some(workdir) = &config.workdir {
        cmd.current_dir(workdir);
    }

    // Spawn the process
    let mut child = cmd
        .spawn()
        .map_err(|e| AgentError::SpawnError(e.to_string()))?;

    // Take ownership of stdout/stderr handles
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    // Capture output with timeout
    let result = tokio::time::timeout(config.timeout, async {
        let mut stdout_lines = Vec::new();
        let mut stderr_lines = Vec::new();
        let mut session_id: Option<String> = None;
        let mut checkpoint_id: Option<String> = None;

        // Read stdout line by line to capture session/checkpoint IDs
        if let Some(stdout) = stdout_handle {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                // Check for session ID
                if let Some(regex) = SESSION_ID_REGEX.as_ref()
                    && let Some(caps) = regex.captures(&line)
                    && let Some(id) = caps.get(1)
                {
                    session_id = Some(id.as_str().to_string());
                    debug!(session_id = %id.as_str(), "Captured session ID");
                }

                // Check for checkpoint ID
                if let Some(regex) = CHECKPOINT_ID_REGEX.as_ref()
                    && let Some(caps) = regex.captures(&line)
                    && let Some(id) = caps.get(1)
                {
                    checkpoint_id = Some(id.as_str().to_string());
                    debug!(checkpoint_id = %id.as_str(), "Captured checkpoint ID");
                }

                stdout_lines.push(line);
            }
        }

        // Read stderr
        if let Some(stderr) = stderr_handle {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                stderr_lines.push(line);
            }
        }

        // Wait for the process to exit
        let status = child.wait().await;

        (
            stdout_lines.join("\n"),
            stderr_lines.join("\n"),
            status,
            session_id,
            checkpoint_id,
        )
    })
    .await;

    match result {
        Ok((stdout, stderr, status, session_id, checkpoint_id)) => {
            let exit_code = status.ok().and_then(|s| s.code());

            info!(
                exit_code = ?exit_code,
                session_id = ?session_id,
                checkpoint_id = ?checkpoint_id,
                "Agent completed"
            );

            Ok(AgentResult {
                exit_code,
                session_id,
                checkpoint_id,
                timed_out: false,
                stdout,
                stderr,
            })
        }
        Err(_) => {
            // Timeout occurred - process will be killed by kill_on_drop
            warn!(
                timeout_secs = config.timeout.as_secs(),
                "Agent timed out, killing process"
            );

            Ok(AgentResult {
                exit_code: None,
                session_id: None,
                checkpoint_id: None,
                timed_out: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

/// Parse session ID from agent output text.
pub fn parse_session_id(output: &str) -> Option<String> {
    SESSION_ID_REGEX
        .as_ref()?
        .captures(output)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Parse checkpoint ID from agent output text.
pub fn parse_checkpoint_id(output: &str) -> Option<String> {
    CHECKPOINT_ID_REGEX
        .as_ref()?
        .captures(output)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_id() {
        let output = r#"
[info] Starting agent...
Some output here
Session ID: 550e8400-e29b-41d4-a716-446655440000
More output
"#;
        let session_id = parse_session_id(output);
        assert_eq!(
            session_id,
            Some("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
    }

    #[test]
    fn test_parse_session_id_no_match() {
        let output = "No session ID here";
        let session_id = parse_session_id(output);
        assert_eq!(session_id, None);
    }

    #[test]
    fn test_parse_checkpoint_id() {
        let output = r#"
[success] Checkpoint abc12345-e29b-41d4-a716-446655440000 saved to /path/to/file

To resume, run:
stakpak -c abc12345-e29b-41d4-a716-446655440000

Session ID: 550e8400-e29b-41d4-a716-446655440000
"#;
        let checkpoint_id = parse_checkpoint_id(output);
        assert_eq!(
            checkpoint_id,
            Some("abc12345-e29b-41d4-a716-446655440000".to_string())
        );
    }

    #[test]
    fn test_parse_checkpoint_id_no_match() {
        let output = "No checkpoint here";
        let checkpoint_id = parse_checkpoint_id(output);
        assert_eq!(checkpoint_id, None);
    }

    #[test]
    fn test_agent_result_success() {
        let result = AgentResult {
            exit_code: Some(0),
            session_id: Some("test-session".to_string()),
            checkpoint_id: Some("test-checkpoint".to_string()),
            timed_out: false,
            stdout: String::new(),
            stderr: String::new(),
        };

        assert!(result.success());
        assert!(!result.failed());
    }

    #[test]
    fn test_agent_result_failure() {
        let result = AgentResult {
            exit_code: Some(1),
            session_id: None,
            checkpoint_id: None,
            timed_out: false,
            stdout: String::new(),
            stderr: "Error occurred".to_string(),
        };

        assert!(!result.success());
        assert!(result.failed());
    }

    #[test]
    fn test_agent_result_timeout() {
        let result = AgentResult {
            exit_code: None,
            session_id: None,
            checkpoint_id: None,
            timed_out: true,
            stdout: String::new(),
            stderr: String::new(),
        };

        assert!(!result.success());
        assert!(result.failed());
    }

    // Integration tests would require mocking the stakpak binary
    // These are better suited for integration test files
}
