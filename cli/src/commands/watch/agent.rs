//! Agent spawner for autopilot schedules.
//!
//! Spawns the stakpak agent as a child process when a schedule fires,
//! capturing session and checkpoint information from JSON output.

use crate::commands::agent::run::pause::EXIT_CODE_PAUSED;
use stakpak_shared::models::async_manifest::{AsyncManifest, PauseReason};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, info, warn};

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
    /// Whether the agent paused (needs approval or input).
    pub paused: bool,
    /// Pause reason if the agent paused.
    pub pause_reason: Option<PauseReason>,
    /// Resume hint command if the agent paused.
    pub resume_hint: Option<String>,
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

    /// Returns true if the agent paused (exit code 10).
    pub fn is_paused(&self) -> bool {
        self.paused || self.exit_code == Some(EXIT_CODE_PAUSED)
    }

    /// Returns true if the agent failed (non-zero exit, not paused, or timeout).
    pub fn failed(&self) -> bool {
        self.timed_out
            || matches!(self.exit_code, Some(code) if code != 0 && code != EXIT_CODE_PAUSED)
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
    /// Pause when tools require approval instead of auto-approving.
    pub pause_on_approval: bool,
}

/// Spawn the stakpak agent with the given configuration.
///
/// The agent is run in async mode (`-a`) with JSON output (`-o json`).
/// Output is parsed from the JSON manifest for session ID, checkpoint ID,
/// and pause state.
///
/// # Arguments
/// * `config` - Configuration for spawning the agent
///
/// # Returns
/// * `Ok(AgentResult)` - Agent completed (possibly with timeout or pause)
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

    // Build the command with JSON output for robust parsing
    let mut cmd = Command::new(&binary);
    cmd.arg("-a") // async mode
        .arg("-o")
        .arg("json") // JSON output for robust parsing
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
    if config.pause_on_approval {
        cmd.arg("--pause-on-approval");
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

        // Read stdout line by line
        if let Some(stdout) = stdout_handle {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
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

        (stdout_lines.join("\n"), stderr_lines.join("\n"), status)
    })
    .await;

    match result {
        Ok((stdout, stderr, status)) => {
            let exit_code = status.ok().and_then(|s| s.code());

            // Try to parse JSON manifest from stdout
            let manifest = parse_json_manifest(&stdout);

            let (session_id, checkpoint_id, paused, pause_reason, resume_hint) =
                if let Some(m) = &manifest {
                    (
                        m.session_id.clone(),
                        m.checkpoint_id.clone(),
                        m.outcome == "paused",
                        m.pause_reason.clone(),
                        m.resume_hint.clone(),
                    )
                } else {
                    (None, None, false, None, None)
                };

            info!(
                exit_code = ?exit_code,
                session_id = ?session_id,
                checkpoint_id = ?checkpoint_id,
                paused = paused,
                "Agent completed"
            );

            Ok(AgentResult {
                exit_code,
                session_id,
                checkpoint_id,
                timed_out: false,
                paused,
                pause_reason,
                resume_hint,
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
                paused: false,
                pause_reason: None,
                resume_hint: None,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

/// Parse JSON manifest from agent stdout.
/// The manifest is the last valid JSON object in the output.
fn parse_json_manifest(stdout: &str) -> Option<AsyncManifest> {
    // In JSON mode, the agent outputs a single JSON object at the end
    // Try to find and parse it by looking for lines that start with '{'
    for line in stdout.lines().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with('{')
            && let Ok(manifest) = serde_json::from_str::<AsyncManifest>(trimmed)
        {
            return Some(manifest);
        }
    }

    // Try parsing from the beginning if stdout starts with '{'
    if stdout.trim().starts_with('{')
        && let Ok(manifest) = serde_json::from_str::<AsyncManifest>(stdout.trim())
    {
        return Some(manifest);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_manifest_completed() {
        let output = r#"{"outcome":"completed","checkpoint_id":"abc12345-e29b-41d4-a716-446655440000","session_id":"550e8400-e29b-41d4-a716-446655440000","model":"claude-sonnet-4-5-20250929","agent_message":"Done!","steps":3,"total_steps":3,"usage":{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150}}"#;

        let manifest = parse_json_manifest(output);
        assert!(manifest.is_some());
        let m = manifest.unwrap();
        assert_eq!(m.outcome, "completed");
        assert_eq!(
            m.session_id,
            Some("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
        assert_eq!(
            m.checkpoint_id,
            Some("abc12345-e29b-41d4-a716-446655440000".to_string())
        );
        assert!(m.pause_reason.is_none());
    }

    #[test]
    fn test_parse_json_manifest_paused() {
        let output = r#"{"outcome":"paused","checkpoint_id":"abc12345-e29b-41d4-a716-446655440000","session_id":"550e8400-e29b-41d4-a716-446655440000","model":"claude-sonnet-4-5-20250929","agent_message":"Need approval","steps":2,"total_steps":2,"usage":{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150},"pause_reason":{"type":"tool_approval_required","pending_tool_calls":[{"id":"call_123","name":"run_command","arguments":{"command":"ls"}}]},"resume_hint":"stakpak -c abc12345-e29b-41d4-a716-446655440000 --approve-all"}"#;

        let manifest = parse_json_manifest(output);
        assert!(manifest.is_some());
        let m = manifest.unwrap();
        assert_eq!(m.outcome, "paused");
        assert!(m.pause_reason.is_some());
        assert!(m.resume_hint.is_some());
    }

    #[test]
    fn test_parse_json_manifest_no_match() {
        let output = "No JSON here, just text output";
        let manifest = parse_json_manifest(output);
        assert!(manifest.is_none());
    }

    #[test]
    fn test_parse_json_manifest_with_prefix() {
        // JSON output may have some text before it
        let output = r#"[info] Starting...
[info] Processing...
{"outcome":"completed","checkpoint_id":"abc12345","session_id":"def67890","model":"test","agent_message":null,"steps":1,"total_steps":1,"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;

        let manifest = parse_json_manifest(output);
        assert!(manifest.is_some());
        assert_eq!(manifest.unwrap().outcome, "completed");
    }

    #[test]
    fn test_agent_result_success() {
        let result = AgentResult {
            exit_code: Some(0),
            session_id: Some("test-session".to_string()),
            checkpoint_id: Some("test-checkpoint".to_string()),
            timed_out: false,
            paused: false,
            pause_reason: None,
            resume_hint: None,
            stdout: String::new(),
            stderr: String::new(),
        };

        assert!(result.success());
        assert!(!result.failed());
        assert!(!result.is_paused());
    }

    #[test]
    fn test_agent_result_failure() {
        let result = AgentResult {
            exit_code: Some(1),
            session_id: None,
            checkpoint_id: None,
            timed_out: false,
            paused: false,
            pause_reason: None,
            resume_hint: None,
            stdout: String::new(),
            stderr: "Error occurred".to_string(),
        };

        assert!(!result.success());
        assert!(result.failed());
        assert!(!result.is_paused());
    }

    #[test]
    fn test_agent_result_timeout() {
        let result = AgentResult {
            exit_code: None,
            session_id: None,
            checkpoint_id: None,
            timed_out: true,
            paused: false,
            pause_reason: None,
            resume_hint: None,
            stdout: String::new(),
            stderr: String::new(),
        };

        assert!(!result.success());
        assert!(result.failed());
        assert!(!result.is_paused());
    }

    #[test]
    fn test_agent_result_paused() {
        let result = AgentResult {
            exit_code: Some(EXIT_CODE_PAUSED),
            session_id: Some("test-session".to_string()),
            checkpoint_id: Some("test-checkpoint".to_string()),
            timed_out: false,
            paused: true,
            pause_reason: Some(PauseReason::ToolApprovalRequired {
                pending_tool_calls: vec![],
            }),
            resume_hint: Some("stakpak -c test-checkpoint --approve-all".to_string()),
            stdout: String::new(),
            stderr: String::new(),
        };

        assert!(!result.success());
        assert!(!result.failed()); // Paused is not a failure
        assert!(result.is_paused());
    }

    // Integration tests would require mocking the stakpak binary
    // These are better suited for integration test files
}
