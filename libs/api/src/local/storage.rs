//! Local SQLite storage implementation
//!
//! Implements SessionStorage using local SQLite database.

use crate::storage::{
    Checkpoint, CheckpointState, CheckpointSummary, CreateCheckpointRequest, CreateSessionRequest,
    CreateSessionResult, ListCheckpointsQuery, ListCheckpointsResult, ListSessionsQuery,
    ListSessionsResult, Session, SessionStatus, SessionStorage, SessionSummary, SessionVisibility,
    StorageError, UpdateSessionRequest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use libsql::Connection;
use std::path::Path;
use std::str::FromStr;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Local SQLite storage implementation
///
/// Uses a `Mutex<Connection>` to ensure thread-safe access to the
/// underlying libsql connection, which is not `Send + Sync` by default.
pub struct LocalStorage {
    conn: Mutex<Connection>,
}

impl LocalStorage {
    /// Create a new local storage instance
    pub async fn new(db_path: &str) -> Result<Self, StorageError> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                StorageError::Connection(format!("Failed to create database directory: {}", e))
            })?;
        }

        let db = libsql::Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| StorageError::Connection(format!("Failed to open database: {}", e)))?;

        let conn = db.connect().map_err(|e| {
            StorageError::Connection(format!("Failed to connect to database: {}", e))
        })?;

        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_schema().await?;

        Ok(storage)
    }

    /// Create from existing connection (for use with AgentClient)
    ///
    /// This will initialize the database schema if it hasn't been set up yet.
    pub async fn from_connection(conn: Connection) -> Result<Self, StorageError> {
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_schema().await?;
        Ok(storage)
    }

    /// Initialize database schema by running migrations
    async fn init_schema(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().await;
        super::migrations::run_migrations(&conn)
            .await
            .map_err(StorageError::Internal)
    }

    /// Get connection reference (locked)
    ///
    /// Useful for direct SQL access in tests or migrations.
    pub fn connection(&self) -> &Mutex<Connection> {
        &self.conn
    }

    /// Get the latest checkpoint for a session
    ///
    /// Expects the caller to already hold the connection lock.
    async fn get_latest_checkpoint_for_session_inner(
        conn: &Connection,
        session_id: Uuid,
    ) -> Result<Checkpoint, StorageError> {
        let mut rows = conn
            .query(
                "SELECT id, session_id, parent_id, state, created_at, updated_at FROM checkpoints 
                 WHERE session_id = ? ORDER BY created_at DESC LIMIT 1",
                [session_id.to_string()],
            )
            .await
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            let id: String = row
                .get(0)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let session_id: String = row
                .get(1)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let parent_id: Option<String> = row.get(2).ok();
            let state: Option<String> = row.get(3).ok();
            let created_at: String = row
                .get(4)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let updated_at: String = row
                .get(5)
                .map_err(|e| StorageError::Internal(e.to_string()))?;

            let state: CheckpointState = if let Some(state_str) = state {
                serde_json::from_str(&state_str).unwrap_or_default()
            } else {
                CheckpointState::default()
            };

            Ok(Checkpoint {
                id: Uuid::from_str(&id).map_err(|e| StorageError::Internal(e.to_string()))?,
                session_id: Uuid::from_str(&session_id)
                    .map_err(|e| StorageError::Internal(e.to_string()))?,
                parent_id: parent_id.and_then(|id| Uuid::from_str(&id).ok()),
                state,
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            })
        } else {
            Err(StorageError::NotFound(format!(
                "No checkpoints found for session {}",
                session_id
            )))
        }
    }
}

#[async_trait]
impl SessionStorage for LocalStorage {
    async fn list_sessions(
        &self,
        query: &ListSessionsQuery,
    ) -> Result<ListSessionsResult, StorageError> {
        let limit = query.limit.unwrap_or(100);
        let offset = query.offset.unwrap_or(0);

        let mut sql = "SELECT s.id, s.title, s.visibility, COALESCE(s.status, 'ACTIVE') as status, s.cwd, s.created_at, s.updated_at, 
            (SELECT COUNT(*) FROM checkpoints c WHERE c.session_id = s.id) as checkpoint_count,
            (SELECT id FROM checkpoints c WHERE c.session_id = s.id ORDER BY created_at DESC LIMIT 1) as active_checkpoint_id
            FROM sessions s WHERE 1=1".to_string();

        // Use parameterized values for enum filters (safe because they come from
        // our own Display impls, but we keep this consistent with the rest of
        // the codebase).  The search term is the only free-form user input and
        // is handled with a parameter below.
        if let Some(status) = &query.status {
            sql.push_str(&format!(" AND s.status = '{}'", status));
        }
        if let Some(visibility) = &query.visibility {
            sql.push_str(&format!(" AND s.visibility = '{}'", visibility));
        }
        if query.search.is_some() {
            sql.push_str(" AND s.title LIKE '%' || ? || '%'");
        }

        sql.push_str(&format!(
            " ORDER BY s.updated_at DESC LIMIT {} OFFSET {}",
            limit, offset
        ));

        let conn = self.conn.lock().await;
        let mut rows = if let Some(search) = &query.search {
            conn.query(&sql, [search.as_str()])
                .await
                .map_err(|e| StorageError::Internal(e.to_string()))?
        } else {
            conn.query(&sql, ())
                .await
                .map_err(|e| StorageError::Internal(e.to_string()))?
        };

        let mut sessions = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let id: String = row
                .get(0)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let title: String = row
                .get(1)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let visibility: String = row
                .get(2)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let status: String = row
                .get(3)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let cwd: Option<String> = row.get(4).ok();
            let created_at: String = row
                .get(5)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let updated_at: String = row
                .get(6)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let checkpoint_count: i64 = row.get(7).unwrap_or(0);
            let active_checkpoint_id: Option<String> = row.get(8).ok();

            sessions.push(SessionSummary {
                id: Uuid::from_str(&id).map_err(|e| StorageError::Internal(e.to_string()))?,
                title,
                visibility: parse_visibility(&visibility),
                status: parse_status(&status),
                cwd,
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
                message_count: checkpoint_count as u32,
                active_checkpoint_id: active_checkpoint_id.and_then(|id| Uuid::from_str(&id).ok()),
                last_message_at: None,
            });
        }

        Ok(ListSessionsResult {
            sessions,
            total: None,
        })
    }

    async fn get_session(&self, session_id: Uuid) -> Result<Session, StorageError> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT id, title, visibility, COALESCE(status, 'ACTIVE') as status, cwd, created_at, updated_at FROM sessions WHERE id = ?",
                [session_id.to_string()],
            )
            .await
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            let id: String = row
                .get(0)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let title: String = row
                .get(1)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let visibility: String = row
                .get(2)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let status: String = row
                .get(3)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let cwd: Option<String> = row.get(4).ok();
            let created_at: String = row
                .get(5)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let updated_at: String = row
                .get(6)
                .map_err(|e| StorageError::Internal(e.to_string()))?;

            // Get the latest checkpoint (reuse the same lock)
            let active_checkpoint =
                Self::get_latest_checkpoint_for_session_inner(&conn, session_id)
                    .await
                    .ok();

            Ok(Session {
                id: Uuid::from_str(&id).map_err(|e| StorageError::Internal(e.to_string()))?,
                title,
                visibility: parse_visibility(&visibility),
                status: parse_status(&status),
                cwd,
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
                active_checkpoint,
            })
        } else {
            Err(StorageError::NotFound(format!(
                "Session {} not found",
                session_id
            )))
        }
    }

    async fn create_session(
        &self,
        request: &CreateSessionRequest,
    ) -> Result<CreateSessionResult, StorageError> {
        let now = Utc::now();
        let session_id = Uuid::new_v4();
        let checkpoint_id = Uuid::new_v4();

        let conn = self.conn.lock().await;

        // Create session
        conn.execute(
            "INSERT INTO sessions (id, title, visibility, status, cwd, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                session_id.to_string(),
                request.title.as_str(),
                request.visibility.to_string(),
                "ACTIVE",
                request.cwd.as_deref(),
                now.to_rfc3339(),
                now.to_rfc3339(),
            ),
        )
        .await
        .map_err(|e| StorageError::Internal(e.to_string()))?;

        // Create initial checkpoint
        let state_json = serde_json::to_string(&request.initial_state)
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        conn.execute(
            "INSERT INTO checkpoints (id, session_id, parent_id, state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            (
                checkpoint_id.to_string(),
                session_id.to_string(),
                None::<String>,
                state_json,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ),
        )
        .await
        .map_err(|e| StorageError::Internal(e.to_string()))?;

        Ok(CreateSessionResult {
            session_id,
            checkpoint: Checkpoint {
                id: checkpoint_id,
                session_id,
                parent_id: None,
                state: request.initial_state.clone(),
                created_at: now,
                updated_at: now,
            },
        })
    }

    async fn update_session(
        &self,
        session_id: Uuid,
        request: &UpdateSessionRequest,
    ) -> Result<Session, StorageError> {
        let now = Utc::now();

        {
            let conn = self.conn.lock().await;

            // Update fields individually since libsql doesn't support dynamic params easily
            if let Some(title) = &request.title {
                conn.execute(
                    "UPDATE sessions SET title = ?, updated_at = ? WHERE id = ?",
                    (title.as_str(), now.to_rfc3339(), session_id.to_string()),
                )
                .await
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            }
            if let Some(visibility) = &request.visibility {
                conn.execute(
                    "UPDATE sessions SET visibility = ?, updated_at = ? WHERE id = ?",
                    (
                        visibility.to_string(),
                        now.to_rfc3339(),
                        session_id.to_string(),
                    ),
                )
                .await
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            }
        }

        self.get_session(session_id).await
    }

    async fn delete_session(&self, session_id: Uuid) -> Result<(), StorageError> {
        // Mark as deleted instead of actually deleting
        let now = Utc::now();
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE sessions SET status = 'DELETED', updated_at = ? WHERE id = ?",
            (now.to_rfc3339(), session_id.to_string()),
        )
        .await
        .map_err(|e| StorageError::Internal(e.to_string()))?;
        Ok(())
    }

    async fn list_checkpoints(
        &self,
        session_id: Uuid,
        query: &ListCheckpointsQuery,
    ) -> Result<ListCheckpointsResult, StorageError> {
        let limit = query.limit.unwrap_or(100);
        let offset = query.offset.unwrap_or(0);

        let sql = format!(
            "SELECT id, session_id, parent_id, state, created_at, updated_at FROM checkpoints 
             WHERE session_id = ? ORDER BY created_at ASC LIMIT {} OFFSET {}",
            limit, offset
        );

        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(&sql, [session_id.to_string()])
            .await
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        let mut checkpoints = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let id: String = row
                .get(0)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let session_id: String = row
                .get(1)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let parent_id: Option<String> = row.get(2).ok();
            let state: Option<String> = row.get(3).ok();
            let created_at: String = row
                .get(4)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let updated_at: String = row
                .get(5)
                .map_err(|e| StorageError::Internal(e.to_string()))?;

            let state: CheckpointState = if let Some(state_str) = state {
                serde_json::from_str(&state_str).unwrap_or_default()
            } else {
                CheckpointState::default()
            };

            checkpoints.push(CheckpointSummary {
                id: Uuid::from_str(&id).map_err(|e| StorageError::Internal(e.to_string()))?,
                session_id: Uuid::from_str(&session_id)
                    .map_err(|e| StorageError::Internal(e.to_string()))?,
                parent_id: parent_id.and_then(|id| Uuid::from_str(&id).ok()),
                message_count: state.messages.len() as u32,
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            });
        }

        Ok(ListCheckpointsResult {
            checkpoints,
            total: None,
        })
    }

    async fn get_checkpoint(&self, checkpoint_id: Uuid) -> Result<Checkpoint, StorageError> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT id, session_id, parent_id, state, created_at, updated_at FROM checkpoints WHERE id = ?",
                [checkpoint_id.to_string()],
            )
            .await
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            let id: String = row
                .get(0)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let session_id: String = row
                .get(1)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let parent_id: Option<String> = row.get(2).ok();
            let state: Option<String> = row.get(3).ok();
            let created_at: String = row
                .get(4)
                .map_err(|e| StorageError::Internal(e.to_string()))?;
            let updated_at: String = row
                .get(5)
                .map_err(|e| StorageError::Internal(e.to_string()))?;

            let state: CheckpointState = if let Some(state_str) = state {
                serde_json::from_str(&state_str).unwrap_or_default()
            } else {
                CheckpointState::default()
            };

            Ok(Checkpoint {
                id: Uuid::from_str(&id).map_err(|e| StorageError::Internal(e.to_string()))?,
                session_id: Uuid::from_str(&session_id)
                    .map_err(|e| StorageError::Internal(e.to_string()))?,
                parent_id: parent_id.and_then(|id| Uuid::from_str(&id).ok()),
                state,
                created_at: parse_datetime(&created_at)?,
                updated_at: parse_datetime(&updated_at)?,
            })
        } else {
            Err(StorageError::NotFound(format!(
                "Checkpoint {} not found",
                checkpoint_id
            )))
        }
    }

    async fn create_checkpoint(
        &self,
        session_id: Uuid,
        request: &CreateCheckpointRequest,
    ) -> Result<Checkpoint, StorageError> {
        let now = Utc::now();
        let checkpoint_id = Uuid::new_v4();

        let state_json = serde_json::to_string(&request.state)
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO checkpoints (id, session_id, parent_id, state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            (
                checkpoint_id.to_string(),
                session_id.to_string(),
                request.parent_id.map(|id| id.to_string()),
                state_json,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ),
        )
        .await
        .map_err(|e| StorageError::Internal(e.to_string()))?;

        // Update session's updated_at
        conn.execute(
            "UPDATE sessions SET updated_at = ? WHERE id = ?",
            (now.to_rfc3339(), session_id.to_string()),
        )
        .await
        .map_err(|e| StorageError::Internal(e.to_string()))?;

        Ok(Checkpoint {
            id: checkpoint_id,
            session_id,
            parent_id: request.parent_id,
            state: request.state.clone(),
            created_at: now,
            updated_at: now,
        })
    }
}

// Helper functions
fn parse_visibility(s: &str) -> SessionVisibility {
    match s.to_uppercase().as_str() {
        "PUBLIC" => SessionVisibility::Public,
        _ => SessionVisibility::Private,
    }
}

fn parse_status(s: &str) -> SessionStatus {
    match s.to_uppercase().as_str() {
        "DELETED" => SessionStatus::Deleted,
        _ => SessionStatus::Active,
    }
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| StorageError::Internal(format!("Failed to parse datetime: {}", e)))
}
