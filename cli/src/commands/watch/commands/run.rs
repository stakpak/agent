//! Watch run command - runs the watch service in foreground mode.
//!
//! This is the main entry point for the watch service, which:
//! 1. Loads and validates configuration
//! 2. Initializes the SQLite database
//! 3. Sets watch state (PID, start time)
//! 4. Registers all triggers with the scheduler
//! 5. Runs the scheduler loop
//! 6. Handles graceful shutdown on SIGTERM/SIGINT

use crate::commands::watch::{
    RunStatus, SpawnConfig, WatchConfig, WatchDb, WatchScheduler, assemble_prompt,
    is_process_running, run_check_script, spawn_agent,
};
use chrono::{DateTime, Utc};
use croner::Cron;
use std::str::FromStr;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info, warn};

/// Run the watch service in foreground mode.
///
/// This function blocks until the watch service receives a shutdown signal (SIGTERM/SIGINT).
pub async fn run_watch() -> Result<(), String> {
    print_banner();

    // Load and validate configuration
    let config =
        WatchConfig::load_default().map_err(|e| format!("Failed to load config: {}", e))?;

    info!(
        triggers = config.triggers.len(),
        "Configuration loaded successfully"
    );

    // Initialize database directory
    let db_path = config.db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create database directory: {}", e))?;
    }

    // Check for existing watch service via PID file
    let pid_file = db_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("watch.pid");
    if let Some(existing_pid) = check_existing_watch(&pid_file) {
        return Err(format!(
            "Another watch instance is already running (PID {}). \
             Stop it first with 'kill {}' or remove the stale PID file at {}",
            existing_pid,
            existing_pid,
            pid_file.display()
        ));
    }

    // Write PID file
    let pid = std::process::id();
    std::fs::write(&pid_file, pid.to_string())
        .map_err(|e| format!("Failed to write PID file: {}", e))?;

    // Ensure PID file is cleaned up on exit
    let pid_file_cleanup = pid_file.clone();

    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| "Invalid database path".to_string())?;

    let db = WatchDb::new(db_path_str)
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    // Set watch state
    db.set_watch_state(pid as i64)
        .await
        .map_err(|e| format!("Failed to set watch state: {}", e))?;

    info!(pid = pid, db_path = %db_path.display(), "Watch state initialized");

    // Print config summary
    print_config_summary(&config, pid as i64);

    // Create scheduler (returns scheduler and event receiver)
    let (mut scheduler, mut event_rx) = WatchScheduler::new()
        .await
        .map_err(|e| format!("Failed to create scheduler: {}", e))?;

    // Register triggers and collect info for display
    let mut registered_triggers = Vec::new();
    for trigger in &config.triggers {
        if let Err(e) = scheduler.register_trigger(trigger.clone()).await {
            error!(trigger = %trigger.name, error = %e, "Failed to register trigger, skipping");
            eprintln!(
                "  \x1b[31m✗\x1b[0m {} - failed to register: {}",
                trigger.name, e
            );
        } else {
            info!(trigger = %trigger.name, schedule = %trigger.schedule, "Registered trigger");
            registered_triggers.push(trigger.clone());
        }
    }

    scheduler
        .start()
        .await
        .map_err(|e| format!("Failed to start scheduler: {}", e))?;

    info!("Scheduler started, waiting for triggers...");

    // Print registered triggers table
    print_triggers_table(&registered_triggers);

    // Wrap shared state in Arc for the event loop
    let db = Arc::new(db);
    let config = Arc::new(config);

    // Spawn event handler task for scheduled triggers
    let db_clone = Arc::clone(&db);
    let config_clone = Arc::clone(&config);
    let event_handler = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let db = Arc::clone(&db_clone);
            let config = Arc::clone(&config_clone);

            // Handle each trigger event in a separate task
            tokio::spawn(async move {
                if let Err(e) = handle_trigger_event(&db, &config, &event.trigger).await {
                    error!(
                        trigger = %event.trigger.name,
                        error = %e,
                        "Failed to handle trigger event"
                    );
                }
            });
        }
    });

    // Spawn pending trigger poller for manual trigger fires
    let db_clone2 = Arc::clone(&db);
    let config_clone2 = Arc::clone(&config);
    let pending_poller = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;

            // Pop all pending triggers
            match db_clone2.pop_pending_triggers().await {
                Ok(pending) => {
                    for pending_trigger in pending {
                        // Find the trigger config by name
                        if let Some(trigger) = config_clone2
                            .triggers
                            .iter()
                            .find(|t| t.name == pending_trigger.trigger_name)
                        {
                            let db = Arc::clone(&db_clone2);
                            let trigger = trigger.clone();
                            let config = Arc::clone(&config_clone2);

                            // Handle in a separate task
                            tokio::spawn(async move {
                                info!(trigger = %trigger.name, "Manual trigger fired");
                                print_event("fire", &trigger.name, "Manual trigger fired");
                                if let Err(e) = handle_trigger_event(&db, &config, &trigger).await {
                                    error!(
                                        trigger = %trigger.name,
                                        error = %e,
                                        "Failed to handle manual trigger event"
                                    );
                                }
                            });
                        } else {
                            warn!(
                                trigger = %pending_trigger.trigger_name,
                                "Pending trigger not found in config, skipping"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to poll pending triggers");
                }
            }
        }
    });

    // Wait for shutdown signal
    info!("Watch service running. Press Ctrl+C to stop.");
    wait_for_shutdown_signal().await;

    println!();
    println!("\x1b[33mShutdown signal received, stopping watch service...\x1b[0m");
    info!("Shutdown signal received, stopping watch service...");

    // Stop scheduler
    if let Err(e) = scheduler.shutdown().await {
        warn!(error = %e, "Failed to shutdown scheduler");
    }

    // Cancel event handler and pending poller
    event_handler.abort();
    pending_poller.abort();

    // Clear watch state
    if let Err(e) = db.clear_watch_state().await {
        warn!(error = %e, "Failed to clear watch state");
    }

    // Remove PID file
    if let Err(e) = std::fs::remove_file(&pid_file_cleanup) {
        warn!(error = %e, "Failed to remove PID file");
    }

    println!("\x1b[32mWatch service stopped.\x1b[0m");
    info!("Watch service stopped");
    Ok(())
}

/// Handle a trigger event by running the check script and spawning the agent if needed.
async fn handle_trigger_event(
    db: &WatchDb,
    config: &WatchConfig,
    trigger: &crate::commands::watch::Trigger,
) -> Result<(), String> {
    info!(trigger = %trigger.name, "Trigger fired");
    print_event("fire", &trigger.name, "Trigger fired");

    // Insert a new run record
    let run_id = db
        .insert_run(&trigger.name)
        .await
        .map_err(|e| format!("Failed to insert run: {}", e))?;

    // Run check script if defined
    let check_result = if let Some(check_path) = &trigger.check {
        let expanded_path = crate::commands::watch::config::expand_tilde(check_path);
        let timeout = trigger.effective_check_timeout(&config.defaults);

        info!(
            trigger = %trigger.name,
            check_script = %expanded_path.display(),
            "Running check script"
        );
        print_event(
            "check",
            &trigger.name,
            &format!("Running check: {}", expanded_path.display()),
        );

        match run_check_script(&expanded_path, timeout).await {
            Ok(result) => {
                // Update run with check result
                let exit_code = result.exit_code.unwrap_or(-1);
                db.update_run_check_result(
                    run_id,
                    exit_code,
                    &result.stdout,
                    &result.stderr,
                    result.timed_out,
                )
                .await
                .map_err(|e| format!("Failed to update check result: {}", e))?;

                if result.timed_out {
                    warn!(trigger = %trigger.name, "Check script timed out");
                    print_event("timeout", &trigger.name, "Check script timed out");
                    db.update_run_finished(
                        run_id,
                        RunStatus::Failed,
                        Some("Check script timed out"),
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| format!("Failed to update run status: {}", e))?;
                    return Ok(());
                }

                if result.failed() {
                    warn!(
                        trigger = %trigger.name,
                        exit_code = ?result.exit_code,
                        "Check script failed"
                    );
                    print_event(
                        "fail",
                        &trigger.name,
                        &format!("Check failed (exit {})", result.exit_code.unwrap_or(-1)),
                    );
                    db.update_run_finished(
                        run_id,
                        RunStatus::Failed,
                        Some("Check script failed"),
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| format!("Failed to update run status: {}", e))?;
                    return Ok(());
                }

                if result.skipped() {
                    info!(trigger = %trigger.name, "Check script returned skip (exit 1)");
                    print_event("skip", &trigger.name, "Skipped (check returned exit 1)");
                    db.update_run_finished(run_id, RunStatus::Skipped, None, None, None)
                        .await
                        .map_err(|e| format!("Failed to update run status: {}", e))?;
                    return Ok(());
                }

                Some(result)
            }
            Err(e) => {
                error!(trigger = %trigger.name, error = %e, "Failed to run check script");
                print_event("fail", &trigger.name, &format!("Check error: {}", e));
                db.update_run_finished(
                    run_id,
                    RunStatus::Failed,
                    Some(&format!("Check script error: {}", e)),
                    None,
                    None,
                )
                .await
                .map_err(|e| format!("Failed to update run status: {}", e))?;
                return Ok(());
            }
        }
    } else {
        None
    };

    // Assemble prompt
    let prompt = assemble_prompt(trigger, check_result.as_ref());

    info!(trigger = %trigger.name, "Waking agent");
    print_event("agent", &trigger.name, "Spawning agent...");

    // Spawn agent
    let spawn_config = SpawnConfig {
        prompt,
        profile: trigger.effective_profile(&config.defaults).to_string(),
        timeout: trigger.effective_timeout(&config.defaults),
        workdir: None,
        enable_slack_tools: trigger.effective_enable_slack_tools(&config.defaults),
        enable_subagents: trigger.effective_enable_subagents(&config.defaults),
        pause_on_approval: trigger.effective_pause_on_approval(&config.defaults),
    };

    match spawn_agent(spawn_config).await {
        Ok(result) => {
            // Update run with agent session info
            if let Some(session_id) = &result.session_id {
                db.update_run_agent_started(run_id, session_id)
                    .await
                    .map_err(|e| format!("Failed to update agent session: {}", e))?;
            }

            if let Some(checkpoint_id) = &result.checkpoint_id {
                db.update_run_checkpoint(run_id, checkpoint_id)
                    .await
                    .map_err(|e| format!("Failed to update checkpoint: {}", e))?;
            }

            // Determine final status and print event
            let (status, error_msg) = if result.timed_out {
                print_event("timeout", &trigger.name, "Agent timed out");
                (RunStatus::TimedOut, Some("Agent timed out".to_string()))
            } else if result.is_paused() {
                let resume_hint = result
                    .resume_hint
                    .as_deref()
                    .unwrap_or("stakpak watch resume <run_id>");
                print_event(
                    "pause",
                    &trigger.name,
                    &format!("Agent paused - resume with: {}", resume_hint),
                );
                (RunStatus::Paused, None)
            } else if result.success() {
                print_event("done", &trigger.name, "Agent completed successfully");
                (RunStatus::Completed, None)
            } else {
                print_event(
                    "fail",
                    &trigger.name,
                    &format!("Agent failed (exit {:?})", result.exit_code),
                );
                (
                    RunStatus::Failed,
                    Some(format!("Agent exited with code {:?}", result.exit_code)),
                )
            };

            // Store agent output (truncate if too large, respecting unicode boundaries)
            let stdout = if result.stdout.is_empty() {
                None
            } else {
                Some(truncate_string(&result.stdout, 100_000))
            };
            let stderr = if result.stderr.is_empty() {
                None
            } else {
                Some(truncate_string(&result.stderr, 100_000))
            };

            db.update_run_finished(
                run_id,
                status,
                error_msg.as_deref(),
                stdout.as_deref(),
                stderr.as_deref(),
            )
            .await
            .map_err(|e| format!("Failed to update run status: {}", e))?;

            info!(
                trigger = %trigger.name,
                status = ?status,
                session_id = ?result.session_id,
                paused = result.is_paused(),
                "Agent completed"
            );
        }
        Err(e) => {
            error!(trigger = %trigger.name, error = %e, "Failed to spawn agent");
            print_event(
                "fail",
                &trigger.name,
                &format!("Failed to spawn agent: {}", e),
            );
            db.update_run_finished(
                run_id,
                RunStatus::Failed,
                Some(&format!("Failed to spawn agent: {}", e)),
                None,
                None,
            )
            .await
            .map_err(|e| format!("Failed to update run status: {}", e))?;
        }
    }

    Ok(())
}

/// Truncate a string to a maximum byte length, respecting unicode character boundaries.
fn truncate_string(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    // Find the last valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}... (truncated)", &s[..end])
}

/// Wait for SIGTERM, SIGINT, or SIGHUP signal.
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = signal::ctrl_c().await {
            tracing::error!(error = %e, "Failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to install SIGTERM handler");
            }
        }
    };

    #[cfg(unix)]
    let hangup = async {
        match signal::unix::signal(signal::unix::SignalKind::hangup()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to install SIGHUP handler");
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    #[cfg(not(unix))]
    let hangup = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
        _ = hangup => {},
    }
}

/// Check if an existing watch service is running by reading the PID file.
/// Returns Some(pid) if a watch service is running, None otherwise.
fn check_existing_watch(pid_file: &std::path::Path) -> Option<u32> {
    let pid_str = std::fs::read_to_string(pid_file).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;

    // Check if process is actually running
    if is_process_running(pid) {
        Some(pid)
    } else {
        // Stale PID file - remove it
        let _ = std::fs::remove_file(pid_file);
        None
    }
}

// ============================================================================
// Console output helpers for foreground mode
// ============================================================================

/// Print startup banner.
fn print_banner() {
    println!();
    println!("\x1b[1;36m+-------------------------------------+\x1b[0m");
    println!(
        "\x1b[1;36m|\x1b[0m   \x1b[1mStakpak Watch\x1b[0m                      \x1b[1;36m|\x1b[0m"
    );
    println!("\x1b[1;36m|\x1b[0m   Autonomous Agent Scheduler        \x1b[1;36m|\x1b[0m");
    println!("\x1b[1;36m+-------------------------------------+\x1b[0m");
    println!();
}

/// Print configuration summary.
fn print_config_summary(config: &WatchConfig, pid: i64) {
    println!("\x1b[1mConfiguration:\x1b[0m");
    println!("  PID:        {}", pid);
    println!("  Database:   {}", config.db_path().display());
    println!("  Log dir:    {}", config.log_dir().display());
    println!("  Profile:    {}", config.defaults.profile);
    println!(
        "  Timeout:    {}",
        humantime::format_duration(config.defaults.timeout)
    );
    println!();
}

/// Print registered triggers table with next run times.
fn print_triggers_table(triggers: &[crate::commands::watch::Trigger]) {
    if triggers.is_empty() {
        println!("\x1b[33mNo triggers registered.\x1b[0m");
        println!();
        return;
    }

    println!("\x1b[1mRegistered Triggers ({}):\x1b[0m", triggers.len());
    println!("  {:<24} {:<18} {:<24}", "NAME", "SCHEDULE", "NEXT RUN");
    println!("  {}", "-".repeat(66));

    for trigger in triggers {
        let next_run = calculate_next_run(&trigger.schedule)
            .map(|dt| format_relative_time(&dt))
            .unwrap_or_else(|| "invalid".to_string());

        println!(
            "  {:<24} {:<18} {}",
            truncate(&trigger.name, 24),
            truncate(&trigger.schedule, 18),
            next_run
        );
    }

    println!();
    println!("\x1b[32mWatch service running.\x1b[0m Press \x1b[1mCtrl+C\x1b[0m to stop.");
    println!();
    println!("\x1b[2m--- Event Log ---\x1b[0m");
    println!();
}

/// Calculate the next run time for a cron expression.
fn calculate_next_run(schedule: &str) -> Option<DateTime<Utc>> {
    let cron = Cron::from_str(schedule).ok()?;
    cron.find_next_occurrence(&Utc::now(), false).ok()
}

/// Format datetime as relative time (e.g., "in 5m 30s").
fn format_relative_time(dt: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = dt.signed_duration_since(now);

    if duration.num_seconds() < 0 {
        return "now".to_string();
    }

    let total_secs = duration.num_seconds();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("in {}h {}m", hours, mins)
    } else if mins > 0 {
        format!("in {}m {}s", mins, secs)
    } else {
        format!("in {}s", secs)
    }
}

/// Truncate a string to a maximum length, respecting unicode character boundaries.
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Print a timestamped event to the console.
#[allow(dead_code)]
fn print_event(event_type: &str, trigger_name: &str, message: &str) {
    let timestamp = Utc::now().format("%H:%M:%S");
    let (color, symbol) = match event_type {
        "fire" => ("\x1b[33m", ">>"),
        "check" => ("\x1b[36m", "?"),
        "skip" => ("\x1b[2m", "--"),
        "agent" => ("\x1b[35m", "=>"),
        "done" => ("\x1b[32m", "OK"),
        "fail" => ("\x1b[31m", "XX"),
        "timeout" => ("\x1b[31m", "TO"),
        _ => ("\x1b[0m", ".."),
    };
    println!(
        "{}{} [{}] {}: {}\x1b[0m",
        color, symbol, timestamp, trigger_name, message
    );
}
