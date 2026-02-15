//! Autopilot status command - shows autopilot status and schedule information.

use crate::commands::watch::{
    ListRunsFilter, RunStatus, ScheduleConfig, ScheduleDb, is_process_running,
};
use chrono::{DateTime, Utc};
use croner::Cron;
use std::str::FromStr;

/// Show autopilot status and upcoming schedule runs.
pub async fn show_status() -> Result<(), String> {
    // Load configuration
    let config = match ScheduleConfig::load_default() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load autopilot config: {}", e);
            eprintln!("Run 'stakpak autopilot init' to create a configuration file.");
            return Ok(());
        }
    };

    // Try to connect to database
    let db_path = config.db_path();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| "Invalid database path".to_string())?;

    let db = (ScheduleDb::new(db_path_str).await).ok();

    // Check autopilot state
    let autopilot_state = if let Some(ref db) = db {
        db.get_autopilot_state().await.ok().flatten()
    } else {
        None
    };

    // Print autopilot status
    if let Some(state) = autopilot_state {
        // Check if process is actually running
        if is_process_running(state.pid as u32) {
            println!(
                "Autopilot: \x1b[32mrunning\x1b[0m (PID {}, started {})",
                state.pid,
                format_datetime(&state.started_at)
            );
            println!(
                "  Last heartbeat: {}",
                format_datetime(&state.last_heartbeat)
            );
        } else {
            println!(
                "Autopilot: \x1b[33mstale\x1b[0m (PID {} not running, last seen {})",
                state.pid,
                format_datetime(&state.started_at)
            );
        }
    } else {
        println!("Autopilot: \x1b[31mnot running\x1b[0m");
    }

    println!();

    // Print schedules
    if config.schedules.is_empty() {
        println!("No schedules configured.");
        return Ok(());
    }

    println!("Schedules ({}):", config.schedules.len());
    println!(
        "  {:<24} {:<16} {:<14} {:<22} LAST RUN",
        "NAME", "CRON", "PROFILE", "NEXT RUN"
    );

    for schedule in &config.schedules {
        // Calculate next run time
        let next_run = calculate_next_run(&schedule.cron);

        // Get last run from database
        let last_run_info = if let Some(ref db) = db {
            get_last_run_info(db, &schedule.name).await
        } else {
            None
        };

        let next_run_str = next_run
            .map(|dt| format_datetime(&dt))
            .unwrap_or_else(|| "invalid schedule".to_string());

        let last_run_str = last_run_info
            .map(|(dt, status)| format!("{} ({})", format_datetime(&dt), status))
            .unwrap_or_else(|| "-".to_string());

        let profile = schedule.effective_profile(&config.defaults);

        println!(
            "  {:<24} {:<16} {:<14} {:<22} {}",
            truncate(&schedule.name, 24),
            truncate(&schedule.cron, 16),
            truncate(profile, 14),
            next_run_str,
            last_run_str
        );
    }

    Ok(())
}

/// Calculate the next run time for a cron expression.
fn calculate_next_run(cron_expr: &str) -> Option<DateTime<Utc>> {
    let cron = Cron::from_str(cron_expr).ok()?;
    cron.find_next_occurrence(&Utc::now(), false).ok()
}

/// Get the last run info for a schedule.
async fn get_last_run_info(
    db: &ScheduleDb,
    schedule_name: &str,
) -> Option<(DateTime<Utc>, String)> {
    let filter = ListRunsFilter {
        schedule_name: Some(schedule_name.to_string()),
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
