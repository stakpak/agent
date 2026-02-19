pub mod cloud_accounts;
pub mod crontabs;
pub mod git_repos;
pub mod listening_ports;
pub mod project_markers;

use std::fmt::Write;
use tokio::task::JoinSet;

/// Result of a single discovery probe.
pub struct ProbeResult {
    pub name: &'static str,
    pub output: String,
}

/// Run all discovery probes in parallel and return combined output.
pub async fn run_all() -> String {
    let mut set: JoinSet<ProbeResult> = JoinSet::new();

    let home = dirs::home_dir();
    let cwd = std::env::current_dir().ok();

    // All probes run as blocking spawn since they do filesystem I/O
    let home_c = home.clone();
    set.spawn_blocking(move || ProbeResult {
        name: "Git Repositories",
        output: git_repos::discover(home_c.as_deref()),
    });

    let home_c = home.clone();
    let cwd_c = cwd.clone();
    set.spawn_blocking(move || ProbeResult {
        name: "Project Markers",
        output: project_markers::discover(home_c.as_deref(), cwd_c.as_deref()),
    });

    set.spawn_blocking(move || ProbeResult {
        name: "Listening Ports",
        output: listening_ports::discover(),
    });

    set.spawn_blocking(move || ProbeResult {
        name: "Crontabs",
        output: crontabs::discover(),
    });

    set.spawn_blocking(move || ProbeResult {
        name: "Cloud Accounts",
        output: cloud_accounts::discover(),
    });

    // Collect results, preserving a stable order by name
    let mut results: Vec<ProbeResult> = Vec::new();
    while let Some(Ok(result)) = set.join_next().await {
        results.push(result);
    }
    results.sort_by_key(|r| r.name);

    let mut out = String::with_capacity(4096);
    for r in &results {
        if r.output.is_empty() {
            continue;
        }
        let _ = writeln!(out, "## {}\n", r.name);
        let _ = writeln!(out, "{}", r.output.trim());
        out.push('\n');
    }
    out
}
