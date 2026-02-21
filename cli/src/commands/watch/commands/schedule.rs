//! Autopilot schedule command - inspect or manually fire a schedule.

use crate::commands::watch::{
    RunStatus, ScheduleConfig, ScheduleDb, assemble_prompt, build_schedule_caller_context,
    is_process_running, run_check_script,
};

/// Show detailed information about a schedule.
pub async fn show_schedule(name: &str) -> Result<(), String> {
    // Load configuration
    let config = ScheduleConfig::load_default()
        .map_err(|e| format!("Failed to load watch config: {}", e))?;

    // Find the schedule
    let schedule = config
        .schedules
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Schedule '{}' not found", name))?;

    // Print schedule details
    println!("\x1b[1m{}\x1b[0m", schedule.name);
    println!();
    println!("Cron:          {}", schedule.cron);
    println!(
        "Profile:       {} {}",
        schedule.effective_profile(&config.defaults),
        if schedule.profile.is_some() {
            ""
        } else {
            "(default)"
        }
    );
    println!(
        "Timeout:       {:?} {}",
        schedule.effective_timeout(&config.defaults),
        if schedule.timeout.is_some() {
            ""
        } else {
            "(default)"
        }
    );

    if let Some(check) = &schedule.check {
        println!("Check script:  {}", check);
        println!(
            "Check timeout: {:?} {}",
            schedule.effective_check_timeout(&config.defaults),
            if schedule.check_timeout.is_some() {
                ""
            } else {
                "(default)"
            }
        );
    } else {
        println!("Check script:  none");
    }

    if let Some(board_id) = &schedule.board_id {
        println!("Board ID:      {}", board_id);
    }

    println!();
    println!("Prompt:");
    println!("---");
    println!("{}", schedule.prompt.trim());
    println!("---");

    // Show recent runs
    let db_path = config.db_path();
    if let Ok(db_path_str) = db_path.to_str().ok_or("Invalid path")
        && let Ok(db) = ScheduleDb::new(db_path_str).await
    {
        let filter = crate::commands::watch::ListRunsFilter {
            schedule_name: Some(name.to_string()),
            status: None,
            limit: Some(5),
            offset: None,
        };

        if let Ok(runs) = db.list_runs(&filter).await
            && !runs.is_empty()
        {
            println!();
            println!("Recent runs:");
            for run in runs {
                let status_str = match run.status {
                    RunStatus::Running => "\x1b[34mrunning\x1b[0m",
                    RunStatus::Completed => "\x1b[32mcompleted\x1b[0m",
                    RunStatus::Failed => "\x1b[31mfailed\x1b[0m",
                    RunStatus::Skipped => "\x1b[90mskipped\x1b[0m",
                    RunStatus::TimedOut => "\x1b[31mtimed out\x1b[0m",
                    RunStatus::Paused => "\x1b[33mpaused\x1b[0m",
                };
                let time_str = run.started_at.format("%Y-%m-%d %H:%M:%S");
                println!("  #{:<4} {} {}", run.id, time_str, status_str);
            }
            println!();
            println!(
                "Use 'stakpak autopilot schedule history {}' to inspect a run.",
                schedule.name
            );
        }
    }

    Ok(())
}

/// Manually fire a schedule or perform a dry run.
pub async fn fire_schedule(name: &str, dry_run: bool) -> Result<(), String> {
    // Load configuration
    let config = ScheduleConfig::load_default()
        .map_err(|e| format!("Failed to load watch config: {}", e))?;

    // Find the schedule
    let schedule = config
        .schedules
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Schedule '{}' not found", name))?;

    // For dry run, show what would happen without queuing
    if dry_run {
        println!("Schedule: {}", schedule.name);

        // Run check script if defined (for dry run preview)
        let check_result = if let Some(check_path) = &schedule.check {
            let expanded_path = crate::commands::watch::config::expand_tilde(check_path);
            let timeout = schedule.effective_check_timeout(&config.defaults);

            println!("Check script: {}", expanded_path.display());

            match run_check_script(&expanded_path, timeout).await {
                Ok(result) => {
                    let exit_code = result.exit_code.unwrap_or(-1);
                    println!("Check result: exit {}", exit_code);

                    if !result.stdout.trim().is_empty() {
                        println!("Check stdout:");
                        for line in result.stdout.lines() {
                            println!("  {}", line);
                        }
                    }

                    if !result.stderr.trim().is_empty() {
                        println!("Check stderr:");
                        for line in result.stderr.lines() {
                            println!("  {}", line);
                        }
                    }

                    if result.timed_out {
                        println!("\n\x1b[31mCheck script timed out\x1b[0m");
                    } else {
                        let trigger_on = schedule.effective_trigger_on(&config.defaults);
                        let should_trigger = trigger_on.should_trigger(exit_code);
                        println!("Check trigger_on: {}", trigger_on);
                        if should_trigger {
                            println!(
                                "\n\x1b[32mCheck passed (exit {} matches trigger_on={}) - agent would be woken\x1b[0m",
                                exit_code, trigger_on
                            );
                        } else {
                            println!(
                                "\n\x1b[33mCheck skipped (exit {} does not match trigger_on={}) - agent would not be woken\x1b[0m",
                                exit_code, trigger_on
                            );
                        }
                    }

                    Some(result)
                }
                Err(e) => {
                    println!("\n\x1b[31mFailed to run check script: {}\x1b[0m", e);
                    None
                }
            }
        } else {
            println!("Check script: none");
            None
        };

        // Assemble user prompt + structured caller context preview
        let prompt = assemble_prompt(schedule, check_result.as_ref());
        let caller_context = build_schedule_caller_context(schedule, check_result.as_ref());

        println!("\nAssembled prompt:");
        println!("---");
        println!("{}", prompt);
        println!("---");

        println!("\nStructured caller context:");
        for entry in caller_context {
            println!("- name: {}", entry.name);
            if let Some(priority) = entry.priority {
                println!("  priority: {}", priority);
            }
            println!("  content:");
            for line in entry.content.lines() {
                println!("    {}", line);
            }
        }

        println!("\n\x1b[33m[Dry run - schedule not queued, nothing recorded]\x1b[0m");
        return Ok(());
    }

    // Connect to database
    let db_path = config.db_path();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| "Invalid database path".to_string())?;

    let db = ScheduleDb::new(db_path_str)
        .await
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Check if autopilot service is running
    let autopilot_state = db
        .get_autopilot_state()
        .await
        .map_err(|e| format!("Failed to check autopilot state: {}", e))?;

    match autopilot_state {
        None => {
            return Err(
                "Autopilot is not running. Start it with 'stakpak autopilot up' first, or use --dry-run to preview."
                    .to_string(),
            );
        }
        Some(state) => {
            // Verify the process is actually running
            if !is_process_running(state.pid as u32) {
                return Err(
                    "Autopilot is not running (stale state). Start it with 'stakpak autopilot up' first, or use --dry-run to preview."
                        .to_string(),
                );
            }
        }
    }

    // Queue the schedule for the autopilot service to pick up
    db.insert_pending_schedule(name)
        .await
        .map_err(|e| format!("Failed to queue schedule: {}", e))?;

    println!(
        "\x1b[32m✓\x1b[0m Schedule '{}' queued for execution by autopilot service",
        name
    );
    println!(
        "  Use 'stakpak autopilot schedule history {}' to monitor progress.",
        name
    );

    Ok(())
}
