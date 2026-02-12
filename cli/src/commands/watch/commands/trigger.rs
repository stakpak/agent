//! Watch trigger command - inspect or manually fire a trigger.

use crate::commands::watch::{
    RunStatus, WatchConfig, WatchDb, assemble_prompt, is_process_running, run_check_script,
};

/// Show detailed information about a trigger.
pub async fn show_trigger(name: &str) -> Result<(), String> {
    // Load configuration
    let config =
        WatchConfig::load_default().map_err(|e| format!("Failed to load watch config: {}", e))?;

    // Find the trigger
    let trigger = config
        .triggers
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("Trigger '{}' not found", name))?;

    // Print trigger details
    println!("\x1b[1m{}\x1b[0m", trigger.name);
    println!();
    println!("Schedule:      {}", trigger.schedule);
    println!(
        "Profile:       {} {}",
        trigger.effective_profile(&config.defaults),
        if trigger.profile.is_some() {
            ""
        } else {
            "(default)"
        }
    );
    println!(
        "Timeout:       {:?} {}",
        trigger.effective_timeout(&config.defaults),
        if trigger.timeout.is_some() {
            ""
        } else {
            "(default)"
        }
    );

    if let Some(check) = &trigger.check {
        println!("Check script:  {}", check);
        println!(
            "Check timeout: {:?} {}",
            trigger.effective_check_timeout(&config.defaults),
            if trigger.check_timeout.is_some() {
                ""
            } else {
                "(default)"
            }
        );
    } else {
        println!("Check script:  none");
    }

    if let Some(board_id) = &trigger.board_id {
        println!("Board ID:      {}", board_id);
    }

    println!();
    println!("Prompt:");
    println!("---");
    println!("{}", trigger.prompt.trim());
    println!("---");

    // Show recent runs
    let db_path = config.db_path();
    if let Ok(db_path_str) = db_path.to_str().ok_or("Invalid path")
        && let Ok(db) = WatchDb::new(db_path_str).await
    {
        let filter = crate::commands::watch::ListRunsFilter {
            trigger_name: Some(name.to_string()),
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
            println!("Use 'stakpak watch describe run <id>' to inspect a run.");
        }
    }

    Ok(())
}

/// Manually fire a trigger or perform a dry run.
pub async fn fire_trigger(name: &str, dry_run: bool) -> Result<(), String> {
    // Load configuration
    let config =
        WatchConfig::load_default().map_err(|e| format!("Failed to load watch config: {}", e))?;

    // Find the trigger
    let trigger = config
        .triggers
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("Trigger '{}' not found", name))?;

    // For dry run, show what would happen without queuing
    if dry_run {
        println!("Trigger: {}", trigger.name);

        // Run check script if defined (for dry run preview)
        let check_result = if let Some(check_path) = &trigger.check {
            let expanded_path = crate::commands::watch::config::expand_tilde(check_path);
            let timeout = trigger.effective_check_timeout(&config.defaults);

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
                        let check_trigger_on = trigger.effective_check_trigger_on(&config.defaults);
                        let should_trigger = check_trigger_on.should_trigger(exit_code);
                        println!("Check trigger_on: {}", check_trigger_on);
                        if should_trigger {
                            println!(
                                "\n\x1b[32mCheck passed (exit {} matches trigger_on={}) - agent would be woken\x1b[0m",
                                exit_code, check_trigger_on
                            );
                        } else {
                            println!(
                                "\n\x1b[33mCheck skipped (exit {} does not match trigger_on={}) - agent would not be woken\x1b[0m",
                                exit_code, check_trigger_on
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

        // Assemble prompt
        let prompt = assemble_prompt(trigger, check_result.as_ref());

        println!("\nAssembled prompt:");
        println!("---");
        println!("{}", prompt);
        println!("---");

        println!("\n\x1b[33m[Dry run - trigger not queued, nothing recorded]\x1b[0m");
        return Ok(());
    }

    // Connect to database
    let db_path = config.db_path();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| "Invalid database path".to_string())?;

    let db = WatchDb::new(db_path_str)
        .await
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Check if watch service is running
    let watch_state = db
        .get_watch_state()
        .await
        .map_err(|e| format!("Failed to check watch state: {}", e))?;

    match watch_state {
        None => {
            return Err(
                "Watch service is not running. Start it with 'stakpak watch run' first, or use --dry-run to preview."
                    .to_string(),
            );
        }
        Some(state) => {
            // Verify the process is actually running
            if !is_process_running(state.pid as u32) {
                return Err(
                    "Watch process is not running (stale state). Start it with 'stakpak watch run' first, or use --dry-run to preview."
                        .to_string(),
                );
            }
        }
    }

    // Queue the trigger for the watch service to pick up
    db.insert_pending_trigger(name)
        .await
        .map_err(|e| format!("Failed to queue trigger: {}", e))?;

    println!(
        "\x1b[32m✓\x1b[0m Trigger '{}' queued for execution by watch service",
        name
    );
    println!("  Use 'stakpak watch get runs' to monitor progress.");

    Ok(())
}
