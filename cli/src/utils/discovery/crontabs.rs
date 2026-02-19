use std::fmt::Write;
use std::process::Command;

/// Discover cron jobs / scheduled tasks for the current user.
/// Linux/macOS: parse crontab. macOS also checks launchd. Windows: schtasks.
pub fn discover() -> String {
    let os = std::env::consts::OS;
    match os {
        "linux" => discover_linux(),
        "macos" => discover_macos(),
        "windows" => discover_windows(),
        _ => discover_linux(), // best effort
    }
}

fn discover_linux() -> String {
    let mut out = String::with_capacity(512);

    // User crontab
    if let Ok(output) = Command::new("crontab").arg("-l").output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let jobs: Vec<&str> = stdout
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .collect();
        if !jobs.is_empty() {
            let _ = writeln!(out, "### User Crontab\n");
            for job in &jobs {
                let _ = writeln!(out, "- {}", job.trim());
            }
            out.push('\n');
        }
    }

    // System cron dirs (existence check only)
    let cron_dirs = [
        "/etc/cron.d",
        "/etc/cron.daily",
        "/etc/cron.hourly",
        "/etc/cron.weekly",
        "/etc/cron.monthly",
    ];
    let mut sys_entries = Vec::new();
    for dir in &cron_dirs {
        let path = std::path::Path::new(dir);
        if path.exists()
            && let Ok(entries) = std::fs::read_dir(path)
        {
            let count = entries
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .count();
            if count > 0 {
                sys_entries.push(format!("- {} ({} entries)", dir, count));
            }
        }
    }
    if !sys_entries.is_empty() {
        let _ = writeln!(out, "### System Cron Dirs\n");
        for entry in &sys_entries {
            let _ = writeln!(out, "{}", entry);
        }
        out.push('\n');
    }

    // Systemd timers
    if let Ok(output) = Command::new("systemctl")
        .args(["list-timers", "--no-pager", "--no-legend"])
        .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let timers: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        if !timers.is_empty() {
            let _ = writeln!(out, "### Systemd Timers\n");
            for timer in timers.iter().take(20) {
                let _ = writeln!(out, "- {}", timer.trim());
            }
            out.push('\n');
        }
    }

    if out.is_empty() {
        "(no cron jobs or scheduled tasks found)\n".to_string()
    } else {
        out
    }
}

fn discover_macos() -> String {
    let mut out = String::with_capacity(512);

    // User crontab
    if let Ok(output) = Command::new("crontab").arg("-l").output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let jobs: Vec<&str> = stdout
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .collect();
        if !jobs.is_empty() {
            let _ = writeln!(out, "### User Crontab\n");
            for job in &jobs {
                let _ = writeln!(out, "- {}", job.trim());
            }
            out.push('\n');
        }
    }

    // LaunchAgents (user)
    let home = dirs::home_dir();
    if let Some(ref h) = home {
        let launch_agents = h.join("Library/LaunchAgents");
        if launch_agents.exists()
            && let Ok(entries) = std::fs::read_dir(&launch_agents)
        {
            let plists: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".plist") {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();
            if !plists.is_empty() {
                let _ = writeln!(out, "### User LaunchAgents\n");
                for plist in plists.iter().take(20) {
                    let _ = writeln!(out, "- {}", plist);
                }
                out.push('\n');
            }
        }
    }

    if out.is_empty() {
        "(no cron jobs or scheduled tasks found)\n".to_string()
    } else {
        out
    }
}

fn discover_windows() -> String {
    let output = match Command::new("schtasks")
        .args(["/Query", "/FO", "LIST", "/V"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return "(failed to query scheduled tasks)\n".to_string(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut tasks = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_status: Option<String> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("TaskName:") {
            current_name = Some(name.trim().to_string());
        } else if let Some(status) = trimmed.strip_prefix("Status:") {
            current_status = Some(status.trim().to_string());
        } else if trimmed.is_empty() {
            if let Some(ref name) = current_name {
                // Skip system tasks
                if !name.starts_with("\\Microsoft\\") {
                    tasks.push(format!(
                        "- {} ({})",
                        name,
                        current_status.as_deref().unwrap_or("?")
                    ));
                }
            }
            current_name = None;
            current_status = None;
        }
    }

    if tasks.is_empty() {
        return "(no user scheduled tasks found)\n".to_string();
    }

    let mut out = String::with_capacity(tasks.len() * 60);
    let _ = writeln!(out, "### Scheduled Tasks\n");
    for task in tasks.iter().take(30) {
        let _ = writeln!(out, "{}", task);
    }
    out
}
