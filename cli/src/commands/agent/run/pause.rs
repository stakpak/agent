use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::async_manifest::{AsyncManifest, PauseReason};
use stakpak_shared::models::integrations::openai::{ChatMessage, Role, ToolCall};
use std::collections::HashSet;

/// Exit code indicating the agent has paused and needs input or approval to resume.
pub const EXIT_CODE_PAUSED: i32 = 10;

/// The outcome of an async agent run.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AsyncOutcome {
    /// Agent completed successfully.
    Completed {
        checkpoint_id: Option<String>,
        session_id: Option<String>,
        agent_message: Option<String>,
        steps: usize,
    },
    /// Agent paused and needs input or approval.
    Paused {
        checkpoint_id: Option<String>,
        session_id: Option<String>,
        pause_reason: PauseReason,
        agent_message: Option<String>,
    },
    /// Agent failed.
    Failed { error: String },
}

/// Resume input provided via CLI flags when resuming from a paused checkpoint.
#[derive(Debug, Clone, Default)]
pub struct ResumeInput {
    /// Tool call IDs explicitly approved.
    pub approved: HashSet<String>,
    /// Tool call IDs explicitly rejected.
    pub rejected: HashSet<String>,
    /// Approve all pending tool calls.
    pub approve_all: bool,
    /// Reject all pending tool calls.
    pub reject_all: bool,
    /// Text prompt for input-required pauses.
    pub prompt: Option<String>,
}

impl ResumeInput {
    /// Returns true if this resume input contains any tool decisions.
    pub fn has_tool_decisions(&self) -> bool {
        self.approve_all
            || self.reject_all
            || !self.approved.is_empty()
            || !self.rejected.is_empty()
    }

    /// Determine whether a specific tool call ID should be approved.
    /// Unspecified tools are rejected (per design decision).
    pub fn is_approved(&self, tool_call_id: &str) -> bool {
        if self.approve_all {
            return !self.rejected.contains(tool_call_id);
        }
        if self.reject_all {
            return false;
        }
        self.approved.contains(tool_call_id)
    }
}

/// Detect pending tool calls from checkpoint messages.
/// Returns tool calls from the last assistant message that don't have corresponding tool results.
pub fn detect_pending_tool_calls(messages: &[ChatMessage]) -> Vec<ToolCall> {
    // Find the last assistant message with tool_calls
    let tool_calls = messages
        .iter()
        .rev()
        .find(|msg| msg.role == Role::Assistant && msg.tool_calls.is_some())
        .and_then(|msg| msg.tool_calls.as_ref());

    let Some(tool_calls) = tool_calls else {
        return Vec::new();
    };

    // Collect IDs of tool calls that already have results
    let executed_ids: HashSet<String> = messages
        .iter()
        .filter(|msg| msg.role == Role::Tool)
        .filter_map(|msg| msg.tool_call_id.clone())
        .collect();

    // Return tool calls without results
    tool_calls
        .iter()
        .filter(|tc| !executed_ids.contains(&tc.id))
        .cloned()
        .collect()
}

/// Write the async manifest to `.stakpak/session/pause.json`.
pub fn write_pause_manifest(manifest: &AsyncManifest) -> Result<String, String> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize pause manifest: {}", e))?;
    LocalStore::write_session_data("pause.json", &json)
}

/// Build a resume hint command string.
pub fn build_resume_hint(checkpoint_id: &str, pause_reason: &PauseReason) -> String {
    match pause_reason {
        PauseReason::ToolApprovalRequired { pending_tool_calls } => {
            if pending_tool_calls.len() == 1 {
                format!(
                    "stakpak -c {} --approve {}",
                    checkpoint_id, pending_tool_calls[0].id
                )
            } else {
                format!("stakpak -c {} --approve-all", checkpoint_id)
            }
        }
        PauseReason::InputRequired => {
            format!("stakpak -c {} \"your input here\"", checkpoint_id)
        }
    }
}
