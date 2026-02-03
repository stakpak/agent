//! v001: Initial schema
//!
//! Creates the sessions and checkpoints tables with the original schema.

use super::Migration;
use libsql::Connection;
use std::future::Future;
use std::pin::Pin;

pub fn migration() -> Migration {
    Migration {
        version: 1,
        description: "Initial schema",
        apply,
        rollback,
    }
}

fn apply(conn: &Connection) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
    Box::pin(async move {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                visibility TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS checkpoints (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                status TEXT NOT NULL,
                execution_depth INTEGER NOT NULL,
                parent_id TEXT,
                state TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id),
                FOREIGN KEY(parent_id) REFERENCES checkpoints(id)
            )",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_checkpoints_session_id ON checkpoints(session_id)",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    })
}

fn rollback(conn: &Connection) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
    Box::pin(async move {
        conn.execute("DROP INDEX IF EXISTS idx_checkpoints_session_id", ())
            .await
            .map_err(|e| e.to_string())?;

        conn.execute("DROP TABLE IF EXISTS checkpoints", ())
            .await
            .map_err(|e| e.to_string())?;

        conn.execute("DROP TABLE IF EXISTS sessions", ())
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    })
}
