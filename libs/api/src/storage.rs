//! Session Storage abstraction
//!
//! Provides a unified interface for session and checkpoint management
//! with implementations for both Stakpak API and local SQLite storage.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::openai::ChatMessage;
use uuid::Uuid;

// Re-export implementations
pub use crate::local::storage::LocalStorage;
pub use crate::stakpak::storage::StakpakStorage;

// =============================================================================
// SessionStorage Trait
// =============================================================================

/// Unified session storage trait
///
/// Abstracts session and checkpoint operations for both
/// Stakpak API and local SQLite storage backends.
#[async_trait]
pub trait SessionStorage: Send + Sync {
    // =========================================================================
    // Session Operations
    // =========================================================================

    /// List all sessions
    async fn list_sessions(
        &self,
        query: &ListSessionsQuery,
    ) -> Result<ListSessionsResult, StorageError>;

    /// Get a session by ID (includes active checkpoint)
    async fn get_session(&self, session_id: Uuid) -> Result<Session, StorageError>;

    /// Create a new session with initial checkpoint
    async fn create_session(
        &self,
        request: &CreateSessionRequest,
    ) -> Result<CreateSessionResult, StorageError>;

    /// Update session metadata (title, visibility)
    async fn update_session(
        &self,
        session_id: Uuid,
        request: &UpdateSessionRequest,
    ) -> Result<Session, StorageError>;

    /// Delete a session
    async fn delete_session(&self, session_id: Uuid) -> Result<(), StorageError>;

    // =========================================================================
    // Checkpoint Operations
    // =========================================================================

    /// List checkpoints for a session
    async fn list_checkpoints(
        &self,
        session_id: Uuid,
        query: &ListCheckpointsQuery,
    ) -> Result<ListCheckpointsResult, StorageError>;

    /// Get a checkpoint by ID
    async fn get_checkpoint(&self, checkpoint_id: Uuid) -> Result<Checkpoint, StorageError>;

    /// Create a new checkpoint for a session
    async fn create_checkpoint(
        &self,
        session_id: Uuid,
        request: &CreateCheckpointRequest,
    ) -> Result<Checkpoint, StorageError>;

    // =========================================================================
    // Convenience Methods (with default implementations)
    // =========================================================================

    /// Get the latest/active checkpoint for a session
    async fn get_active_checkpoint(&self, session_id: Uuid) -> Result<Checkpoint, StorageError> {
        let session = self.get_session(session_id).await?;
        session
            .active_checkpoint
            .ok_or(StorageError::NotFound("No active checkpoint".to_string()))
    }

    /// Get session stats (optional - returns default if not supported)
    async fn get_session_stats(&self, _session_id: Uuid) -> Result<SessionStats, StorageError> {
        Ok(SessionStats::default())
    }
}

/// Box wrapper for dynamic dispatch
pub type BoxedSessionStorage = Box<dyn SessionStorage>;

// =============================================================================
// Error Types
// =============================================================================

/// Storage operation errors
#[derive(Debug, Clone, PartialEq)]
pub enum StorageError {
    /// Resource not found
    NotFound(String),
    /// Invalid request
    InvalidRequest(String),
    /// Authentication/authorization error
    Unauthorized(String),
    /// Rate limit exceeded
    RateLimited(String),
    /// Internal storage error
    Internal(String),
    /// Connection error
    Connection(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::NotFound(msg) => write!(f, "Not found: {}", msg),
            StorageError::InvalidRequest(msg) => write!(f, "Invalid request: {}", msg),
            StorageError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            StorageError::RateLimited(msg) => write!(f, "Rate limited: {}", msg),
            StorageError::Internal(msg) => write!(f, "Internal error: {}", msg),
            StorageError::Connection(msg) => write!(f, "Connection error: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<String> for StorageError {
    fn from(s: String) -> Self {
        StorageError::Internal(s)
    }
}

impl From<&str> for StorageError {
    fn from(s: &str) -> Self {
        StorageError::Internal(s.to_string())
    }
}

// =============================================================================
// Session Types
// =============================================================================

/// Session visibility
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum SessionVisibility {
    #[default]
    Private,
    Public,
}

impl std::fmt::Display for SessionVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionVisibility::Private => write!(f, "PRIVATE"),
            SessionVisibility::Public => write!(f, "PUBLIC"),
        }
    }
}

/// Session status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum SessionStatus {
    #[default]
    Active,
    Deleted,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Active => write!(f, "ACTIVE"),
            SessionStatus::Deleted => write!(f, "DELETED"),
        }
    }
}

/// Full session with optional active checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub title: String,
    pub visibility: SessionVisibility,
    pub status: SessionStatus,
    pub cwd: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub active_checkpoint: Option<Checkpoint>,
}

/// Session summary for list responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: Uuid,
    pub title: String,
    pub visibility: SessionVisibility,
    pub status: SessionStatus,
    pub cwd: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u32,
    pub active_checkpoint_id: Option<Uuid>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointSummary {
    pub id: Uuid,
    pub session_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub message_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Checkpoint state containing messages
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckpointState {
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    /// Optional metadata for context trimming state, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// =============================================================================
// Request Types
// =============================================================================

/// Request to create a session with initial checkpoint
#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionRequest {
    pub title: String,
    pub visibility: SessionVisibility,
    pub cwd: Option<String>,
    pub initial_state: CheckpointState,
}

impl CreateSessionRequest {
    pub fn new(title: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            title: title.into(),
            visibility: SessionVisibility::Private,
            cwd: None,
            initial_state: CheckpointState {
                messages,
                metadata: None,
            },
        }
    }

    pub fn with_visibility(mut self, visibility: SessionVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// Request to update a session
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
    pub visibility: Option<SessionVisibility>,
}

impl UpdateSessionRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_visibility(mut self, visibility: SessionVisibility) -> Self {
        self.visibility = Some(visibility);
        self
    }
}

/// Request to create a checkpoint
#[derive(Debug, Clone, Serialize)]
pub struct CreateCheckpointRequest {
    pub state: CheckpointState,
    pub parent_id: Option<Uuid>,
}

impl CreateCheckpointRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            state: CheckpointState {
                messages,
                metadata: None,
            },
            parent_id: None,
        }
    }

    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.state.metadata = Some(metadata);
        self
    }
}

/// Query parameters for listing sessions
#[derive(Debug, Clone, Default, Serialize)]
pub struct ListSessionsQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub search: Option<String>,
    pub status: Option<SessionStatus>,
    pub visibility: Option<SessionVisibility>,
}

impl ListSessionsQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn with_search(mut self, search: impl Into<String>) -> Self {
        self.search = Some(search.into());
        self
    }
}

/// Query parameters for listing checkpoints
#[derive(Debug, Clone, Default, Serialize)]
pub struct ListCheckpointsQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub include_state: Option<bool>,
}

impl ListCheckpointsQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_state(mut self) -> Self {
        self.include_state = Some(true);
        self
    }
}

// =============================================================================
// Response Types
// =============================================================================

/// Result of creating a session
#[derive(Debug, Clone)]
pub struct CreateSessionResult {
    pub session_id: Uuid,
    pub checkpoint: Checkpoint,
}

/// Result of listing sessions
#[derive(Debug, Clone)]
pub struct ListSessionsResult {
    pub sessions: Vec<SessionSummary>,
    pub total: Option<u32>,
}

/// Result of listing checkpoints
#[derive(Debug, Clone)]
pub struct ListCheckpointsResult {
    pub checkpoints: Vec<CheckpointSummary>,
    pub total: Option<u32>,
}

// =============================================================================
// Stats Types
// =============================================================================

/// Session statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStats {
    pub total_sessions: u32,
    pub total_tool_calls: u32,
    pub successful_tool_calls: u32,
    pub failed_tool_calls: u32,
    pub aborted_tool_calls: u32,
    pub sessions_with_activity: u32,
    pub total_time_saved_seconds: Option<u32>,
    pub tools_usage: Vec<ToolUsageStats>,
}

/// Tool usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsageStats {
    pub tool_name: String,
    pub display_name: String,
    pub usage_counts: ToolUsageCounts,
    pub time_saved_per_call: Option<f64>,
    pub time_saved_seconds: Option<u32>,
}

/// Tool usage counts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsageCounts {
    pub total: u32,
    pub successful: u32,
    pub failed: u32,
    pub aborted: u32,
}
