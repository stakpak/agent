//! SQLite database layer for autopilot state and schedule run history.
//!
//! Uses libsql for async SQLite operations.

use chrono::{DateTime, Utc};
use libsql::{Connection, Database};
use std::path::Path;

/// Run status for schedule executions.
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

/// A schedule run record.
#[derive(Debug, Clone)]
pub struct ScheduleRun {
    pub id: i64,
    pub schedule_name: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub check_exit_code: Option<i32>,
    pub check_stdout: Option<String>,
    pub check_stderr: Option<String>,
    pub check_timed_out: bool,
    pub agent_woken: bool,
    pub interactive_delegated: bool,
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
pub struct SchedulerState {
    pub started_at: DateTime<Utc>,
    pub pid: i64,
    pub last_heartbeat: DateTime<Utc>,
}

/// Filter options for listing runs.
#[derive(Debug, Default)]
pub struct ListRunsFilter {
    pub schedule_name: Option<String>,
    pub status: Option<RunStatus>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Well-known sentinel name used to request in-process schedule config reload.
pub const RELOAD_SENTINEL: &str = "__config_reload__";

/// Informational note stored on runs delegated to gateway interactive sessions.
pub const INTERACTIVE_DELEGATED_NOTE: &str = "Delegated to interactive gateway session";

/// A pending schedule request (for manual schedule fires).
#[derive(Debug, Clone)]
pub struct PendingSchedule {
    pub id: i64,
    pub schedule_name: String,
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
pub struct ScheduleDb {
    /// Keep the libsql Database handle alive for the lifetime of each operation connection.
    db: Database,
}

impl ScheduleDb {
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

        let storage = Self { db };
        storage.configure_pragmas().await?;
        storage.init_schema().await?;

        Ok(storage)
    }

    async fn configure_pragmas(&self) -> Result<(), DbError> {
        let conn = self.connection()?;
        // journal_mode returns a result row, so use query() instead of execute()
        conn.query("PRAGMA journal_mode = WAL", ())
            .await
            .map_err(|e| DbError::Query(format!("Failed to set journal_mode: {}", e)))?;
        conn.query("PRAGMA busy_timeout = 5000", ())
            .await
            .map_err(|e| DbError::Query(format!("Failed to set busy_timeout: {}", e)))?;
        conn.query("PRAGMA synchronous = NORMAL", ())
            .await
            .map_err(|e| DbError::Query(format!("Failed to set synchronous: {}", e)))?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, DbError> {
        self.db
            .connect()
            .map_err(|e| DbError::Connection(format!("Failed to connect to database: {}", e)))
    }

    /// Initialize database schema.
    async fn init_schema(&self) -> Result<(), DbError> {
        let conn = self.connection()?;

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
                interactive_delegated INTEGER NOT NULL DEFAULT 0,
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
        let _ = conn
            .execute(
                "ALTER TABLE trigger_runs ADD COLUMN interactive_delegated INTEGER NOT NULL DEFAULT 0",
                (),
            )
            .await;

        // Create autopilot_state table (singleton)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS autopilot_state (
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

    /// Check if a schedule already has a run in "running" status.
    pub async fn has_running_run(&self, schedule_name: &str) -> Result<bool, DbError> {
        let conn = self.connection()?;
        let status = RunStatus::Running.to_string();

        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM trigger_runs WHERE trigger_name = ? AND status = ?",
                (schedule_name, status.as_str()),
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        let row = rows
            .next()
            .await
            .map_err(|e| DbError::Query(e.to_string()))?
            .ok_or_else(|| DbError::NotFound("count query returned no rows".to_string()))?;

        let count: i64 = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
        Ok(count > 0)
    }

    /// Insert a new schedule run, returning the run ID.
    pub async fn insert_run(&self, schedule_name: &str) -> Result<i64, DbError> {
        let conn = self.connection()?;
        let now = Utc::now().to_rfc3339();
        let status = RunStatus::Running.to_string();

        conn.execute(
            "INSERT INTO trigger_runs (trigger_name, started_at, status, created_at) VALUES (?, ?, ?, ?)",
            (schedule_name, now.as_str(), status.as_str(), now.as_str()),
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
        let conn = self.connection()?;

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
        let conn = self.connection()?;

        conn.execute(
            "UPDATE trigger_runs SET agent_woken = 1, interactive_delegated = 0, agent_session_id = ? WHERE id = ?",
            (session_id, run_id),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Mark run as delegated to gateway interactive session while keeping status=running.
    pub async fn update_run_interactive_started(
        &self,
        run_id: i64,
        session_id: &str,
        note: &str,
    ) -> Result<(), DbError> {
        let conn = self.connection()?;

        conn.execute(
            "UPDATE trigger_runs SET agent_woken = 1, interactive_delegated = 1, agent_session_id = ?, error_message = ? WHERE id = ?",
            (session_id, note, run_id),
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
        let conn = self.connection()?;

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
        let conn = self.connection()?;
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
    pub async fn get_run(&self, run_id: i64) -> Result<ScheduleRun, DbError> {
        let conn = self.connection()?;

        let mut rows = conn
            .query(
                "SELECT id, trigger_name, started_at, finished_at, check_exit_code, check_stdout,
                        check_stderr, check_timed_out, agent_woken, interactive_delegated, agent_session_id,
                        agent_last_checkpoint_id, agent_stdout, agent_stderr, status, error_message, created_at
                 FROM trigger_runs WHERE id = ?",
                [run_id],
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            parse_schedule_run(&row)
        } else {
            Err(DbError::NotFound(format!("Run {} not found", run_id)))
        }
    }

    /// List runs with optional filters.
    pub async fn list_runs(&self, filter: &ListRunsFilter) -> Result<Vec<ScheduleRun>, DbError> {
        let conn = self.connection()?;

        let mut sql =
            "SELECT id, trigger_name, started_at, finished_at, check_exit_code, check_stdout,
                              check_stderr, check_timed_out, agent_woken, interactive_delegated, agent_session_id,
                              agent_last_checkpoint_id, agent_stdout, agent_stderr, status, error_message, created_at
                       FROM trigger_runs WHERE 1=1"
                .to_string();

        let mut params: Vec<String> = Vec::new();

        if let Some(name) = &filter.schedule_name {
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
            runs.push(parse_schedule_run(&row)?);
        }

        Ok(runs)
    }

    /// Delete runs older than the specified number of days.
    pub async fn prune_runs(&self, older_than_days: u32) -> Result<u64, DbError> {
        let conn = self.connection()?;

        let result = conn
            .execute(
                "DELETE FROM trigger_runs WHERE created_at < datetime('now', ?)",
                [format!("-{} days", older_than_days)],
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(result)
    }

    /// Mark all stale "running" runs as failed.
    /// Runs are considered stale if they've been running and the autopilot service is no longer active.
    pub async fn clean_stale_runs(&self) -> Result<u64, DbError> {
        let conn = self.connection()?;
        let now = Utc::now().to_rfc3339();

        let result = conn
            .execute(
                "UPDATE trigger_runs SET status = 'failed', finished_at = ?, error_message = 'Marked as failed: autopilot was stopped while run was in progress' WHERE status = 'running'",
                [now.as_str()],
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(result)
    }

    /// Set autopilot state (upsert).
    pub async fn set_autopilot_state(&self, pid: i64) -> Result<(), DbError> {
        let conn = self.connection()?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO autopilot_state (id, started_at, pid, last_heartbeat) VALUES (1, ?, ?, ?)",
            (now.as_str(), pid, now.as_str()),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Update autopilot heartbeat.
    pub async fn update_heartbeat(&self) -> Result<(), DbError> {
        let conn = self.connection()?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE autopilot_state SET last_heartbeat = ? WHERE id = 1",
            [now.as_str()],
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Get autopilot state.
    pub async fn get_autopilot_state(&self) -> Result<Option<SchedulerState>, DbError> {
        let conn = self.connection()?;

        let mut rows = conn
            .query(
                "SELECT started_at, pid, last_heartbeat FROM autopilot_state WHERE id = 1",
                (),
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        if let Ok(Some(row)) = rows.next().await {
            let started_at: String = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
            let pid: i64 = row.get(1).map_err(|e| DbError::Query(e.to_string()))?;
            let last_heartbeat: String = row.get(2).map_err(|e| DbError::Query(e.to_string()))?;

            Ok(Some(SchedulerState {
                started_at: parse_datetime(&started_at)?,
                pid,
                last_heartbeat: parse_datetime(&last_heartbeat)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Clear autopilot state.
    pub async fn clear_autopilot_state(&self) -> Result<(), DbError> {
        let conn = self.connection()?;

        conn.execute("DELETE FROM autopilot_state WHERE id = 1", ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Insert a pending schedule request (for manual schedule fires).
    pub async fn insert_pending_schedule(&self, schedule_name: &str) -> Result<i64, DbError> {
        let conn = self.connection()?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO pending_triggers (trigger_name, created_at) VALUES (?, ?)",
            (schedule_name, now.as_str()),
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

    /// Request a scheduler config reload using the pending_triggers signal queue.
    pub async fn request_config_reload(&self) -> Result<(), DbError> {
        let conn = self.connection()?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO pending_triggers (trigger_name, created_at)
             SELECT ?, ?
             WHERE NOT EXISTS (
                 SELECT 1 FROM pending_triggers WHERE trigger_name = ?
             )",
            (RELOAD_SENTINEL, now.as_str(), RELOAD_SENTINEL),
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Get and delete all pending schedules (atomic pop).
    pub async fn pop_pending_schedules(&self) -> Result<Vec<PendingSchedule>, DbError> {
        let conn = self.connection()?;

        let mut rows = conn
            .query(
                "DELETE FROM pending_triggers
                 WHERE id IN (
                     SELECT id FROM pending_triggers ORDER BY created_at ASC
                 )
                 RETURNING id, trigger_name, created_at",
                (),
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        let mut schedules = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let id: i64 = match row.get(0) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let schedule_name: String = match row.get(1) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let created_at_raw: String = match row.get(2) {
                Ok(value) => value,
                Err(_) => continue,
            };

            let created_at = parse_datetime(&created_at_raw).unwrap_or_else(|_| Utc::now());

            schedules.push(PendingSchedule {
                id,
                schedule_name,
                created_at,
            });
        }

        schedules.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        Ok(schedules)
    }
}

/// Parse a row into a ScheduleRun.
fn parse_schedule_run(row: &libsql::Row) -> Result<ScheduleRun, DbError> {
    let id: i64 = row.get(0).map_err(|e| DbError::Query(e.to_string()))?;
    let schedule_name: String = row.get(1).map_err(|e| DbError::Query(e.to_string()))?;
    let started_at: String = row.get(2).map_err(|e| DbError::Query(e.to_string()))?;
    let finished_at: Option<String> = row.get(3).ok();
    let check_exit_code: Option<i32> = row.get(4).ok();
    let check_stdout: Option<String> = row.get(5).ok();
    let check_stderr: Option<String> = row.get(6).ok();
    let check_timed_out: i32 = row.get(7).unwrap_or(0);
    let agent_woken: i32 = row.get(8).unwrap_or(0);
    let interactive_delegated: i32 = row.get(9).unwrap_or(0);
    let agent_session_id: Option<String> = row.get(10).ok();
    let agent_last_checkpoint_id: Option<String> = row.get(11).ok();
    let agent_stdout: Option<String> = row.get(12).ok();
    let agent_stderr: Option<String> = row.get(13).ok();
    let status: String = row.get(14).map_err(|e| DbError::Query(e.to_string()))?;
    let error_message: Option<String> = row.get(15).ok();
    let created_at: String = row.get(16).map_err(|e| DbError::Query(e.to_string()))?;

    Ok(ScheduleRun {
        id,
        schedule_name,
        started_at: parse_datetime(&started_at)?,
        finished_at: finished_at.map(|s| parse_datetime(&s)).transpose()?,
        check_exit_code,
        check_stdout,
        check_stderr,
        check_timed_out: check_timed_out != 0,
        agent_woken: agent_woken != 0,
        interactive_delegated: interactive_delegated != 0,
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

#[cfg(all(test, feature = "libsql-test"))]
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    async fn create_test_db() -> (ScheduleDb, TempDir) {
        let dir = tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let db = ScheduleDb::new(db_path.to_str().expect("Invalid path"))
            .await
            .expect("Failed to create test db");
        (db, dir)
    }

    #[tokio::test]
    async fn test_init_creates_tables() {
        let dir = tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test.db");

        let db = ScheduleDb::new(db_path.to_str().expect("Invalid path"))
            .await
            .expect("Failed to create db");

        // Verify tables exist by querying them
        let conn = db.connection().expect("Failed to open connection");

        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('trigger_runs', 'autopilot_state')",
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
        assert!(tables.contains(&"autopilot_state".to_string()));
    }

    #[tokio::test]
    async fn test_insert_and_get_run() {
        let (db, _dir) = create_test_db().await;

        let run_id = db.insert_run("test-schedule").await.expect("Insert failed");
        assert!(run_id > 0);

        let run = db.get_run(run_id).await.expect("Get failed");
        assert_eq!(run.id, run_id);
        assert_eq!(run.schedule_name, "test-schedule");
        assert_eq!(run.status, RunStatus::Running);
        assert!(!run.agent_woken);
    }

    #[tokio::test]
    async fn test_update_run_status() {
        let (db, _dir) = create_test_db().await;

        let run_id = db.insert_run("test-schedule").await.expect("Insert failed");

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
        let id1 = db.insert_run("schedule-a").await.expect("Insert failed");
        let _id2 = db.insert_run("schedule-b").await.expect("Insert failed");
        let _id3 = db.insert_run("schedule-a").await.expect("Insert failed");

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

        // Filter by schedule name
        let runs = db
            .list_runs(&ListRunsFilter {
                schedule_name: Some("schedule-a".to_string()),
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
        db.insert_run("test-schedule").await.expect("Insert failed");

        // Prune runs older than 0 days (should delete the run we just created)
        // Note: This test is a bit tricky because the run was just created
        // In practice, we'd need to manipulate timestamps for a proper test
        let deleted = db.prune_runs(0).await.expect("Prune failed");
        // The run was just created, so it shouldn't be deleted with 0 days
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn test_autopilot_state_lifecycle() {
        let (db, _dir) = create_test_db().await;

        // Initially no state
        let state = db.get_autopilot_state().await.expect("Get state failed");
        assert!(state.is_none());

        // Set state
        db.set_autopilot_state(12345)
            .await
            .expect("Set state failed");

        let state = db
            .get_autopilot_state()
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
            .get_autopilot_state()
            .await
            .expect("Get state failed")
            .expect("State should exist");
        assert!(state2.last_heartbeat >= state.last_heartbeat);

        // Clear state
        db.clear_autopilot_state()
            .await
            .expect("Clear state failed");

        let state = db.get_autopilot_state().await.expect("Get state failed");
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

        assert_eq!(
            "running"
                .parse::<RunStatus>()
                .expect("failed to parse running status"),
            RunStatus::Running
        );
        assert_eq!(
            "COMPLETED"
                .parse::<RunStatus>()
                .expect("failed to parse completed status"),
            RunStatus::Completed
        );
        assert!("invalid".parse::<RunStatus>().is_err());
    }

    #[tokio::test]
    async fn test_pending_schedules() {
        let (db, _dir) = create_test_db().await;

        // Initially no pending schedules
        let pending = db.pop_pending_schedules().await.expect("Pop failed");
        assert!(pending.is_empty());

        // Insert some pending schedules
        let id1 = db
            .insert_pending_schedule("schedule-a")
            .await
            .expect("Insert failed");
        let id2 = db
            .insert_pending_schedule("schedule-b")
            .await
            .expect("Insert failed");
        let id3 = db
            .insert_pending_schedule("schedule-a")
            .await
            .expect("Insert failed");

        assert!(id1 > 0);
        assert!(id2 > id1);
        assert!(id3 > id2);

        // Pop should return all and delete them
        let pending = db.pop_pending_schedules().await.expect("Pop failed");
        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].schedule_name, "schedule-a");
        assert_eq!(pending[1].schedule_name, "schedule-b");
        assert_eq!(pending[2].schedule_name, "schedule-a");

        // Second pop should return empty
        let pending = db.pop_pending_schedules().await.expect("Pop failed");
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_config_reload_signal_roundtrip() {
        let (db, _dir) = create_test_db().await;

        db.request_config_reload()
            .await
            .expect("requesting config reload should succeed");

        let pending = db
            .pop_pending_schedules()
            .await
            .expect("pop should succeed");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].schedule_name, RELOAD_SENTINEL);
    }

    #[tokio::test]
    async fn test_config_reload_signal_is_deduplicated() {
        let (db, _dir) = create_test_db().await;

        db.request_config_reload()
            .await
            .expect("first reload request should succeed");
        db.request_config_reload()
            .await
            .expect("second reload request should succeed");

        let pending = db
            .pop_pending_schedules()
            .await
            .expect("pop should succeed");

        let count = pending
            .iter()
            .filter(|item| item.schedule_name == RELOAD_SENTINEL)
            .count();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_config_reload_sentinel_not_confused_with_real_trigger() {
        let (db, _dir) = create_test_db().await;

        db.request_config_reload()
            .await
            .expect("requesting config reload should succeed");
        db.insert_pending_schedule("real-schedule")
            .await
            .expect("inserting real schedule should succeed");

        let pending = db
            .pop_pending_schedules()
            .await
            .expect("pop should succeed");
        assert_eq!(pending.len(), 2);
        assert!(
            pending
                .iter()
                .any(|item| item.schedule_name == RELOAD_SENTINEL)
        );
        assert!(
            pending
                .iter()
                .any(|item| item.schedule_name == "real-schedule")
        );
    }

    #[tokio::test]
    async fn test_pending_schedules_tolerates_malformed_created_at() {
        let (db, _dir) = create_test_db().await;
        let conn = db.connection().expect("Failed to open connection");

        conn.execute(
            "INSERT INTO pending_triggers (trigger_name, created_at) VALUES (?, ?)",
            ("schedule-a", "not-a-timestamp"),
        )
        .await
        .expect("insert malformed pending trigger failed");

        let pending = db.pop_pending_schedules().await.expect("Pop failed");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].schedule_name, "schedule-a");
    }
}
