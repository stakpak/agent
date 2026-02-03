//! v002: Add nullable columns and make some columns nullable
//!
//! - sessions: add status, cwd columns; make agent_id nullable
//! - checkpoints: make status, execution_depth nullable

use super::Migration;
use libsql::Connection;
use std::future::Future;
use std::pin::Pin;

pub fn migration() -> Migration {
    Migration {
        version: 2,
        description: "Add status/cwd columns, make agent_id/status/execution_depth nullable",
        apply,
        rollback,
    }
}

fn apply(conn: &Connection) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
    Box::pin(async move {
        // Migrate sessions table
        conn.execute("ALTER TABLE sessions RENAME TO _sessions_old", ())
            .await
            .map_err(|e| e.to_string())?;

        conn.execute(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                agent_id TEXT,
                visibility TEXT NOT NULL DEFAULT 'PRIVATE',
                status TEXT DEFAULT 'ACTIVE',
                cwd TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT INTO sessions (id, title, agent_id, visibility, status, cwd, created_at, updated_at)
             SELECT id, title, agent_id, visibility, 'ACTIVE', NULL, created_at, updated_at
             FROM _sessions_old",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        conn.execute("DROP TABLE _sessions_old", ())
            .await
            .map_err(|e| e.to_string())?;

        // Migrate checkpoints table
        conn.execute("ALTER TABLE checkpoints RENAME TO _checkpoints_old", ())
            .await
            .map_err(|e| e.to_string())?;

        conn.execute(
            "CREATE TABLE checkpoints (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                status TEXT,
                execution_depth INTEGER,
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
            "INSERT INTO checkpoints (id, session_id, status, execution_depth, parent_id, state, created_at, updated_at)
             SELECT id, session_id, status, execution_depth, parent_id, state, created_at, updated_at
             FROM _checkpoints_old",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        conn.execute("DROP TABLE _checkpoints_old", ())
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    })
}

fn rollback(conn: &Connection) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
    Box::pin(async move {
        // Rollback sessions table
        conn.execute("ALTER TABLE sessions RENAME TO _sessions_old", ())
            .await
            .map_err(|e| e.to_string())?;

        conn.execute(
            "CREATE TABLE sessions (
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
            "INSERT INTO sessions (id, title, agent_id, visibility, created_at, updated_at)
             SELECT id, title, COALESCE(agent_id, ''), visibility, created_at, updated_at
             FROM _sessions_old",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        conn.execute("DROP TABLE _sessions_old", ())
            .await
            .map_err(|e| e.to_string())?;

        // Rollback checkpoints table
        conn.execute("ALTER TABLE checkpoints RENAME TO _checkpoints_old", ())
            .await
            .map_err(|e| e.to_string())?;

        conn.execute(
            "CREATE TABLE checkpoints (
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
            "INSERT INTO checkpoints (id, session_id, status, execution_depth, parent_id, state, created_at, updated_at)
             SELECT id, session_id, COALESCE(status, ''), COALESCE(execution_depth, 0), parent_id, state, created_at, updated_at
             FROM _checkpoints_old",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

        conn.execute("DROP TABLE _checkpoints_old", ())
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    })
}
