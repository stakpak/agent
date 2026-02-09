//! Watch stop command - stops a running watch service.

use crate::commands::watch::{WatchConfig, is_process_running};
use std::path::Path;

/// Stop a running watch service by sending SIGTERM.
pub async fn stop_watch() -> Result<(), String> {
    // Load config to find PID file location
    let config =
        WatchConfig::load_default().map_err(|e| format!("Failed to load config: {}", e))?;

    let pid_file = config
        .db_path()
        .parent()
        .unwrap_or(Path::new("."))
        .join("watch.pid");

    // Read PID from file
    let pid_str = std::fs::read_to_string(&pid_file).map_err(|_| {
        format!(
            "No watch service running (PID file not found at {})",
            pid_file.display()
        )
    })?;

    let pid: u32 = pid_str
        .trim()
        .parse()
        .map_err(|_| "Invalid PID in PID file".to_string())?;

    // Check if process is running
    if !is_process_running(pid) {
        // Clean up stale PID file
        let _ = std::fs::remove_file(&pid_file);
        return Err(format!(
            "Watch service not running (stale PID file for PID {})",
            pid
        ));
    }

    println!("Stopping watch service (PID {})...", pid);

    // Send SIGTERM
    #[cfg(unix)]
    {
        use std::process::Command;
        let status = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .map_err(|e| format!("Failed to send SIGTERM: {}", e))?;

        if !status.success() {
            return Err(format!("Failed to stop watch service (PID {})", pid));
        }
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        let status = Command::new("taskkill")
            .arg("/PID")
            .arg(pid.to_string())
            .status()
            .map_err(|e| format!("Failed to terminate process: {}", e))?;

        if !status.success() {
            return Err(format!("Failed to stop watch service (PID {})", pid));
        }
    }

    // Wait briefly for watch service to stop
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !is_process_running(pid) {
            println!("Watch service stopped.");
            return Ok(());
        }
    }

    // If still running after 1 second, warn user
    println!("Watch service may still be shutting down (PID {})", pid);
    Ok(())
}
