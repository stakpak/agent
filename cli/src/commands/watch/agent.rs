//! Agent spawner for autopilot schedules.
//!
//! Spawns the stakpak agent via the co-hosted agent server API.

use crate::commands::agent::run::pause::EXIT_CODE_PAUSED;
use stakpak_gateway::client::{
    CallerContextInput, ClientError, SendMessageOptions, StakpakClient, ToolDecisionAction,
    ToolDecisionInput,
};
use stakpak_shared::models::async_manifest::PauseReason;
use std::collections::HashMap;
use std::time::Duration;
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
    #[error("Failed to spawn agent: {0}")]
    SpawnError(String),
}

/// Connection details for the co-hosted agent server.
#[derive(Debug, Clone)]
pub struct AgentServerConnection {
    /// Base URL of the agent server (e.g., "http://127.0.0.1:4096").
    pub url: String,
    /// Bearer token for server authentication.
    pub token: String,
    /// Resolved model identifier (e.g., "claude-sonnet-4-5-20250929").
    pub model: Option<String>,
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
    /// Run agent tool calls inside a sandboxed warden container.
    pub sandbox: bool,
    /// Additional structured caller context injected via server API.
    pub caller_context: Vec<CallerContextInput>,
    /// Agent server connection.
    pub server: AgentServerConnection,
}

/// Spawn the stakpak agent via the co-hosted agent server API.
///
/// Creates a session, sends the prompt, and drains SSE events until
/// the run completes, errors, or times out.
pub async fn spawn_agent(config: SpawnConfig) -> Result<AgentResult, AgentError> {
    let server = &config.server;
    let client = StakpakClient::new(server.url.clone(), server.token.clone());

    debug!(
        server_url = %server.url,
        model = ?server.model,
        timeout_secs = config.timeout.as_secs(),
        "Spawning agent via server API"
    );

    let result = tokio::time::timeout(config.timeout, async {
        run_server_session(&client, server, &config).await
    })
    .await;

    match result {
        Ok(Ok(agent_result)) => Ok(agent_result),
        Ok(Err(e)) => Err(AgentError::SpawnError(format!("Server API error: {}", e))),
        Err(_) => {
            warn!(
                timeout_secs = config.timeout.as_secs(),
                "Agent timed out (server API)"
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

/// Execute a full server session: create → send message → drain events.
async fn run_server_session(
    client: &StakpakClient,
    server: &AgentServerConnection,
    config: &SpawnConfig,
) -> Result<AgentResult, ClientError> {
    let session = client
        .create_session(&format!("autopilot: {}", config.profile))
        .await?;
    let session_id = session.id.to_string();

    let message = stakai::Message {
        role: stakai::Role::User,
        content: stakai::MessageContent::Text(config.prompt.clone()),
        name: None,
        provider_options: None,
    };

    let opts = SendMessageOptions {
        model: server.model.clone(),
        sandbox: if config.sandbox { Some(true) } else { None },
        context: config.caller_context.clone(),
        ..Default::default()
    };

    let send_resp = client
        .send_messages(&session_id, vec![message], opts)
        .await?;
    let run_id = send_resp.run_id.to_string();

    let mut event_stream = client.subscribe_events(&session_id, None).await?;

    let mut agent_message = String::new();
    let mut paused = false;
    let mut pause_reason: Option<PauseReason> = None;

    loop {
        let Some(event) = event_stream.next_event().await? else {
            break;
        };

        if let Some(delta) = event.as_text_delta() {
            agent_message.push_str(&delta);
        }

        if let Some(proposed) = event.as_tool_calls_proposed() {
            if config.pause_on_approval {
                paused = true;
                let pending: Vec<stakpak_shared::models::async_manifest::PendingToolCall> =
                    proposed
                        .tool_calls
                        .iter()
                        .map(
                            |tc| stakpak_shared::models::async_manifest::PendingToolCall {
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            },
                        )
                        .collect();
                pause_reason = Some(PauseReason::ToolApprovalRequired {
                    pending_tool_calls: pending,
                });
                break;
            }

            // Auto-approve all tool calls
            let mut decisions = HashMap::new();
            for tc in &proposed.tool_calls {
                decisions.insert(
                    tc.id.clone(),
                    ToolDecisionInput {
                        action: ToolDecisionAction::Accept,
                        content: None,
                    },
                );
            }
            client
                .resolve_tools(&session_id, &run_id, decisions)
                .await?;
        }

        if event.as_run_completed().is_some() || event.as_run_error().is_some() {
            if let Some(err) = event.as_run_error() {
                let error_msg = err.error.unwrap_or_else(|| "unknown error".to_string());
                info!(session_id = %session_id, error = %error_msg, "Agent run error");
                return Ok(AgentResult {
                    exit_code: Some(1),
                    session_id: Some(session_id),
                    checkpoint_id: None,
                    timed_out: false,
                    paused: false,
                    pause_reason: None,
                    resume_hint: None,
                    stdout: agent_message,
                    stderr: error_msg,
                });
            }
            break;
        }
    }

    info!(
        session_id = %session_id,
        paused = paused,
        "Agent completed via server API"
    );

    Ok(AgentResult {
        exit_code: if paused {
            Some(EXIT_CODE_PAUSED)
        } else {
            Some(0)
        },
        session_id: Some(session_id),
        checkpoint_id: None,
        timed_out: false,
        paused,
        pause_reason,
        resume_hint: None,
        stdout: agent_message,
        stderr: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!result.failed());
        assert!(result.is_paused());
    }
}
