//! Watch resume command - resume an interrupted agent run in TUI mode.

use crate::commands::watch::{RunStatus, WatchConfig, WatchDb};

/// Resume an interrupted agent run in TUI mode.
///
/// This launches the TUI with the checkpoint flag, allowing the user to
/// take over the session interactively where the watch service left off.
pub async fn resume_run(run_id: i64, force: bool) -> Result<(), String> {
    // Load configuration
    let config =
        WatchConfig::load_default().map_err(|e| format!("Failed to load watch config: {}", e))?;

    // Connect to database
    let db_path = config.db_path();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| "Invalid database path".to_string())?;

    let db = WatchDb::new(db_path_str)
        .await
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Get the run
    let run = db
        .get_run(run_id)
        .await
        .map_err(|e| format!("Failed to get run: {}", e))?;

    println!("Run #{}: {} ({})", run.id, run.trigger_name, run.status);

    // Check if run has a checkpoint to resume from
    let checkpoint_id = run.agent_last_checkpoint_id.as_ref().ok_or_else(|| {
        "Cannot resume: no checkpoint found. The agent may not have been woken or didn't create a checkpoint.".to_string()
    })?;

    println!("Checkpoint: {}", checkpoint_id);

    // Warn if run already completed
    if run.status == RunStatus::Completed && !force {
        println!("\n\x1b[33mWarning: This run already completed successfully.\x1b[0m");
        println!("Use --force to resume anyway.");
        return Ok(());
    }

    // Find the trigger config to get profile
    let trigger = config.triggers.iter().find(|t| t.name == run.trigger_name);

    let profile = trigger
        .and_then(|t| t.profile.as_ref())
        .unwrap_or(&config.defaults.profile);

    println!("\nLaunching TUI with checkpoint from run #{}...", run.id);
    println!("Profile: {}", profile);
    println!();

    // Get the current executable path
    let exe_path =
        std::env::current_exe().map_err(|e| format!("Failed to get executable path: {}", e))?;

    // Build command args for TUI mode with checkpoint
    let mut cmd = std::process::Command::new(&exe_path);
    cmd.arg("--profile").arg(profile);
    cmd.arg("-c").arg(checkpoint_id);

    // Replace current process with TUI (exec on Unix)
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(format!("Failed to exec TUI: {}", err))
    }

    #[cfg(not(unix))]
    {
        // On non-Unix, spawn and wait
        let status = cmd
            .status()
            .map_err(|e| format!("Failed to spawn TUI: {}", e))?;

        if !status.success() {
            return Err(format!("TUI exited with status: {:?}", status.code()));
        }
        Ok(())
    }
}
