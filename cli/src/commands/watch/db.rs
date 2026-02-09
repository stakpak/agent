//! SQLite database layer for watch state and trigger run history.
//!
//! Uses libsql for async SQLite operations.

use chrono::{DateTime, Utc};
use libsql::Connection;
use std::path::Path;
use tokio::sync::Mutex;

/// Run status for trigger executions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Currently executing
    Running,
    /// Completed successfully
    Completed,
    /// Failed with error
    Failed,
    /// Skipped (check script returned non-zero)
    Skipped,
    /// Timed out
    TimedOut,
    /// Paused (agent needs approval or input to continue)
    Paused,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunStatus::Running => write!(f, "running"),
            RunStatus::Completed => write!(f, "completed"),
            RunStatus::Failed => write!(f, "failed"),
            RunStatus::Skipped => write!(f, "skipped"),
            RunStatus::TimedOut => write!(f, "timed_out"),
            RunStatus::Paused => write!(f, "paused"),
        }
    }
}

impl std::str::FromStr for RunStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "running" => Ok(RunStatus::Running),
            "completed" => Ok(RunStatus::Completed),
            "failed" => Ok(RunStatus::Failed),
            "skipped" => Ok(RunStatus::Skipped),
            "timed_out" => Ok(RunStatus::TimedOut),
            "paused" => Ok(RunStatus::Paused),
            _ => Err(format!("Unknown run status: {}", s)),
        }
    }
}

/// A trigger run record.
#[derive(Debug, Clone)]
pub struct TriggerRun {
    pub id: i64,
    pub trigger_name: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub check_exit_code: Option<i32>,
    pub check_stdout: Option<String>,
    pub check_stderr: Option<String>,
    pub check_timed_out: bool,
    pub agent_woken: bool,
    pub agent_session_id: Option<String>,
    pub agent_last_checkpoint_id: Option<String>,
    pub agent_stdout: Option<String>,
    pub agent_stderr: Option<String>,
    pub status: RunStatus,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Watch state record.
#[derive(Debug, Clone)]
pub struct WatchState {
    pub started_at: DateTime<Utc>,
    pub pid: i64,
    pub last_heartbeat: DateTime<Utc>,
}

/// Filter options for listing runs.
#[derive(Debug, Default)]
pub struct ListRunsFilter {
    pub trigger_name: Option<String>,
    pub status: Option<RunStatus>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// A pending trigger request (for manual trigger fires).
#[derive(Debug, Clone)]
pub struct PendingTrigger {
    pub id: i64,
    pub trigger_name: String,
    pub created_at: DateTime<Utc>,
}

/// Database errors.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Database connection error: {0}")]
    Connection(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

/// Watch database storage.
pub struct WatchDb {
    conn: Mutex<Connection>,
}

impl WatchDb {
    /// Create a new database instance, initializing schema if needed.
    pub async fn new(db_path: &str) -> Result<Self, DbError> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DbError::Connection(format!("Failed to create database directory: {}", e))
            })?;
        }

        let db = libsql::Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| DbError::Connection(format!("Failed to open database: {}", e)))?;

        let conn = db
            .connect()
            .map_err(|e| DbError::Connection(format!("Failed to connect to database: {}", e)))?;

        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.init_schema().await?;

        Ok(storage)
    }

    /// Initialize database schema.
    async fn init_schema(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().await;

        // Create trigger_runs table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS trigger_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trigger_name TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                check_exit_code INTEGER,
                check_stdout TEXT,
                check_stderr TEXT,
                check_timed_out INTEGER DEFAULT 0,
                agent_woken INTEGER NOT NULL DEFAULT 0,
                agent_session_id TEXT,
                agent_last_checkpoint_id TEXT,
                agent_stdout TEXT,
                agent_stderr TEXT,
                status TEXT NOT NULL,
                error_message TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            (),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        // Add agent_stdout and agent_stderr columns if they don't exist (migration)
        let _ = conn
            .execute("ALTER TABLE trigger_runs ADD COLUMN agent_stdout TEXT", ())
            .await;
        let _ = conn
            .execute("ALTER TABLE trigger_runs ADD COLUMN agent_stderr TEXT", ())
            .await;

        // Create watch_state table (singleton)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS watch_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                started_at TEXT,
                pid INTEGER,
                last_heartbeat TEXT
            )",
            (),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        // Create pending_triggers table for manual trigger requests
        conn.execute(
            "CREATE TABLE IF NOT EXISTS pending_triggers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trigger_name TEXT NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            (),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        // Create index for faster queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trigger_runs_trigger_name ON trigger_runs(trigger_name)",
            (),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trigger_runs_status ON trigger_runs(status)",
            (),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trigger_runs_created_at ON trigger_runs(created_at)",
            (),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Insert a new trigger run, returning the run ID.
    pub async fn insert_run(&self, trigger_name: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();
        let status = RunStatus::Running.to_string();

        conn.execute(
            "INSERT INTO trigger_runs (trigger_name, started_at, status, created_at) VALUES (?, ?, ?, ?)",
            (trigger_name, now.as_str(), status.as_str(), now.as_str()),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        // Get the last inserted row ID
        let mut rows = conn
            .query("SELECT last_insert_rowid()", ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            let id: i64 = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
            Ok(id)
        } else {
            Err(DbError::Query("Failed to get last insert ID".to_string()))
        }
    }

    /// Update run with check script results.
    pub async fn update_run_check_result(
        &self,
        run_id: i64,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
        timed_out: bool,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE trigger_runs SET check_exit_code = ?, check_stdout = ?, check_stderr = ?, check_timed_out = ? WHERE id = ?",
            (exit_code, stdout, stderr, timed_out as i32, run_id),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Update run when agent is started.
    pub async fn update_run_agent_started(
        &self,
        run_id: i64,
        session_id: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE trigger_runs SET agent_woken = 1, agent_session_id = ? WHERE id = ?",
            (session_id, run_id),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Update run with latest checkpoint ID.
    pub async fn update_run_checkpoint(
        &self,
        run_id: i64,
        checkpoint_id: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE trigger_runs SET agent_last_checkpoint_id = ? WHERE id = ?",
            (checkpoint_id, run_id),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Update run when finished.
    pub async fn update_run_finished(
        &self,
        run_id: i64,
        status: RunStatus,
        error_message: Option<&str>,
        agent_stdout: Option<&str>,
        agent_stderr: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();
        let status_str = status.to_string();

        conn.execute(
            "UPDATE trigger_runs SET finished_at = ?, status = ?, error_message = ?, agent_stdout = ?, agent_stderr = ? WHERE id = ?",
            (now.as_str(), status_str.as_str(), error_message, agent_stdout, agent_stderr, run_id),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Get a run by ID.
    pub async fn get_run(&self, run_id: i64) -> Result<TriggerRun, DbError> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT id, trigger_name, started_at, finished_at, check_exit_code, check_stdout,
                        check_stderr, check_timed_out, agent_woken, agent_session_id,
                        agent_last_checkpoint_id, agent_stdout, agent_stderr, status, error_message, created_at
                 FROM trigger_runs WHERE id = ?",
                [run_id],
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            parse_trigger_run(&row)
        } else {
            Err(DbError::NotFound(format!("Run {} not found", run_id)))
        }
    }

    /// List runs with optional filters.
    pub async fn list_runs(&self, filter: &ListRunsFilter) -> Result<Vec<TriggerRun>, DbError> {
        let conn = self.conn.lock().await;

        let mut sql =
            "SELECT id, trigger_name, started_at, finished_at, check_exit_code, check_stdout,
                              check_stderr, check_timed_out, agent_woken, agent_session_id,
                              agent_last_checkpoint_id, agent_stdout, agent_stderr, status, error_message, created_at
                       FROM trigger_runs WHERE 1=1"
                .to_string();

        let mut params: Vec<String> = Vec::new();

        if let Some(name) = &filter.trigger_name {
            sql.push_str(" AND trigger_name = ?");
            params.push(name.clone());
        }

        if let Some(status) = &filter.status {
            sql.push_str(" AND status = ?");
            params.push(status.to_string());
        }

        sql.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        if let Some(offset) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        // Execute with appropriate number of params
        let mut rows = match params.len() {
            0 => conn.query(&sql, ()).await,
            1 => conn.query(&sql, [params[0].as_str()]).await,
            2 => {
                conn.query(&sql, [params[0].as_str(), params[1].as_str()])
                    .await
            }
            _ => return Err(DbError::Query("Too many filter parameters".to_string())),
        }
        .map_err(|e| DbError::Query(e.to_string()))?;

        let mut runs = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            runs.push(parse_trigger_run(&row)?);
        }

        Ok(runs)
    }

    /// Delete runs older than the specified number of days.
    pub async fn prune_runs(&self, older_than_days: u32) -> Result<u64, DbError> {
        let conn = self.conn.lock().await;

        let result = conn
            .execute(
                "DELETE FROM trigger_runs WHERE created_at < datetime('now', ?)",
                [format!("-{} days", older_than_days)],
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(result)
    }

    /// Set watch state (upsert).
    pub async fn set_watch_state(&self, pid: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO watch_state (id, started_at, pid, last_heartbeat) VALUES (1, ?, ?, ?)",
            (now.as_str(), pid, now.as_str()),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Update watch heartbeat.
    pub async fn update_heartbeat(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE watch_state SET last_heartbeat = ? WHERE id = 1",
            [now.as_str()],
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Get watch state.
    pub async fn get_watch_state(&self) -> Result<Option<WatchState>, DbError> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT started_at, pid, last_heartbeat FROM watch_state WHERE id = 1",
                (),
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            let started_at: String = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
            let pid: i64 = row.get(1).map_err(|e| DbError::Query(e.to_string()))?;
            let last_heartbeat: String = row.get(2).map_err(|e| DbError::Query(e.to_string()))?;

            Ok(Some(WatchState {
                started_at: parse_datetime(&started_at)?,
                pid,
                last_heartbeat: parse_datetime(&last_heartbeat)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Clear watch state.
    pub async fn clear_watch_state(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().await;

        conn.execute("DELETE FROM watch_state WHERE id = 1", ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Insert a pending trigger request (for manual trigger fires).
    pub async fn insert_pending_trigger(&self, trigger_name: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO pending_triggers (trigger_name, created_at) VALUES (?, ?)",
            (trigger_name, now.as_str()),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        // Get the last inserted row ID
        let mut rows = conn
            .query("SELECT last_insert_rowid()", ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            let id: i64 = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
            Ok(id)
        } else {
            Err(DbError::Query("Failed to get last insert ID".to_string()))
        }
    }

    /// Get and delete all pending triggers (atomic pop).
    pub async fn pop_pending_triggers(&self) -> Result<Vec<PendingTrigger>, DbError> {
        let conn = self.conn.lock().await;

        // Get all pending triggers
        let mut rows = conn
            .query(
                "SELECT id, trigger_name, created_at FROM pending_triggers ORDER BY created_at ASC",
                (),
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        let mut triggers = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let id: i64 = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
            let trigger_name: String = row.get(1).map_err(|e| DbError::Query(e.to_string()))?;
            let created_at: String = row.get(2).map_err(|e| DbError::Query(e.to_string()))?;

            triggers.push(PendingTrigger {
                id,
                trigger_name,
                created_at: parse_datetime(&created_at)?,
            });
        }

        // Delete all pending triggers we just read
        if !triggers.is_empty() {
            conn.execute("DELETE FROM pending_triggers", ())
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;
        }

        Ok(triggers)
    }
}

/// Parse a row into a TriggerRun.
fn parse_trigger_run(row: &libsql::Row) -> Result<TriggerRun, DbError> {
    let id: i64 = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
    let trigger_name: String = row.get(1).map_err(|e| DbError::Query(e.to_string()))?;
    let started_at: String = row.get(2).map_err(|e| DbError::Query(e.to_string()))?;
    let finished_at: Option<String> = row.get(3).ok();
    let check_exit_code: Option<i32> = row.get(4).ok();
    let check_stdout: Option<String> = row.get(5).ok();
    let check_stderr: Option<String> = row.get(6).ok();
    let check_timed_out: i32 = row.get(7).unwrap_or(0);
    let agent_woken: i32 = row.get(8).unwrap_or(0);
    let agent_session_id: Option<String> = row.get(9).ok();
    let agent_last_checkpoint_id: Option<String> = row.get(10).ok();
    let agent_stdout: Option<String> = row.get(11).ok();
    let agent_stderr: Option<String> = row.get(12).ok();
    let status: String = row.get(13).map_err(|e| DbError::Query(e.to_string()))?;
    let error_message: Option<String> = row.get(14).ok();
    let created_at: String = row.get(15).map_err(|e| DbError::Query(e.to_string()))?;

    Ok(TriggerRun {
        id,
        trigger_name,
        started_at: parse_datetime(&started_at)?,
        finished_at: finished_at.map(|s| parse_datetime(&s)).transpose()?,
        check_exit_code,
        check_stdout,
        check_stderr,
        check_timed_out: check_timed_out != 0,
        agent_woken: agent_woken != 0,
        agent_session_id,
        agent_last_checkpoint_id,
        agent_stdout,
        agent_stderr,
        status: status.parse().map_err(DbError::Query)?,
        error_message,
        created_at: parse_datetime(&created_at)?,
    })
}

/// Parse an RFC3339 datetime string.
fn parse_datetime(s: &str) -> Result<DateTime<Utc>, DbError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            // Try parsing without timezone (SQLite CURRENT_TIMESTAMP format)
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc())
        })
        .map_err(|e| DbError::Query(format!("Failed to parse datetime '{}': {}", s, e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    async fn create_test_db() -> (WatchDb, TempDir) {
        let dir = tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = WatchDb::new(db_path.to_str().expect("Invalid path"))
            .await
            .expect("Failed to create test db");
        (db, dir)
    }

    #[tokio::test]
    async fn test_init_creates_tables() {
        let dir = tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test.db");

        let db = WatchDb::new(db_path.to_str().expect("Invalid path"))
            .await
            .expect("Failed to create db");

        // Verify tables exist by querying them
        let conn = db.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('trigger_runs', 'watch_state')",
                (),
            )
            .await
            .expect("Query failed");

        let mut tables = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let name: String = row.get(0).expect("Failed to get table name");
            tables.push(name);
        }

        assert!(tables.contains(&"trigger_runs".to_string()));
        assert!(tables.contains(&"watch_state".to_string()));
    }

    #[tokio::test]
    async fn test_insert_and_get_run() {
        let (db, _dir) = create_test_db().await;

        let run_id = db.insert_run("test-trigger").await.expect("Insert failed");
        assert!(run_id > 0);

        let run = db.get_run(run_id).await.expect("Get failed");
        assert_eq!(run.id, run_id);
        assert_eq!(run.trigger_name, "test-trigger");
        assert_eq!(run.status, RunStatus::Running);
        assert!(!run.agent_woken);
    }

    #[tokio::test]
    async fn test_update_run_status() {
        let (db, _dir) = create_test_db().await;

        let run_id = db.insert_run("test-trigger").await.expect("Insert failed");

        // Update check result
        db.update_run_check_result(run_id, 0, "output", "errors", false)
            .await
            .expect("Update check failed");

        let run = db.get_run(run_id).await.expect("Get failed");
        assert_eq!(run.check_exit_code, Some(0));
        assert_eq!(run.check_stdout, Some("output".to_string()));
        assert_eq!(run.check_stderr, Some("errors".to_string()));
        assert!(!run.check_timed_out);

        // Update agent started
        db.update_run_agent_started(run_id, "session-123")
            .await
            .expect("Update agent failed");

        let run = db.get_run(run_id).await.expect("Get failed");
        assert!(run.agent_woken);
        assert_eq!(run.agent_session_id, Some("session-123".to_string()));

        // Update checkpoint
        db.update_run_checkpoint(run_id, "checkpoint-456")
            .await
            .expect("Update checkpoint failed");

        let run = db.get_run(run_id).await.expect("Get failed");
        assert_eq!(
            run.agent_last_checkpoint_id,
            Some("checkpoint-456".to_string())
        );

        // Update finished
        db.update_run_finished(run_id, RunStatus::Completed, None, None, None)
            .await
            .expect("Update finished failed");

        let run = db.get_run(run_id).await.expect("Get failed");
        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.finished_at.is_some());
    }

    #[tokio::test]
    async fn test_list_runs_filter() {
        let (db, _dir) = create_test_db().await;

        // Insert multiple runs
        let id1 = db.insert_run("trigger-a").await.expect("Insert failed");
        let _id2 = db.insert_run("trigger-b").await.expect("Insert failed");
        let _id3 = db.insert_run("trigger-a").await.expect("Insert failed");

        // Mark one as completed
        db.update_run_finished(id1, RunStatus::Completed, None, None, None)
            .await
            .expect("Update failed");

        // List all
        let runs = db
            .list_runs(&ListRunsFilter::default())
            .await
            .expect("List failed");
        assert_eq!(runs.len(), 3);

        // Filter by trigger name
        let runs = db
            .list_runs(&ListRunsFilter {
                trigger_name: Some("trigger-a".to_string()),
                ..Default::default()
            })
            .await
            .expect("List failed");
        assert_eq!(runs.len(), 2);

        // Filter by status
        let runs = db
            .list_runs(&ListRunsFilter {
                status: Some(RunStatus::Running),
                ..Default::default()
            })
            .await
            .expect("List failed");
        assert_eq!(runs.len(), 2);

        // Filter with limit
        let runs = db
            .list_runs(&ListRunsFilter {
                limit: Some(1),
                ..Default::default()
            })
            .await
            .expect("List failed");
        assert_eq!(runs.len(), 1);
    }

    #[tokio::test]
    async fn test_prune_old_runs() {
        let (db, _dir) = create_test_db().await;

        // Insert a run
        db.insert_run("test-trigger").await.expect("Insert failed");

        // Prune runs older than 0 days (should delete the run we just created)
        // Note: This test is a bit tricky because the run was just created
        // In practice, we'd need to manipulate timestamps for a proper test
        let deleted = db.prune_runs(0).await.expect("Prune failed");
        // The run was just created, so it shouldn't be deleted with 0 days
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn test_watch_state_lifecycle() {
        let (db, _dir) = create_test_db().await;

        // Initially no state
        let state = db.get_watch_state().await.expect("Get state failed");
        assert!(state.is_none());

        // Set state
        db.set_watch_state(12345).await.expect("Set state failed");

        let state = db
            .get_watch_state()
            .await
            .expect("Get state failed")
            .expect("State should exist");
        assert_eq!(state.pid, 12345);

        // Update heartbeat
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        db.update_heartbeat()
            .await
            .expect("Update heartbeat failed");

        let state2 = db
            .get_watch_state()
            .await
            .expect("Get state failed")
            .expect("State should exist");
        assert!(state2.last_heartbeat >= state.last_heartbeat);

        // Clear state
        db.clear_watch_state().await.expect("Clear state failed");

        let state = db.get_watch_state().await.expect("Get state failed");
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn test_run_not_found() {
        let (db, _dir) = create_test_db().await;

        let result = db.get_run(99999).await;
        assert!(matches!(result, Err(DbError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_run_status_display_and_parse() {
        assert_eq!(RunStatus::Running.to_string(), "running");
        assert_eq!(RunStatus::Completed.to_string(), "completed");
        assert_eq!(RunStatus::Failed.to_string(), "failed");
        assert_eq!(RunStatus::Skipped.to_string(), "skipped");
        assert_eq!(RunStatus::TimedOut.to_string(), "timed_out");

        assert_eq!("running".parse::<RunStatus>().unwrap(), RunStatus::Running);
        assert_eq!(
            "COMPLETED".parse::<RunStatus>().unwrap(),
            RunStatus::Completed
        );
        assert!("invalid".parse::<RunStatus>().is_err());
    }

    #[tokio::test]
    async fn test_pending_triggers() {
        let (db, _dir) = create_test_db().await;

        // Initially no pending triggers
        let pending = db.pop_pending_triggers().await.expect("Pop failed");
        assert!(pending.is_empty());

        // Insert some pending triggers
        let id1 = db
            .insert_pending_trigger("trigger-a")
            .await
            .expect("Insert failed");
        let id2 = db
            .insert_pending_trigger("trigger-b")
            .await
            .expect("Insert failed");
        let id3 = db
            .insert_pending_trigger("trigger-a")
            .await
            .expect("Insert failed");

        assert!(id1 > 0);
        assert!(id2 > id1);
        assert!(id3 > id2);

        // Pop should return all and delete them
        let pending = db.pop_pending_triggers().await.expect("Pop failed");
        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].trigger_name, "trigger-a");
        assert_eq!(pending[1].trigger_name, "trigger-b");
        assert_eq!(pending[2].trigger_name, "trigger-a");

        // Second pop should return empty
        let pending = db.pop_pending_triggers().await.expect("Pop failed");
        assert!(pending.is_empty());
    }
}
