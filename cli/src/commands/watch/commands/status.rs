//! Watch status command - shows watch status and trigger information.

use crate::commands::watch::{ListRunsFilter, RunStatus, WatchConfig, WatchDb, is_process_running};
use chrono::{DateTime, Utc};
use croner::Cron;
use std::str::FromStr;

/// Show watch status and upcoming trigger runs.
pub async fn show_status() -> Result<(), String> {
    // Load configuration
    let config = match WatchConfig::load_default() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load watch config: {}", e);
            eprintln!("Run 'stakpak watch init' to create a configuration file.");
            return Ok(());
        }
    };

    // Try to connect to database
    let db_path = config.db_path();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| "Invalid database path".to_string())?;

    let db = (WatchDb::new(db_path_str).await).ok();

    // Check watch state
    let watch_state = if let Some(ref db) = db {
        db.get_watch_state().await.ok().flatten()
    } else {
        None
    };

    // Print watch status
    if let Some(state) = watch_state {
        // Check if process is actually running
        if is_process_running(state.pid as u32) {
            println!(
                "Watch: \x1b[32mrunning\x1b[0m (PID {}, started {})",
                state.pid,
                format_datetime(&state.started_at)
            );
            println!(
                "  Last heartbeat: {}",
                format_datetime(&state.last_heartbeat)
            );
        } else {
            println!(
                "Watch: \x1b[33mstale\x1b[0m (PID {} not running, last seen {})",
                state.pid,
                format_datetime(&state.started_at)
            );
        }
    } else {
        println!("Watch: \x1b[31mnot running\x1b[0m");
    }

    println!();

    // Print triggers
    if config.triggers.is_empty() {
        println!("No triggers configured.");
        return Ok(());
    }

    println!("Triggers ({}):", config.triggers.len());
    println!(
        "  {:<24} {:<16} {:<14} {:<22} LAST RUN",
        "NAME", "SCHEDULE", "PROFILE", "NEXT RUN"
    );

    for trigger in &config.triggers {
        // Calculate next run time
        let next_run = calculate_next_run(&trigger.schedule);

        // Get last run from database
        let last_run_info = if let Some(ref db) = db {
            get_last_run_info(db, &trigger.name).await
        } else {
            None
        };

        let next_run_str = next_run
            .map(|dt| format_datetime(&dt))
            .unwrap_or_else(|| "invalid schedule".to_string());

        let last_run_str = last_run_info
            .map(|(dt, status)| format!("{} ({})", format_datetime(&dt), status))
            .unwrap_or_else(|| "-".to_string());

        let profile = trigger.effective_profile(&config.defaults);

        println!(
            "  {:<24} {:<16} {:<14} {:<22} {}",
            truncate(&trigger.name, 24),
            truncate(&trigger.schedule, 16),
            truncate(profile, 14),
            next_run_str,
            last_run_str
        );
    }

    Ok(())
}

/// Calculate the next run time for a cron expression.
fn calculate_next_run(schedule: &str) -> Option<DateTime<Utc>> {
    let cron = Cron::from_str(schedule).ok()?;
    cron.find_next_occurrence(&Utc::now(), false).ok()
}

/// Get the last run info for a trigger.
async fn get_last_run_info(db: &WatchDb, trigger_name: &str) -> Option<(DateTime<Utc>, String)> {
    let filter = ListRunsFilter {
        trigger_name: Some(trigger_name.to_string()),
        status: None,
        limit: Some(1),
        offset: None,
    };

    let runs = db.list_runs(&filter).await.ok()?;
    runs.first().map(|run| {
        let status_str = match run.status {
            RunStatus::Running => "running",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Skipped => "skipped",
            RunStatus::TimedOut => "timed out",
            RunStatus::Paused => "paused",
        };
        (
            run.finished_at.unwrap_or(run.started_at),
            status_str.to_string(),
        )
    })
}

/// Format a datetime for display.
fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Truncate a string to a maximum length, respecting char boundaries.
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
