//! Stakpak API models for sessions and checkpoints
//!
//! These types match the new `/v1/sessions` API endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::openai::ChatMessage;
use uuid::Uuid;

// =============================================================================
// Session Types
// =============================================================================

/// Session visibility
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum SessionVisibility {
    Private,
    Public,
}

impl Default for SessionVisibility {
    fn default() -> Self {
        Self::Private
    }
}

/// Session status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum SessionStatus {
    Active,
    Deleted,
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Full session with active checkpoint
#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub title: String,
    pub visibility: SessionVisibility,
    pub status: SessionStatus,
    pub cwd: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub active_checkpoint: Option<Checkpoint>,
}

/// Session summary for list responses
#[derive(Debug, Clone, Deserialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub title: String,
    pub visibility: SessionVisibility,
    pub status: SessionStatus,
    pub cwd: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u32,
    pub active_checkpoint_id: Uuid,
    pub last_message_at: Option<DateTime<Utc>>,
}

// =============================================================================
// Checkpoint Types
// =============================================================================

/// Full checkpoint with state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: Uuid,
    pub session_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub state: CheckpointState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Checkpoint summary for list responses
#[derive(Debug, Clone, Deserialize)]
pub struct CheckpointSummary {
    pub id: Uuid,
    pub session_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub message_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// State is optionally included based on `include_state` query param
    pub state: Option<CheckpointState>,
}

/// Checkpoint state containing messages
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckpointState {
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

// =============================================================================
// Request Types
// =============================================================================

/// Request to create a session (with initial checkpoint state)
#[derive(Debug, Serialize)]
pub struct CreateSessionRequest {
    /// Session title
    pub title: String,
    /// Session visibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<SessionVisibility>,
    /// Working directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Initial checkpoint state with messages
    pub state: CheckpointState,
}

/// Request to create a checkpoint (for subsequent checkpoints)
#[derive(Debug, Serialize)]
pub struct CreateCheckpointRequest {
    /// Checkpoint state with messages
    pub state: CheckpointState,
    /// Parent checkpoint ID (for branching)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
}

/// Query parameters for listing sessions
#[derive(Debug, Default, Serialize)]
pub struct ListSessionsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

/// Request to update a session
#[derive(Debug, Serialize)]
pub struct UpdateSessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<SessionVisibility>,
}

/// Query parameters for listing checkpoints
#[derive(Debug, Default, Serialize)]
pub struct ListCheckpointsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_state: Option<bool>,
}

// =============================================================================
// Response Types
// =============================================================================

/// Response from creating a session
#[derive(Debug, Deserialize)]
pub struct CreateSessionResponse {
    pub session_id: Uuid,
    pub checkpoint: Checkpoint,
}

/// Response from creating a checkpoint
#[derive(Debug, Deserialize)]
pub struct CreateCheckpointResponse {
    pub checkpoint: Checkpoint,
}

/// Response from listing sessions
#[derive(Debug, Deserialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionSummary>,
}

/// Response from getting a session
#[derive(Debug, Deserialize)]
pub struct GetSessionResponse {
    pub session: Session,
}

/// Response from updating a session
#[derive(Debug, Deserialize)]
pub struct UpdateSessionResponse {
    pub session: Session,
}

/// Response from deleting a session
#[derive(Debug, Deserialize)]
pub struct DeleteSessionResponse {
    pub success: bool,
    pub deleted_at: DateTime<Utc>,
}

/// Response from listing checkpoints
#[derive(Debug, Deserialize)]
pub struct ListCheckpointsResponse {
    pub checkpoints: Vec<CheckpointSummary>,
}

/// Response from getting a checkpoint
#[derive(Debug, Deserialize)]
pub struct GetCheckpointResponse {
    pub checkpoint: Checkpoint,
}

// =============================================================================
// MCP Tool Request Types
// =============================================================================

/// Request for searching documentation
#[derive(Debug, Serialize)]
pub struct SearchDocsRequest {
    pub keywords: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_keywords: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Request for searching memory
#[derive(Debug, Serialize)]
pub struct SearchMemoryRequest {
    pub keywords: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,
}

/// Request for reading Slack messages
#[derive(Debug, Serialize)]
pub struct SlackReadMessagesRequest {
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Request for reading Slack thread replies
#[derive(Debug, Serialize)]
pub struct SlackReadRepliesRequest {
    pub channel: String,
    pub ts: String,
}

/// Request for sending a Slack message
#[derive(Debug, Serialize)]
pub struct SlackSendMessageRequest {
    pub channel: String,
    pub markdown_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
}

// =============================================================================
// Internal Types
// =============================================================================

/// Parameters for calling MCP tools
#[derive(Debug, Serialize)]
pub(crate) struct ToolsCallParams {
    pub name: String,
    pub arguments: serde_json::Value,
}
