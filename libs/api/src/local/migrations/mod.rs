//! Database migrations
//!
//! Each migration has a unique version number and is applied in order.
//! Applied migrations are tracked in the `_migrations` table.
//!
//! Migrations support both `apply` and `rollback` operations via async functions.

use libsql::Connection;
use std::future::Future;
use std::pin::Pin;

mod v001_initial_schema;
mod v002_nullable_columns;

/// Async migration function type
pub type MigrationFn =
    fn(&Connection) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

/// A database migration with apply and rollback functions
pub struct Migration {
    /// Unique version number (must be sequential)
    pub version: u32,
    /// Human-readable description
    pub description: &'static str,
    /// Function to apply the migration
    pub apply: MigrationFn,
    /// Function to rollback the migration
    pub rollback: MigrationFn,
}

/// All migrations in order
pub fn all_migrations() -> Vec<Migration> {
    vec![
        v001_initial_schema::migration(),
        v002_nullable_columns::migration(),
    ]
}

/// Apply all pending migrations
pub async fn apply_all(conn: &Connection) -> Result<Vec<u32>, String> {
    init_migrations_table(conn).await?;

    let applied = get_applied_versions(conn).await?;
    let mut newly_applied = Vec::new();

    for migration in all_migrations() {
        if applied.contains(&migration.version) {
            continue;
        }

        apply_migration(conn, &migration).await?;
        newly_applied.push(migration.version);
    }

    Ok(newly_applied)
}

/// Rollback the last applied migration
pub async fn rollback_last(conn: &Connection) -> Result<Option<u32>, String> {
    let applied = get_applied_versions(conn).await?;

    if let Some(&last_version) = applied.last() {
        let migrations = all_migrations();
        if let Some(migration) = migrations.iter().find(|m| m.version == last_version) {
            rollback_migration(conn, migration).await?;
            return Ok(Some(last_version));
        }
    }

    Ok(None)
}

/// Rollback to a specific version (keeps that version, removes newer ones)
pub async fn rollback_to(conn: &Connection, target_version: u32) -> Result<Vec<u32>, String> {
    let applied = get_applied_versions(conn).await?;
    let migrations = all_migrations();
    let mut rolled_back = Vec::new();

    for &version in applied.iter().rev() {
        if version <= target_version {
            break;
        }

        if let Some(migration) = migrations.iter().find(|m| m.version == version) {
            rollback_migration(conn, migration).await?;
            rolled_back.push(version);
        }
    }

    Ok(rolled_back)
}

/// Get current migration version (0 if none applied)
pub async fn current_version(conn: &Connection) -> Result<u32, String> {
    let applied = get_applied_versions(conn).await?;
    Ok(applied.last().copied().unwrap_or(0))
}

/// Get list of applied migration versions
pub async fn get_applied_versions(conn: &Connection) -> Result<Vec<u32>, String> {
    let mut rows = conn
        .query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='_migrations'",
            (),
        )
        .await
        .map_err(|e| e.to_string())?;

    if rows.next().await.map_err(|e| e.to_string())?.is_none() {
        return Ok(Vec::new());
    }
    drop(rows);

    let mut applied: Vec<u32> = Vec::new();
    let mut rows = conn
        .query("SELECT version FROM _migrations ORDER BY version", ())
        .await
        .map_err(|e| e.to_string())?;

    while let Ok(Some(row)) = rows.next().await {
        if let Ok(version) = row.get::<u32>(0) {
            applied.push(version);
        }
    }

    Ok(applied)
}

/// Get migration status
pub async fn status(conn: &Connection) -> Result<MigrationStatus, String> {
    let applied = get_applied_versions(conn).await?;
    let all = all_migrations();

    let pending: Vec<u32> = all
        .iter()
        .filter(|m| !applied.contains(&m.version))
        .map(|m| m.version)
        .collect();

    Ok(MigrationStatus { applied, pending })
}

pub struct MigrationStatus {
    pub applied: Vec<u32>,
    pub pending: Vec<u32>,
}

// ============================================================================
// Internal
// ============================================================================

async fn init_migrations_table(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
        (),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn apply_migration(conn: &Connection, migration: &Migration) -> Result<(), String> {
    conn.execute("PRAGMA foreign_keys=OFF", ())
        .await
        .map_err(|e| e.to_string())?;

    (migration.apply)(conn).await?;

    conn.execute(
        "INSERT INTO _migrations (version, description, applied_at) VALUES (?, ?, datetime('now'))",
        (migration.version, migration.description),
    )
    .await
    .map_err(|e| e.to_string())?;

    conn.execute("PRAGMA foreign_keys=ON", ())
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

async fn rollback_migration(conn: &Connection, migration: &Migration) -> Result<(), String> {
    conn.execute("PRAGMA foreign_keys=OFF", ())
        .await
        .map_err(|e| e.to_string())?;

    (migration.rollback)(conn).await?;

    conn.execute(
        "DELETE FROM _migrations WHERE version = ?",
        [migration.version],
    )
    .await
    .map_err(|e| e.to_string())?;

    conn.execute("PRAGMA foreign_keys=ON", ())
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ============================================================================
// Public API for storage.rs
// ============================================================================

/// Run all pending migrations
pub async fn run_migrations(conn: &Connection) -> Result<(), String> {
    apply_all(conn).await?;
    Ok(())
}
