//! Shared SQLite connection helpers for the per-operation connection pattern.
//!
//! All three database layers (ScheduleDb, GatewayStore, LocalStorage) open a
//! fresh `libsql::Connection` per operation to avoid concurrency hazards.
//! Per-connection PRAGMAs like `busy_timeout` and `synchronous` must be
//! re-applied on every new connection — otherwise concurrent writes
//! immediately fail with `SQLITE_BUSY` instead of retrying.
//!
//! This module centralises the PRAGMA values and application logic so the
//! three stores stay consistent.

use libsql::Connection;
use std::time::Duration;

const SYNCHRONOUS_NORMAL: i64 = 1;
const WAL_JOURNAL_MODE: &str = "wal";

/// Default busy-wait timeout.
///
/// When a write is blocked by another writer, SQLite retries for up to this
/// duration before returning `SQLITE_BUSY`.  5 seconds matches the default
/// recommended by the SQLite documentation for multi-writer workloads.
pub const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

/// Errors from applying SQLite connection/database PRAGMAs.
#[derive(Debug, thiserror::Error)]
pub enum PragmaError {
    #[error("failed to set busy_timeout: {0}")]
    BusyTimeout(String),

    #[error("failed to set synchronous: {0}")]
    Synchronous(String),

    #[error("failed to set journal_mode: {0}")]
    JournalMode(String),
}

/// Read the current busy timeout for a connection.
pub async fn read_busy_timeout_millis(conn: &Connection) -> Result<i64, PragmaError> {
    let mut rows = conn
        .query("PRAGMA busy_timeout", ())
        .await
        .map_err(|e| PragmaError::BusyTimeout(e.to_string()))?;
    let row = rows
        .next()
        .await
        .map_err(|e| PragmaError::BusyTimeout(e.to_string()))?
        .ok_or_else(|| {
            PragmaError::BusyTimeout("PRAGMA busy_timeout returned no row".to_string())
        })?;
    row.get(0)
        .map_err(|e| PragmaError::BusyTimeout(e.to_string()))
}

/// Read the current synchronous mode for a connection.
pub async fn read_synchronous_mode(conn: &Connection) -> Result<i64, PragmaError> {
    let mut rows = conn
        .query("PRAGMA synchronous", ())
        .await
        .map_err(|e| PragmaError::Synchronous(e.to_string()))?;
    let row = rows
        .next()
        .await
        .map_err(|e| PragmaError::Synchronous(e.to_string()))?
        .ok_or_else(|| {
            PragmaError::Synchronous("PRAGMA synchronous returned no row".to_string())
        })?;
    row.get(0)
        .map_err(|e| PragmaError::Synchronous(e.to_string()))
}

/// Read the current journal mode for a database connection.
pub async fn read_journal_mode(conn: &Connection) -> Result<String, PragmaError> {
    let mut rows = conn
        .query("PRAGMA journal_mode", ())
        .await
        .map_err(|e| PragmaError::JournalMode(e.to_string()))?;
    let row = rows
        .next()
        .await
        .map_err(|e| PragmaError::JournalMode(e.to_string()))?
        .ok_or_else(|| {
            PragmaError::JournalMode("PRAGMA journal_mode returned no row".to_string())
        })?;
    row.get(0)
        .map_err(|e| PragmaError::JournalMode(e.to_string()))
}

/// Apply per-connection PRAGMAs that do **not** persist across connections.
///
/// Must be called on every connection returned by `Database::connect()`
/// before executing application queries.
///
/// - `busy_timeout` — uses the synchronous `sqlite3_busy_timeout` FFI call
///   (no query round-trip).
/// - `synchronous = NORMAL` — reduces fsync overhead while WAL provides
///   crash safety (one PRAGMA query).
///
/// Database-level PRAGMAs like `journal_mode = WAL` persist and should be
/// set once during initial schema setup via [`apply_database_pragmas`].
pub async fn apply_connection_pragmas(conn: &Connection) -> Result<(), PragmaError> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| PragmaError::BusyTimeout(e.to_string()))?;

    conn.query("PRAGMA synchronous = NORMAL", ())
        .await
        .map_err(|e| PragmaError::Synchronous(e.to_string()))?;

    let mode = read_synchronous_mode(conn).await?;
    if mode != SYNCHRONOUS_NORMAL {
        return Err(PragmaError::Synchronous(format!(
            "expected synchronous=NORMAL ({SYNCHRONOUS_NORMAL}), got {mode}"
        )));
    }

    Ok(())
}

/// Set the database-level WAL journal mode.
///
/// This only needs to be called once per database file (the setting
/// persists). It is separated from [`apply_connection_pragmas`] because
/// `journal_mode` is a database property, not a connection property.
///
/// A startup/setup connection may be a one-off raw connection, so this helper
/// also applies `busy_timeout` before issuing the WAL PRAGMA to tolerate
/// transient writer contention during initialization.
pub async fn apply_database_pragmas(conn: &Connection) -> Result<(), PragmaError> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| PragmaError::BusyTimeout(e.to_string()))?;

    let mut rows = conn
        .query("PRAGMA journal_mode = WAL", ())
        .await
        .map_err(|e| PragmaError::JournalMode(e.to_string()))?;
    let row = rows
        .next()
        .await
        .map_err(|e| PragmaError::JournalMode(e.to_string()))?
        .ok_or_else(|| {
            PragmaError::JournalMode("PRAGMA journal_mode returned no row".to_string())
        })?;
    let mode: String = row
        .get(0)
        .map_err(|e| PragmaError::JournalMode(e.to_string()))?;
    if !mode.eq_ignore_ascii_case(WAL_JOURNAL_MODE) {
        return Err(PragmaError::JournalMode(format!(
            "expected journal_mode=WAL, got {mode}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn open_test_db() -> (libsql::Database, Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("pragma_test.db");
        let db = libsql::Builder::new_local(&db_path)
            .build()
            .await
            .expect("open db");
        let conn = db.connect().expect("connect");
        (db, conn, dir)
    }

    #[tokio::test]
    async fn apply_connection_pragmas_sets_busy_timeout() {
        let (_db, conn, _dir) = open_test_db().await;
        apply_connection_pragmas(&conn)
            .await
            .expect("apply_connection_pragmas");

        let val = read_busy_timeout_millis(&conn)
            .await
            .expect("read_busy_timeout_millis");
        assert_eq!(val, BUSY_TIMEOUT.as_millis() as i64);
    }

    #[tokio::test]
    async fn apply_connection_pragmas_sets_synchronous_normal() {
        let (_db, conn, _dir) = open_test_db().await;
        apply_connection_pragmas(&conn)
            .await
            .expect("apply_connection_pragmas");

        let val = read_synchronous_mode(&conn)
            .await
            .expect("read_synchronous_mode");
        assert_eq!(val, SYNCHRONOUS_NORMAL);
    }

    #[tokio::test]
    async fn bare_connection_has_default_busy_timeout() {
        let (_db, conn, _dir) = open_test_db().await;
        // Do NOT call apply_connection_pragmas.

        let val = read_busy_timeout_millis(&conn)
            .await
            .expect("read_busy_timeout_millis");
        assert_eq!(val, 0, "raw connection should have busy_timeout=0");
    }

    #[tokio::test]
    async fn apply_database_pragmas_sets_busy_timeout_for_startup_queries() {
        let (_db, conn, _dir) = open_test_db().await;
        apply_database_pragmas(&conn)
            .await
            .expect("apply_database_pragmas");

        let timeout = read_busy_timeout_millis(&conn)
            .await
            .expect("read_busy_timeout_millis");
        assert_eq!(timeout, BUSY_TIMEOUT.as_millis() as i64);
    }

    #[tokio::test]
    async fn apply_database_pragmas_sets_wal_journal_mode() {
        let (_db, conn, _dir) = open_test_db().await;
        apply_database_pragmas(&conn)
            .await
            .expect("apply_database_pragmas");

        let mode = read_journal_mode(&conn).await.expect("read_journal_mode");
        assert_eq!(mode, WAL_JOURNAL_MODE);
    }

    /// WAL journal mode persists — a fresh connection should inherit it
    /// without calling apply_database_pragmas again.
    #[tokio::test]
    async fn wal_journal_mode_persists_across_connections() {
        let (db, conn, _dir) = open_test_db().await;
        apply_database_pragmas(&conn)
            .await
            .expect("apply_database_pragmas");
        drop(conn);

        let conn2 = db.connect().expect("second connection");
        let mode = read_journal_mode(&conn2).await.expect("read_journal_mode");
        assert_eq!(mode, WAL_JOURNAL_MODE);
    }

    /// Startup/setup code often uses a one-off raw connection. Ensure the
    /// database-level PRAGMA helper still waits on lock contention by applying
    /// busy_timeout itself before attempting to switch to WAL mode.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_database_pragmas_waits_for_database_lock() {
        let (db, conn_a, _dir) = open_test_db().await;
        conn_a
            .execute("BEGIN EXCLUSIVE", ())
            .await
            .expect("begin exclusive");
        conn_a
            .execute("CREATE TABLE IF NOT EXISTS lock_holder (id INTEGER)", ())
            .await
            .expect("create table under lock");

        let conn_b = db.connect().expect("second connection");
        let writer = tokio::spawn(async move { apply_database_pragmas(&conn_b).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn_a.execute("COMMIT", ()).await.expect("commit");

        let result = writer.await.expect("task panicked");
        assert!(
            result.is_ok(),
            "apply_database_pragmas should wait for lock release; got: {:?}",
            result.err()
        );
    }
}
