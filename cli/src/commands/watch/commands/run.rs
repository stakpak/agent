//! Autopilot run command - runs the autopilot service in foreground mode.
//!
//! This is the main entry point for the autopilot service, which:
//! 1. Loads and validates configuration
//! 2. Initializes the SQLite database
//! 3. Sets autopilot state (PID, start time)
//! 4. Registers all schedules with the scheduler
//! 5. Runs the scheduler loop
//! 6. Handles graceful shutdown on SIGTERM/SIGINT

use crate::commands::watch::{
    RunStatus, ScheduleConfig, ScheduleDb, Scheduler, SpawnConfig, assemble_prompt,
    is_process_running, run_check_script, spawn_agent,
};
use chrono::{DateTime, Utc};
use croner::Cron;
use stakpak_shared::utils::sanitize_text_output;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{error, info, warn};

/// Run the autopilot service in foreground mode.
///
/// This function blocks until the autopilot service receives a shutdown signal (SIGTERM/SIGINT).
pub async fn run_scheduler() -> Result<(), String> {
    print_banner();

    // Load and validate configuration
    let config =
        ScheduleConfig::load_default().map_err(|e| format!("Failed to load config: {}", e))?;

    info!(
        schedules = config.schedules.len(),
        "Configuration loaded successfully"
    );

    // Initialize database directory
    let db_path = config.db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create database directory: {}", e))?;
    }

    // Check for existing autopilot service via PID file
    let pid_file = db_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("autopilot.pid");
    if let Some(existing_pid) = check_existing_autopilot(&pid_file) {
        return Err(format!(
            "Another autopilot instance is already running (PID {}). \
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

    let db = ScheduleDb::new(db_path_str)
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    // Set autopilot state
    db.set_autopilot_state(pid as i64)
        .await
        .map_err(|e| format!("Failed to set autopilot state: {}", e))?;

    info!(pid = pid, db_path = %db_path.display(), "Autopilot state initialized");

    // Print config summary
    print_config_summary(&config, pid as i64);

    // Create scheduler (returns scheduler and event receiver)
    let (mut scheduler, mut event_rx) = Scheduler::new()
        .await
        .map_err(|e| format!("Failed to create scheduler: {}", e))?;

    // Register schedules and collect info for display
    let mut registered_schedules = Vec::new();
    for schedule in &config.schedules {
        if let Err(e) = scheduler.register_schedule(schedule.clone()).await {
            error!(schedule = %schedule.name, error = %e, "Failed to register schedule, skipping");
            eprintln!(
                "  \x1b[31m✗\x1b[0m {} - failed to register: {}",
                schedule.name, e
            );
        } else {
            info!(schedule = %schedule.name, cron = %schedule.cron, "Registered schedule");
            registered_schedules.push(schedule.clone());
        }
    }

    scheduler
        .start()
        .await
        .map_err(|e| format!("Failed to start scheduler: {}", e))?;

    info!("Scheduler started, waiting for schedules...");

    // Print registered schedules table
    print_schedules_table(&registered_schedules);

    // Wrap shared state in Arc for the event loop
    let db = Arc::new(db);
    let config = Arc::new(config);

    // Spawn event handler task for scheduled schedules
    let db_clone = Arc::clone(&db);
    let config_clone = Arc::clone(&config);
    let event_handler = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let db = Arc::clone(&db_clone);
            let config = Arc::clone(&config_clone);

            // Handle each schedule event in a separate task
            tokio::spawn(async move {
                if let Err(e) = handle_schedule_event(&db, &config, &event.schedule).await {
                    error!(
                        schedule = %event.schedule.name,
                        error = %e,
                        "Failed to handle schedule event"
                    );
                }
            });
        }
    });

    // Spawn pending schedule poller for manual schedule fires
    let db_clone2 = Arc::clone(&db);
    let config_clone2 = Arc::clone(&config);
    let pending_poller = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;

            // Pop all pending schedules
            match db_clone2.pop_pending_schedules().await {
                Ok(pending) => {
                    for pending_schedule in pending {
                        // Find the schedule config by name
                        if let Some(schedule) = config_clone2
                            .schedules
                            .iter()
                            .find(|s| s.name == pending_schedule.schedule_name)
                        {
                            let db = Arc::clone(&db_clone2);
                            let schedule = schedule.clone();
                            let config = Arc::clone(&config_clone2);

                            // Handle in a separate task
                            tokio::spawn(async move {
                                info!(schedule = %schedule.name, "Manual schedule fired");
                                print_event("fire", &schedule.name, "Manual schedule fired");
                                if let Err(e) = handle_schedule_event(&db, &config, &schedule).await
                                {
                                    error!(
                                        schedule = %schedule.name,
                                        error = %e,
                                        "Failed to handle manual schedule event"
                                    );
                                }
                            });
                        } else {
                            warn!(
                                schedule = %pending_schedule.schedule_name,
                                "Pending schedule not found in config, skipping"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to poll pending schedules");
                }
            }
        }
    });

    // Wait for shutdown signal
    info!("Autopilot running. Press Ctrl+C to stop.");
    wait_for_shutdown_signal().await;

    println!();
    println!("\x1b[33mShutdown signal received, stopping autopilot service...\x1b[0m");
    info!("Shutdown signal received, stopping autopilot service...");

    // Stop scheduler
    if let Err(e) = scheduler.shutdown().await {
        warn!(error = %e, "Failed to shutdown scheduler");
    }

    // Cancel event handler and pending poller
    event_handler.abort();
    pending_poller.abort();

    // Clear autopilot state
    if let Err(e) = db.clear_autopilot_state().await {
        warn!(error = %e, "Failed to clear autopilot state");
    }

    // Remove PID file
    if let Err(e) = std::fs::remove_file(&pid_file_cleanup) {
        warn!(error = %e, "Failed to remove PID file");
    }

    println!("\x1b[32mAutopilot stopped.\x1b[0m");
    info!("Autopilot stopped");
    Ok(())
}

/// Handle a schedule event by running the check script and spawning the agent if needed.
async fn handle_schedule_event(
    db: &ScheduleDb,
    config: &ScheduleConfig,
    schedule: &crate::commands::watch::Schedule,
) -> Result<(), String> {
    // Singleton guard: skip if this schedule already has a running run
    match db.has_running_run(&schedule.name).await {
        Ok(true) => {
            info!(
                schedule = %schedule.name,
                "Skipping: previous run still in progress"
            );
            print_event(
                "skip",
                &schedule.name,
                "Skipped (previous run still in progress)",
            );
            return Ok(());
        }
        Ok(false) => {} // No running run, proceed
        Err(e) => {
            warn!(
                schedule = %schedule.name,
                error = %e,
                "Failed to check for running runs, proceeding anyway"
            );
        }
    }

    info!(schedule = %schedule.name, "Schedule fired");
    print_event("fire", &schedule.name, "Schedule fired");

    // Insert a new run record
    let run_id = db
        .insert_run(&schedule.name)
        .await
        .map_err(|e| format!("Failed to insert run: {}", e))?;

    // Run check script if defined
    let check_result = if let Some(check_path) = &schedule.check {
        let expanded_path = crate::commands::watch::config::expand_tilde(check_path);
        let timeout = schedule.effective_check_timeout(&config.defaults);

        info!(
            schedule = %schedule.name,
            check_script = %expanded_path.display(),
            "Running check script"
        );
        print_event(
            "check",
            &schedule.name,
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
                    warn!(schedule = %schedule.name, "Check script timed out");
                    print_event("timeout", &schedule.name, "Check script timed out");
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

                // Determine if we should trigger based on trigger_on setting
                let exit_code = result.exit_code.unwrap_or(-1);
                let trigger_on = schedule.effective_trigger_on(&config.defaults);
                let should_trigger = trigger_on.should_trigger(exit_code);

                if !should_trigger {
                    info!(
                        schedule = %schedule.name,
                        exit_code = exit_code,
                        trigger_on = %trigger_on,
                        "Check script did not meet trigger condition"
                    );
                    print_event(
                        "skip",
                        &schedule.name,
                        &format!(
                            "Skipped (exit {} does not match trigger_on={})",
                            exit_code, trigger_on
                        ),
                    );
                    db.update_run_finished(run_id, RunStatus::Skipped, None, None, None)
                        .await
                        .map_err(|e| format!("Failed to update run status: {}", e))?;
                    return Ok(());
                }

                info!(
                    schedule = %schedule.name,
                    exit_code = exit_code,
                    trigger_on = %trigger_on,
                    "Check script met trigger condition"
                );

                Some(result)
            }
            Err(e) => {
                error!(schedule = %schedule.name, error = %e, "Failed to run check script");
                print_event("fail", &schedule.name, &format!("Check error: {}", e));
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
    let prompt = assemble_prompt(schedule, check_result.as_ref());

    info!(schedule = %schedule.name, "Waking agent");
    print_event("agent", &schedule.name, "Spawning agent...");

    // Spawn agent
    let spawn_config = SpawnConfig {
        prompt,
        profile: schedule.effective_profile(&config.defaults).to_string(),
        timeout: schedule.effective_timeout(&config.defaults),
        workdir: None,
        enable_slack_tools: schedule.effective_enable_slack_tools(&config.defaults),
        enable_subagents: schedule.effective_enable_subagents(&config.defaults),
        pause_on_approval: schedule.effective_pause_on_approval(&config.defaults),
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
                print_event("timeout", &schedule.name, "Agent timed out");
                (RunStatus::TimedOut, Some("Agent timed out".to_string()))
            } else if result.is_paused() {
                let resume_hint = result
                    .resume_hint
                    .as_deref()
                    .unwrap_or("stakpak autopilot schedule inspect <run_id>");
                print_event(
                    "pause",
                    &schedule.name,
                    &format!("Agent paused - resume with: {}", resume_hint),
                );
                (RunStatus::Paused, None)
            } else if result.success() {
                print_event("done", &schedule.name, "Agent completed successfully");
                (RunStatus::Completed, None)
            } else {
                print_event(
                    "fail",
                    &schedule.name,
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

            maybe_send_notification(config, schedule, &result, check_result.as_ref(), None).await;

            info!(
                schedule = %schedule.name,
                status = ?status,
                session_id = ?result.session_id,
                paused = result.is_paused(),
                "Agent completed"
            );
        }
        Err(e) => {
            error!(schedule = %schedule.name, error = %e, "Failed to spawn agent");
            print_event(
                "fail",
                &schedule.name,
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

            maybe_send_notification(
                config,
                schedule,
                &crate::commands::watch::agent::AgentResult {
                    exit_code: Some(1),
                    session_id: None,
                    checkpoint_id: None,
                    timed_out: false,
                    paused: false,
                    pause_reason: None,
                    resume_hint: None,
                    stdout: String::new(),
                    stderr: format!("Failed to spawn agent: {}", e),
                },
                check_result.as_ref(),
                Some(&format!("Failed to spawn agent: {}", e)),
            )
            .await;
        }
    }

    Ok(())
}

async fn maybe_send_notification(
    config: &ScheduleConfig,
    schedule: &crate::commands::watch::Schedule,
    result: &crate::commands::watch::agent::AgentResult,
    check_result: Option<&crate::commands::watch::CheckResult>,
    error_override: Option<&str>,
) {
    let Some(notifications) = &config.notifications else {
        return;
    };

    let success = result.success();
    if !notifications.should_notify(schedule, success) {
        return;
    }

    let Some(delivery) = schedule.effective_delivery(notifications) else {
        warn!(schedule = %schedule.name, "Notification enabled but delivery target is missing");
        return;
    };

    let text = format_notification(schedule, result, check_result, error_override);
    let context = serde_json::json!({
        "schedule": schedule.name,
        "summary": extract_summary(result, error_override),
        "check_output": check_result
            .map(|value| sanitize_text_output(value.stdout.trim()))
            .filter(|value| !value.is_empty()),
        "status": if success { "completed" } else { "failed" },
    });

    let payload = serde_json::json!({
        "channel": delivery.channel,
        "target": build_gateway_target(&delivery),
        "text": text,
        "context": context,
    });

    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            warn!(
                schedule = %schedule.name,
                error = %error,
                "Failed to create notification HTTP client"
            );
            return;
        }
    };
    let mut request = client.post(format!("{}/v1/gateway/send", notifications.gateway_url));

    if let Some(token) = notifications.gateway_token.as_deref()
        && !token.is_empty()
    {
        request = request.bearer_auth(token);
    }

    match request.json(&payload).send().await {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                warn!(
                    schedule = %schedule.name,
                    status = %status,
                    body = %body,
                    "Gateway notification request failed"
                );
            }
        }
        Err(error) => {
            warn!(
                schedule = %schedule.name,
                error = %error,
                "Failed to send watch notification"
            );
        }
    }
}

fn build_gateway_target(delivery: &crate::commands::watch::DeliveryConfig) -> serde_json::Value {
    match delivery.channel.as_str() {
        "telegram" => serde_json::json!({ "chat_id": delivery.chat_id }),
        "discord" => serde_json::json!({ "channel_id": delivery.chat_id }),
        "slack" => serde_json::json!({ "channel": delivery.chat_id }),
        _ => serde_json::json!({ "chat_id": delivery.chat_id }),
    }
}

fn format_notification(
    schedule: &crate::commands::watch::Schedule,
    result: &crate::commands::watch::agent::AgentResult,
    check_result: Option<&crate::commands::watch::CheckResult>,
    error_override: Option<&str>,
) -> String {
    let emoji = if result.success() { "✅" } else { "❌" };
    let status = if result.success() {
        "completed"
    } else {
        "failed"
    };

    let mut text = format!("{} {} {}\n", emoji, schedule.name, status);

    if let Some(check) = check_result
        && let Some(exit) = check.exit_code
    {
        text.push_str(&format!("Check exit code: {}\n", exit));
    }

    let summary = extract_summary(result, error_override);
    if !summary.is_empty() {
        text.push('\n');
        text.push_str(&summary);
    }

    text
}

fn extract_summary(
    result: &crate::commands::watch::agent::AgentResult,
    error_override: Option<&str>,
) -> String {
    if let Some(error) = error_override {
        return sanitize_and_truncate(error, 500);
    }

    if !result.stdout.trim().is_empty() {
        return sanitize_and_truncate(result.stdout.trim(), 500);
    }

    if !result.stderr.trim().is_empty() {
        return sanitize_and_truncate(result.stderr.trim(), 500);
    }

    String::new()
}

fn sanitize_and_truncate(text: &str, max_bytes: usize) -> String {
    let sanitized = sanitize_text_output(text);
    truncate_string(&sanitized, max_bytes)
}

/// Truncate a string to a maximum length, respecting char boundaries.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        return s.to_string();
    }

    let truncated: String = s.chars().take(max_len).collect();
    format!("{}... (truncated)", truncated)
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

/// Check if an existing autopilot service is running by reading the PID file.
/// Returns Some(pid) if an autopilot service is running, None otherwise.
fn check_existing_autopilot(pid_file: &std::path::Path) -> Option<u32> {
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
        "\x1b[1;36m|\x1b[0m   \x1b[1mStakpak Autopilot\x1b[0m                      \x1b[1;36m|\x1b[0m"
    );
    println!("\x1b[1;36m|\x1b[0m   Autonomous Agent Scheduler        \x1b[1;36m|\x1b[0m");
    println!("\x1b[1;36m+-------------------------------------+\x1b[0m");
    println!();
}

/// Print configuration summary.
fn print_config_summary(config: &ScheduleConfig, pid: i64) {
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

/// Print registered schedules table with next run times.
fn print_schedules_table(schedules: &[crate::commands::watch::Schedule]) {
    if schedules.is_empty() {
        println!("\x1b[33mNo schedules registered.\x1b[0m");
        println!();
        return;
    }

    println!("\x1b[1mRegistered Schedules ({}):\x1b[0m", schedules.len());
    println!("  {:<24} {:<18} {:<24}", "NAME", "CRON", "NEXT RUN");
    println!("  {}", "-".repeat(66));

    for trigger in schedules {
        let next_run = calculate_next_run(&trigger.cron)
            .map(|dt| format_relative_time(&dt))
            .unwrap_or_else(|| "invalid".to_string());

        println!(
            "  {:<24} {:<18} {}",
            truncate(&trigger.name, 24),
            truncate(&trigger.cron, 18),
            next_run
        );
    }

    println!();
    println!("\x1b[32mAutopilot running.\x1b[0m Press \x1b[1mCtrl+C\x1b[0m to stop.");
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
