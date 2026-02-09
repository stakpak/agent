//! Watch init command - create a sample configuration file.

use std::path::PathBuf;

/// Create a sample watch configuration file.
pub async fn init_config(force: bool) -> Result<(), String> {
    let config_path = get_config_path();

    // Check if config already exists
    if config_path.exists() && !force {
        println!(
            "Configuration file already exists at: {}",
            config_path.display()
        );
        println!("Use --force to overwrite.");
        return Ok(());
    }

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    // Write sample config
    std::fs::write(&config_path, SAMPLE_CONFIG)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    println!("Created watch configuration at: {}", config_path.display());
    println!("\nEdit this file to configure your triggers, then run:");
    println!("  stakpak watch run");
    println!("\nOr install as a system service:");
    println!("  stakpak watch install");

    Ok(())
}

/// Get the default config path.
fn get_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".stakpak")
        .join("watch.toml")
}

const SAMPLE_CONFIG: &str = r#"# Stakpak Watch Configuration
# 
# This file configures the autonomous agent watch service that runs scheduled tasks.
# Place this file at ~/.stakpak/watch.toml

# Default settings applied to all triggers (can be overridden per-trigger)
[defaults]
# Profile to use for agent invocation (must exist in ~/.stakpak/config.toml)
profile = "default"

# Maximum time for agent execution (default: 30m)
timeout = "30m"

# Maximum time for check scripts (default: 30s)
check_timeout = "30s"

# Enable Slack tools for agent (experimental, default: false)
# enable_slack_tools = false

# Enable subagents (default: false)
# enable_subagents = false

# Trigger definitions
# Each trigger runs on a cron schedule and optionally runs a check script first

[[triggers]]
name = "example-health-report"
# Cron expression: minute hour day-of-month month day-of-week
# This runs every day at 9 AM
schedule = "0 9 * * *"
# The prompt sent to the agent
prompt = """
Generate a system health report. Check:
- Disk usage (df -h)
- Memory usage (free -h or vm_stat on macOS)
- Running services status
- Any recent error logs

Output a summary report but DO NOT make any changes to the system.
This is a read-only health check.
"""
# Optional: check script that determines if agent should run
# Exit 0 = run agent, Exit 1 = skip, Exit 2+ = error
# check = "~/.stakpak/triggers/check-weekday.sh"

# Optional: override default timeout for this trigger
# timeout = "10m"

# Optional: board ID for tracking progress
# board_id = "board_abc123"

# Optional: enable Slack tools for this trigger (overrides default)
# enable_slack_tools = true

# Optional: enable subagents for this trigger (overrides default)
# enable_subagents = true

# Example: Git repository status check (read-only)
# [[triggers]]
# name = "repo-status"
# schedule = "0 8 * * 1-5"  # Weekdays at 8 AM
# prompt = """
# Check the status of git repositories in ~/projects:
# - List repos with uncommitted changes
# - List repos that are behind their remote
# - Summarize any stale branches (older than 30 days)
# DO NOT make any changes, just report findings.
# """

# Example: Security advisory check (read-only)
# [[triggers]]
# name = "security-advisories"
# schedule = "0 10 * * 1"  # Every Monday at 10 AM
# prompt = """
# Check for security advisories in project dependencies:
# - Run 'npm audit' or 'cargo audit' as appropriate
# - Summarize any vulnerabilities found
# - DO NOT automatically fix anything, just report
# """
# profile = "security"  # Use a specific profile with security tools
"#;
