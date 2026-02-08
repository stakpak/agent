//! Daemon prune command - clean up old run history.

use crate::commands::daemon::{DaemonConfig, DaemonDb};

/// Prune old run history.
pub async fn prune_history(days: u32) -> Result<(), String> {
    // Load configuration
    let config =
        DaemonConfig::load_default().map_err(|e| format!("Failed to load daemon config: {}", e))?;

    // Connect to database
    let db_path = config.db_path();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| "Invalid database path".to_string())?;

    let db = DaemonDb::new(db_path_str)
        .await
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Prune old runs
    let deleted = db
        .prune_runs(days)
        .await
        .map_err(|e| format!("Failed to prune runs: {}", e))?;

    if deleted > 0 {
        println!(
            "Pruned {} run{} older than {} day{}",
            deleted,
            if deleted == 1 { "" } else { "s" },
            days,
            if days == 1 { "" } else { "s" }
        );
    } else {
        println!("No runs older than {} days to prune", days);
    }

    Ok(())
}
