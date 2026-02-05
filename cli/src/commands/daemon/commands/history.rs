//! Daemon history command - show run history.

use crate::commands::daemon::{DaemonConfig, DaemonDb, ListRunsFilter, RunStatus};
use chrono::{DateTime, Utc};

/// Show run history for all triggers or a specific trigger.
pub async fn show_history(trigger_name: Option<&str>, limit: Option<u32>) -> Result<(), String> {
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

    // Build filter
    let filter = ListRunsFilter {
        trigger_name: trigger_name.map(|s| s.to_string()),
        status: None,
        limit: Some(limit.unwrap_or(20)),
        offset: None,
    };

    // Get runs
    let runs = db
        .list_runs(&filter)
        .await
        .map_err(|e| format!("Failed to list runs: {}", e))?;

    if runs.is_empty() {
        if let Some(name) = trigger_name {
            println!("No runs found for trigger '{}'", name);
        } else {
            println!("No runs found.");
        }
        return Ok(());
    }

    // Print header
    if let Some(name) = trigger_name {
        println!("Run history for '{}' ({} runs):\n", name, runs.len());
    } else {
        println!("Run history ({} runs):\n", runs.len());
    }

    println!(
        "{:<6} {:<20} {:<20} {:<12} {:<20} SESSION",
        "ID", "TRIGGER", "STARTED", "STATUS", "FINISHED"
    );
    println!("{}", "-".repeat(100));

    for run in runs {
        let status_str = format_status(&run.status);
        let finished_str = run
            .finished_at
            .map(|dt| format_datetime(&dt))
            .unwrap_or_else(|| "-".to_string());
        let session_str = run
            .agent_session_id
            .as_ref()
            .map(|s| truncate(s, 20))
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<6} {:<20} {:<20} {:<12} {:<20} {}",
            run.id,
            truncate(&run.trigger_name, 20),
            format_datetime(&run.started_at),
            status_str,
            finished_str,
            session_str
        );

        // Show error message if failed
        if (run.status == RunStatus::Failed || run.status == RunStatus::TimedOut)
            && let Some(error) = &run.error_message
        {
            println!("       \x1b[31mError: {}\x1b[0m", truncate(error, 80));
        }
    }

    Ok(())
}

/// Show detailed information about a specific run.
pub async fn show_run(run_id: i64) -> Result<(), String> {
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

    // Get the run
    let run = db
        .get_run(run_id)
        .await
        .map_err(|e| format!("Failed to get run: {}", e))?;

    // Print run details
    println!("\x1b[1mRun #{}\x1b[0m", run.id);
    println!();
    println!("Trigger:    {}", run.trigger_name);
    println!("Status:     {}", format_status(&run.status));
    println!("Started:    {}", format_datetime(&run.started_at));
    if let Some(finished) = run.finished_at {
        println!("Finished:   {}", format_datetime(&finished));
        let duration = finished - run.started_at;
        println!("Duration:   {}s", duration.num_seconds());
    }

    // Check script results
    if run.check_exit_code.is_some() || run.check_timed_out {
        println!();
        println!("\x1b[1mCheck Script\x1b[0m");
        if run.check_timed_out {
            println!("  Result: \x1b[31mtimed out\x1b[0m");
        } else if let Some(code) = run.check_exit_code {
            let result_str = match code {
                0 => "\x1b[32mpassed (exit 0)\x1b[0m".to_string(),
                1 => "\x1b[33mskipped (exit 1)\x1b[0m".to_string(),
                _ => format!("\x1b[31mfailed (exit {})\x1b[0m", code),
            };
            println!("  Result: {}", result_str);
        }

        if let Some(stdout) = &run.check_stdout
            && !stdout.trim().is_empty()
        {
            println!("  Stdout:");
            for line in stdout.lines().take(20) {
                println!("    {}", line);
            }
            if stdout.lines().count() > 20 {
                println!("    ... ({} more lines)", stdout.lines().count() - 20);
            }
        }

        if let Some(stderr) = &run.check_stderr
            && !stderr.trim().is_empty()
        {
            println!("  Stderr:");
            for line in stderr.lines().take(10) {
                println!("    \x1b[31m{}\x1b[0m", line);
            }
            if stderr.lines().count() > 10 {
                println!("    ... ({} more lines)", stderr.lines().count() - 10);
            }
        }
    }

    // Agent session info
    if run.agent_woken {
        println!();
        println!("\x1b[1mAgent Session\x1b[0m");
        if let Some(session_id) = &run.agent_session_id {
            println!("  Session ID:    {}", session_id);
        }
        if let Some(checkpoint_id) = &run.agent_last_checkpoint_id {
            println!("  Checkpoint ID: {}", checkpoint_id);
        }
    }

    // Error message
    if let Some(error) = &run.error_message {
        println!();
        println!("\x1b[31mError: {}\x1b[0m", error);
    }

    // Show resume hint if applicable
    if run.agent_last_checkpoint_id.is_some()
        && (run.status == RunStatus::Failed || run.status == RunStatus::TimedOut)
    {
        println!();
        println!(
            "\x1b[33mTip: Resume this run with 'stakpak daemon resume {}'\x1b[0m",
            run.id
        );
    }

    Ok(())
}

/// Format a datetime for display.
fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Format a run status with color.
fn format_status(status: &RunStatus) -> String {
    match status {
        RunStatus::Running => "\x1b[34mrunning\x1b[0m".to_string(),
        RunStatus::Completed => "\x1b[32mcompleted\x1b[0m".to_string(),
        RunStatus::Failed => "\x1b[31mfailed\x1b[0m".to_string(),
        RunStatus::Skipped => "\x1b[90mskipped\x1b[0m".to_string(),
        RunStatus::TimedOut => "\x1b[31mtimed out\x1b[0m".to_string(),
    }
}

/// Truncate a string to a maximum length.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
