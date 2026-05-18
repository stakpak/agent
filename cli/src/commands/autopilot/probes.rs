use crate::config::AppConfig;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::Path;

const CRITICAL_SANDBOX_CONTAINER_PATHS: &[&str] = &[
    "/home/agent/.stakpak/config.toml",
    "/home/agent/.aws",
    "/home/agent/.config/gcloud",
    "/home/agent/.azure",
    "/home/agent/.kube",
    "/home/agent/.ssh",
];

const LINUX_MEMINFO_PATH: &str = "/proc/meminfo";
const MIN_RAM_MB_BLOCKING: u64 = 1024;
const MIN_EFFECTIVE_MEMORY_MB_WARNING: u64 = 1536;
const MIN_DISK_MB_WARNING: u64 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeMode {
    Startup,
    Doctor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeSeverity {
    Blocking,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remediation {
    Manual {
        summary: String,
        command: Option<String>,
    },
    Suggested {
        summary: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub id: &'static str,
    pub title: &'static str,
    pub severity: ProbeSeverity,
    pub status: ProbeStatus,
    pub summary: String,
    pub details: Option<String>,
    pub remediation: Option<Remediation>,
}

impl ProbeResult {
    #[cfg(test)]
    pub fn is_blocking_failure(&self) -> bool {
        self.status == ProbeStatus::Fail && self.severity == ProbeSeverity::Blocking
    }

    #[cfg(test)]
    pub fn is_warning(&self) -> bool {
        self.status == ProbeStatus::Fail && self.severity == ProbeSeverity::Warning
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSummary {
    pub blocking_failures: usize,
    pub warnings: usize,
    pub passes: usize,
    pub skipped: usize,
}

pub fn summarize(results: &[ProbeResult]) -> ProbeSummary {
    let mut summary = ProbeSummary {
        blocking_failures: 0,
        warnings: 0,
        passes: 0,
        skipped: 0,
    };

    for result in results {
        match result.status {
            ProbeStatus::Pass => summary.passes += 1,
            ProbeStatus::Skip => summary.skipped += 1,
            ProbeStatus::Fail => {
                if result.severity == ProbeSeverity::Blocking {
                    summary.blocking_failures += 1;
                } else if result.severity == ProbeSeverity::Warning {
                    summary.warnings += 1;
                } else {
                    debug_assert!(
                        result.severity != ProbeSeverity::Info,
                        "Info-severity probe failures are not expected to be actionable"
                    );
                }
            }
        }
    }

    summary
}

pub struct AutopilotProbeContext<'a> {
    pub app_config: &'a AppConfig,
    pub bind_addr: Option<&'a str>,
    pub server_reachable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSnapshot {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait ProbeEnvironment: Send + Sync {
    fn command_output(&self, program: &str, args: &[&str]) -> Result<CommandSnapshot, String>;
    fn read_to_string(&self, path: &Path) -> Result<String, String>;
    fn path_exists(&self, path: &Path) -> bool;
    fn can_read_path(&self, path: &Path) -> Result<(), String>;
    fn current_username(&self) -> Option<String>;
    fn can_bind_addr(&self, addr: &str) -> Result<(), String>;
    /// Linux distro ID parsed from /etc/os-release (e.g. "amzn", "ubuntu", "debian", "fedora").
    /// Returns None on non-Linux hosts or when /etc/os-release is unavailable/unparseable.
    fn os_id(&self) -> Option<String> {
        let contents = self.read_to_string(Path::new("/etc/os-release")).ok()?;
        parse_os_release_id(&contents)
    }
}

pub(crate) fn parse_os_release_id(contents: &str) -> Option<String> {
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("ID=") {
            let trimmed = value.trim().trim_matches('"').trim_matches('\'');
            if !trimmed.is_empty() {
                return Some(trimmed.to_ascii_lowercase());
            }
        }
    }
    None
}

fn docker_install_remediation(os_id: Option<&str>) -> (String, String) {
    match os_id {
        Some("amzn" | "rhel" | "fedora" | "rocky" | "almalinux" | "centos") => (
            "Install Docker, then rerun stakpak up".to_string(),
            "sudo dnf install -y docker && sudo systemctl enable --now docker && sudo usermod -aG docker $USER".to_string(),
        ),
        Some("ubuntu" | "debian") => (
            "Install Docker, then rerun stakpak up".to_string(),
            "sudo apt-get update && sudo apt-get install -y docker.io && sudo usermod -aG docker $USER".to_string(),
        ),
        _ => (
            "Install Docker for your distribution, then rerun stakpak up".to_string(),
            "See https://docs.docker.com/engine/install/ — after install, run: sudo usermod -aG docker $USER".to_string(),
        ),
    }
}

pub struct RealProbeEnvironment;

impl ProbeEnvironment for RealProbeEnvironment {
    fn command_output(&self, program: &str, args: &[&str]) -> Result<CommandSnapshot, String> {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;

        Ok(CommandSnapshot {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    fn read_to_string(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn can_read_path(&self, path: &Path) -> Result<(), String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            std::fs::read_dir(path)
                .map(|_| ())
                .map_err(|error| error.to_string())
        } else {
            std::fs::File::open(path)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }

    fn current_username(&self) -> Option<String> {
        std::env::var("USER")
            .ok()
            .and_then(|value| normalize_non_empty(&value))
    }

    fn can_bind_addr(&self, addr: &str) -> Result<(), String> {
        // This is a single short bind syscall used for local readiness probing.
        // Keeping it synchronous avoids making the probe environment async-only.
        TcpListener::bind(addr)
            .map(|_listener| ())
            .map_err(|error| error.to_string())
    }
}

pub fn run_autopilot_probes(
    mode: ProbeMode,
    ctx: &AutopilotProbeContext<'_>,
    env: &dyn ProbeEnvironment,
) -> Vec<ProbeResult> {
    let mut results = Vec::new();

    let credentials = probe_credentials(ctx);
    results.push(credentials);

    let docker_installed = probe_docker_installed(env);
    let docker_installed_ok = docker_installed.status == ProbeStatus::Pass;
    results.push(docker_installed);

    let docker_accessible = if docker_installed_ok {
        probe_docker_accessible(env)
    } else {
        ProbeResult {
            id: "docker_accessible",
            title: "Docker daemon access",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Skip,
            summary: "Skipped because Docker is not installed".to_string(),
            details: None,
            remediation: None,
        }
    };
    let docker_accessible_ok = docker_accessible.status == ProbeStatus::Pass;
    results.push(docker_accessible);

    // ProbeMode currently only has Startup/Doctor — both want this check, so no
    // mode gating needed today. Revisit if a lighter mode (e.g. Healthcheck) is added.
    if docker_accessible_ok {
        results.push(probe_docker_user_systemd(env));
    }

    results.push(probe_memory(env));

    if let Some(bind_addr) = ctx.bind_addr {
        results.push(probe_bind_port(bind_addr, ctx.server_reachable, env));
    }

    if matches!(mode, ProbeMode::Startup | ProbeMode::Doctor) {
        results.push(probe_systemd_linger(env));
    }

    if matches!(mode, ProbeMode::Doctor) {
        let disk_probe_path = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        results.push(probe_disk_space(disk_probe_path.as_path(), env));
        results.push(probe_sandbox_mount_inputs(ctx, env));
    }

    results
}

pub fn probe_credentials(ctx: &AutopilotProbeContext<'_>) -> ProbeResult {
    let has_stakpak_key = ctx.app_config.get_stakpak_api_key().is_some();
    let has_provider_keys = !ctx
        .app_config
        .get_llm_provider_config()
        .providers
        .is_empty();

    if has_stakpak_key || has_provider_keys {
        ProbeResult {
            id: "credentials",
            title: "Credentials",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Pass,
            summary: "Credentials configured".to_string(),
            details: None,
            remediation: None,
        }
    } else {
        ProbeResult {
            id: "credentials",
            title: "Credentials",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Fail,
            summary: "No provider credentials configured".to_string(),
            details: Some(
                "Autopilot needs either a Stakpak API key or a configured local provider."
                    .to_string(),
            ),
            remediation: Some(Remediation::Manual {
                summary: "Authenticate before starting autopilot".to_string(),
                command: Some("stakpak auth login --api-key <key>".to_string()),
            }),
        }
    }
}

pub fn probe_docker_installed(env: &dyn ProbeEnvironment) -> ProbeResult {
    let snapshot = env.command_output("docker", &["--version"]);
    if let Ok(ref snap) = snapshot
        && snap.success
    {
        return ProbeResult {
            id: "docker_installed",
            title: "Docker",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Pass,
            summary: "Docker is installed".to_string(),
            details: first_non_empty_line(&snap.stdout),
            remediation: None,
        };
    }

    let (summary, command) = docker_install_remediation(env.os_id().as_deref());
    match snapshot {
        Ok(snap) => ProbeResult {
            id: "docker_installed",
            title: "Docker",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Fail,
            summary: "Docker is installed but failed to report its version".to_string(),
            details: command_details(&snap),
            remediation: Some(Remediation::Manual {
                summary: format!("Reinstall or repair Docker. {summary}"),
                command: Some(command),
            }),
        },
        Err(error) => ProbeResult {
            id: "docker_installed",
            title: "Docker",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Fail,
            summary: "Docker is not installed".to_string(),
            details: Some(format!("Command error: {error}")),
            remediation: Some(Remediation::Manual {
                summary,
                command: Some(command),
            }),
        },
    }
}

pub fn probe_docker_accessible(env: &dyn ProbeEnvironment) -> ProbeResult {
    match env.command_output("docker", &["ps"]) {
        Ok(snapshot) if snapshot.success => ProbeResult {
            id: "docker_accessible",
            title: "Docker daemon access",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Pass,
            summary: "Docker is accessible to the current user".to_string(),
            details: None,
            remediation: None,
        },
        Ok(snapshot) => {
            let details = command_details(&snapshot);
            let remediation = if combined_output(&snapshot)
                .to_ascii_lowercase()
                .contains("permission denied")
            {
                Remediation::Manual {
                    summary: "Grant the current user access to the Docker daemon".to_string(),
                    command: Some(
                        "sudo usermod -aG docker $USER && newgrp docker && docker ps".to_string(),
                    ),
                }
            } else {
                Remediation::Manual {
                    summary: "Start Docker and verify daemon access".to_string(),
                    command: Some("docker ps".to_string()),
                }
            };

            ProbeResult {
                id: "docker_accessible",
                title: "Docker daemon access",
                severity: ProbeSeverity::Blocking,
                status: ProbeStatus::Fail,
                summary: "Docker is installed but not accessible".to_string(),
                details,
                remediation: Some(remediation),
            }
        }
        Err(error) => ProbeResult {
            id: "docker_accessible",
            title: "Docker daemon access",
            severity: ProbeSeverity::Blocking,
            status: ProbeStatus::Fail,
            summary: "Docker is installed but not accessible".to_string(),
            details: Some(format!("Command error: {error}")),
            remediation: Some(Remediation::Manual {
                summary: "Verify Docker is installed and the daemon is running".to_string(),
                command: Some("docker ps".to_string()),
            }),
        },
    }
}

pub fn probe_memory(env: &dyn ProbeEnvironment) -> ProbeResult {
    let meminfo_path = Path::new(LINUX_MEMINFO_PATH);
    if !env.path_exists(meminfo_path) {
        return ProbeResult {
            id: "memory",
            title: "Memory",
            severity: ProbeSeverity::Info,
            status: ProbeStatus::Skip,
            summary: "Memory probe is only available on Linux hosts".to_string(),
            details: None,
            remediation: None,
        };
    }

    let meminfo = match env.read_to_string(meminfo_path) {
        Ok(value) => value,
        Err(error) => {
            return ProbeResult {
                id: "memory",
                title: "Memory",
                severity: ProbeSeverity::Warning,
                status: ProbeStatus::Fail,
                summary: "Unable to inspect host memory".to_string(),
                details: Some(error),
                remediation: Some(Remediation::Suggested {
                    summary: "Verify the host has at least 2GB RAM or configured swap".to_string(),
                }),
            };
        }
    };

    match parse_meminfo(&meminfo) {
        Ok(snapshot) if snapshot.ram_mb < MIN_RAM_MB_BLOCKING && snapshot.swap_mb == 0 => {
            ProbeResult {
                id: "memory",
                title: "Memory",
                severity: ProbeSeverity::Blocking,
                status: ProbeStatus::Fail,
                summary: format!(
                    "Insufficient memory: {}MB RAM, no swap configured",
                    snapshot.ram_mb
                ),
                details: Some(
                    "Autopilot with sandbox is likely to OOM on this host.".to_string(),
                ),
                remediation: Some(Remediation::Suggested {
                    summary: "Use a 2GB+ instance or add swap before starting autopilot"
                        .to_string(),
                }),
            }
        }
        Ok(snapshot)
            if snapshot.ram_mb.saturating_add(snapshot.swap_mb)
                < MIN_EFFECTIVE_MEMORY_MB_WARNING =>
        {
            ProbeResult {
                id: "memory",
                title: "Memory",
                severity: ProbeSeverity::Warning,
                status: ProbeStatus::Fail,
                summary: format!(
                    "Low memory: {}MB RAM + {}MB swap",
                    snapshot.ram_mb, snapshot.swap_mb
                ),
                details: Some(
                    "Startup may work, but 2GB+ total memory is recommended for reliable autopilot + sandbox runs."
                        .to_string(),
                ),
                remediation: Some(Remediation::Suggested {
                    summary: "Increase instance size or add swap for better headroom"
                        .to_string(),
                }),
            }
        }
        Ok(snapshot) => ProbeResult {
            id: "memory",
            title: "Memory",
            severity: ProbeSeverity::Info,
            status: ProbeStatus::Pass,
            summary: format!(
                "Memory looks healthy: {}MB RAM + {}MB swap",
                snapshot.ram_mb, snapshot.swap_mb
            ),
            details: None,
            remediation: None,
        },
        Err(error) => ProbeResult {
            id: "memory",
            title: "Memory",
            severity: ProbeSeverity::Warning,
            status: ProbeStatus::Fail,
            summary: "Unable to parse host memory information".to_string(),
            details: Some(error),
            remediation: Some(Remediation::Suggested {
                summary: "Verify the host has at least 2GB RAM or configured swap"
                    .to_string(),
            }),
        },
    }
}

pub fn probe_bind_port(
    bind_addr: &str,
    server_reachable: bool,
    env: &dyn ProbeEnvironment,
) -> ProbeResult {
    if server_reachable {
        return ProbeResult {
            id: "bind_port",
            title: "Bind port",
            severity: ProbeSeverity::Info,
            status: ProbeStatus::Pass,
            summary: format!("Server is already reachable on {bind_addr}"),
            details: None,
            remediation: None,
        };
    }

    match env.can_bind_addr(bind_addr) {
        Ok(()) => ProbeResult {
            id: "bind_port",
            title: "Bind port",
            severity: ProbeSeverity::Info,
            status: ProbeStatus::Pass,
            summary: format!("Bind address {bind_addr} is available"),
            details: None,
            remediation: None,
        },
        Err(error) => ProbeResult {
            id: "bind_port",
            title: "Bind port",
            severity: ProbeSeverity::Warning,
            status: ProbeStatus::Fail,
            summary: format!("Port {bind_addr} is already in use or unavailable"),
            details: Some(error),
            remediation: Some(Remediation::Manual {
                summary: "Stop the existing process or choose another bind address".to_string(),
                command: Some(
                    "stakpak down && systemctl --user status stakpak-autopilot".to_string(),
                ),
            }),
        },
    }
}

/// Detect the silent-failure case where the calling shell can reach the Docker
/// daemon but the user's systemd manager cannot — typically after `usermod -aG
/// docker $USER` was run without restarting `user@UID.service`. The systemd
/// manager retains the old (group-less) credentials, so any service it launches
/// (including autopilot) hits "permission denied on /var/run/docker.sock" and
/// crash-loops without surfacing a useful error.
pub fn probe_docker_user_systemd(env: &dyn ProbeEnvironment) -> ProbeResult {
    let make = |severity, status, summary: String, details, remediation| ProbeResult {
        id: "docker_user_systemd",
        title: "Docker access via systemd user manager",
        severity,
        status,
        summary,
        details,
        remediation,
    };

    if !env.path_exists(Path::new("/etc/os-release")) {
        return make(
            ProbeSeverity::Info,
            ProbeStatus::Skip,
            "Probe is only available on Linux hosts".to_string(),
            None,
            None,
        );
    }

    let snapshot = env.command_output(
        "systemd-run",
        &[
            "--user",
            "--pipe",
            "--wait",
            "--quiet",
            "--collect",
            "docker",
            "ps",
        ],
    );
    match snapshot {
        Ok(snap) if snap.success => make(
            ProbeSeverity::Blocking,
            ProbeStatus::Pass,
            "systemd user manager can reach the Docker daemon".to_string(),
            None,
            None,
        ),
        Ok(snap) if combined_output(&snap).to_ascii_lowercase().contains("permission denied") => {
            make(
                ProbeSeverity::Blocking,
                ProbeStatus::Fail,
                "systemd user manager cannot reach Docker (likely stale group membership)".to_string(),
                command_details(&snap),
                Some(Remediation::Manual {
                    summary: "Restart the user systemd manager so it picks up docker group membership".to_string(),
                    command: Some(
                        "sudo systemctl restart user@$(id -u).service && systemd-run --user --pipe --wait --quiet docker ps".to_string(),
                    ),
                }),
            )
        }
        Ok(snap) => make(
            ProbeSeverity::Info,
            ProbeStatus::Skip,
            "Unable to verify Docker access from systemd user manager".to_string(),
            command_details(&snap),
            None,
        ),
        Err(error) => make(
            ProbeSeverity::Info,
            ProbeStatus::Skip,
            "systemd-run is unavailable; skipping check".to_string(),
            Some(error),
            None,
        ),
    }
}

pub fn probe_systemd_linger(env: &dyn ProbeEnvironment) -> ProbeResult {
    let username = match env.current_username() {
        Some(value) => value,
        None => {
            return ProbeResult {
                id: "systemd_linger",
                title: "Systemd linger",
                severity: ProbeSeverity::Info,
                status: ProbeStatus::Skip,
                summary: "Could not determine current user for linger check".to_string(),
                details: None,
                remediation: None,
            };
        }
    };

    match env.command_output(
        "loginctl",
        &["show-user", &username, "--property=Linger", "--value"],
    ) {
        Ok(snapshot) if snapshot.success => {
            let value = snapshot.stdout.trim().to_ascii_lowercase();
            if value == "yes" || value == "true" {
                ProbeResult {
                    id: "systemd_linger",
                    title: "Systemd linger",
                    severity: ProbeSeverity::Info,
                    status: ProbeStatus::Pass,
                    summary: "Systemd linger is enabled".to_string(),
                    details: None,
                    remediation: None,
                }
            } else {
                ProbeResult {
                    id: "systemd_linger",
                    title: "Systemd linger",
                    severity: ProbeSeverity::Warning,
                    status: ProbeStatus::Fail,
                    summary: "Systemd linger is disabled; user services may stop after logout"
                        .to_string(),
                    details: None,
                    remediation: Some(Remediation::Manual {
                        summary: "Enable linger for the current user".to_string(),
                        command: Some(format!("sudo loginctl enable-linger {username}")),
                    }),
                }
            }
        }
        Ok(snapshot) => ProbeResult {
            id: "systemd_linger",
            title: "Systemd linger",
            severity: ProbeSeverity::Info,
            status: ProbeStatus::Skip,
            summary: "Unable to determine linger status".to_string(),
            details: command_details(&snapshot),
            remediation: None,
        },
        Err(error) => ProbeResult {
            id: "systemd_linger",
            title: "Systemd linger",
            severity: ProbeSeverity::Info,
            status: ProbeStatus::Skip,
            summary: "loginctl is unavailable; skipping linger check".to_string(),
            details: Some(error),
            remediation: None,
        },
    }
}

pub fn probe_disk_space(target_path: &Path, env: &dyn ProbeEnvironment) -> ProbeResult {
    let target = target_path.to_string_lossy().to_string();

    match env.command_output("df", &["-Pk", target.as_str()]) {
        Ok(snapshot) if snapshot.success => match parse_df_available_mb(&snapshot.stdout) {
            Ok(available_mb) if available_mb < MIN_DISK_MB_WARNING => ProbeResult {
                id: "disk_space",
                title: "Disk space",
                severity: ProbeSeverity::Warning,
                status: ProbeStatus::Fail,
                summary: format!(
                    "Low disk space: {}MB available at {}",
                    available_mb,
                    target_path.display()
                ),
                details: Some(
                    "Docker image pulls, logs, and runtime state may fail on a nearly full disk."
                        .to_string(),
                ),
                remediation: Some(Remediation::Suggested {
                    summary: "Free disk space or expand the volume before relying on autopilot"
                        .to_string(),
                }),
            },
            Ok(available_mb) => ProbeResult {
                id: "disk_space",
                title: "Disk space",
                severity: ProbeSeverity::Info,
                status: ProbeStatus::Pass,
                summary: format!(
                    "Disk space looks healthy: {}MB available at {}",
                    available_mb,
                    target_path.display()
                ),
                details: None,
                remediation: None,
            },
            Err(error) => ProbeResult {
                id: "disk_space",
                title: "Disk space",
                severity: ProbeSeverity::Warning,
                status: ProbeStatus::Skip,
                summary: "Unable to parse disk space information".to_string(),
                details: Some(error),
                remediation: None,
            },
        },
        Ok(snapshot) => ProbeResult {
            id: "disk_space",
            title: "Disk space",
            severity: ProbeSeverity::Warning,
            status: ProbeStatus::Skip,
            summary: "Disk space probe could not run successfully".to_string(),
            details: command_details(&snapshot),
            remediation: None,
        },
        Err(error) => ProbeResult {
            id: "disk_space",
            title: "Disk space",
            severity: ProbeSeverity::Warning,
            status: ProbeStatus::Skip,
            summary: "Disk space probe is unavailable on this host".to_string(),
            details: Some(error),
            remediation: None,
        },
    }
}

pub fn probe_sandbox_mount_inputs(
    ctx: &AutopilotProbeContext<'_>,
    env: &dyn ProbeEnvironment,
) -> ProbeResult {
    let volumes = crate::commands::warden::prepare_volumes(ctx.app_config, false);
    let mut checked = 0usize;
    let mut issues = Vec::new();

    for volume in volumes {
        let expanded = stakpak_shared::container::expand_volume_path(&volume);
        let (host_path, container_path) = match parse_volume_mapping(&expanded) {
            Some(parts) => parts,
            None => continue,
        };

        if stakpak_shared::container::is_named_volume(host_path)
            || !is_critical_sandbox_container_path(container_path)
        {
            continue;
        }

        let host_path = Path::new(host_path);
        if !env.path_exists(host_path) {
            continue;
        }

        checked += 1;
        if let Err(error) = env.can_read_path(host_path) {
            issues.push(format!(
                "{} — {}",
                host_path.display(),
                truncate_chars(error.trim(), 160)
            ));
        }
    }

    if issues.is_empty() {
        return ProbeResult {
            id: "sandbox_mount_inputs",
            title: "Sandbox mount inputs",
            severity: ProbeSeverity::Info,
            status: if checked == 0 {
                ProbeStatus::Skip
            } else {
                ProbeStatus::Pass
            },
            summary: if checked == 0 {
                "No critical sandbox mount inputs detected".to_string()
            } else {
                "Critical sandbox mount inputs look accessible".to_string()
            },
            details: None,
            remediation: None,
        };
    }

    ProbeResult {
        id: "sandbox_mount_inputs",
        title: "Sandbox mount inputs",
        severity: ProbeSeverity::Warning,
        status: ProbeStatus::Fail,
        summary: format!(
            "{} critical sandbox mount input(s) may be unreadable",
            issues.len()
        ),
        details: Some(issues.join("\n")),
        remediation: Some(Remediation::Suggested {
            summary:
                "Verify these files/directories are readable by the invoking user. Stakpak maps the host UID/GID into the sandbox; do not loosen secret file permissions globally."
                    .to_string(),
        }),
    }
}

fn parse_df_available_mb(output: &str) -> Result<u64, String> {
    let data_line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("Filesystem"))
        .ok_or_else(|| "No filesystem rows returned by df".to_string())?;

    let columns: Vec<&str> = data_line.split_whitespace().collect();
    if columns.len() < 4 {
        return Err(format!("Unexpected df output row: {data_line}"));
    }

    let available_kb = columns[3]
        .parse::<u64>()
        .map_err(|error| format!("Invalid available-kb value '{}': {error}", columns[3]))?;
    Ok(available_kb / 1024)
}

fn parse_volume_mapping(volume: &str) -> Option<(&str, &str)> {
    let mut parts = volume.rsplitn(3, ':');
    let last = parts.next()?;
    let middle = parts.next()?;
    let first = parts.next();

    if let Some(host_path) = first {
        Some((host_path, middle))
    } else {
        Some((middle, last))
    }
}

fn is_critical_sandbox_container_path(container_path: &str) -> bool {
    CRITICAL_SANDBOX_CONTAINER_PATHS.iter().any(|prefix| {
        container_path == *prefix || container_path.starts_with(&format!("{prefix}/"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MemorySnapshot {
    ram_mb: u64,
    swap_mb: u64,
}

fn parse_meminfo(input: &str) -> Result<MemorySnapshot, String> {
    let mut values = HashMap::new();

    for line in input.lines() {
        if let Some((key, value)) = line.split_once(':') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let ram_kb = parse_meminfo_kb(values.get("MemTotal"), "MemTotal")?;
    let swap_kb = parse_meminfo_kb(values.get("SwapTotal"), "SwapTotal")?;

    Ok(MemorySnapshot {
        ram_mb: ram_kb / 1024,
        swap_mb: swap_kb / 1024,
    })
}

fn parse_meminfo_kb(value: Option<&String>, field: &str) -> Result<u64, String> {
    let raw = value.ok_or_else(|| format!("Missing {field} in /proc/meminfo"))?;
    let number = raw
        .split_whitespace()
        .next()
        .ok_or_else(|| format!("Missing numeric value for {field}"))?;

    number
        .parse::<u64>()
        .map_err(|error| format!("Invalid {field} value '{number}': {error}"))
}

fn combined_output(snapshot: &CommandSnapshot) -> String {
    let stderr = snapshot.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = snapshot.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }

    String::new()
}

fn command_details(snapshot: &CommandSnapshot) -> Option<String> {
    normalize_non_empty(&combined_output(snapshot)).map(|value| truncate_chars(&value, 240))
}

fn first_non_empty_line(value: &str) -> Option<String> {
    value
        .lines()
        .find_map(normalize_non_empty)
        .map(|line| truncate_chars(&line, 240))
}

fn normalize_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_string();
    }

    let mut truncated: String = value.chars().take(max_chars.saturating_sub(3)).collect();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderType, WardenConfig};
    use stakpak_shared::models::llm::ProviderConfig;
    use std::path::PathBuf;

    #[derive(Default)]
    struct MockProbeEnvironment {
        commands: HashMap<(String, Vec<String>), Result<CommandSnapshot, String>>,
        files: HashMap<PathBuf, String>,
        path_access: HashMap<PathBuf, Result<(), String>>,
        bind_results: HashMap<String, Result<(), String>>,
        username: Option<String>,
    }

    impl MockProbeEnvironment {
        fn with_command(
            mut self,
            program: &str,
            args: &[&str],
            result: Result<CommandSnapshot, String>,
        ) -> Self {
            self.commands.insert(
                (
                    program.to_string(),
                    args.iter().map(|value| (*value).to_string()).collect(),
                ),
                result,
            );
            self
        }

        fn with_file(mut self, path: &str, contents: &str) -> Self {
            let path = PathBuf::from(path);
            self.files.insert(path.clone(), contents.to_string());
            self.path_access.insert(path, Ok(()));
            self
        }

        fn with_path_access(mut self, path: &str, result: Result<(), String>) -> Self {
            self.path_access.insert(PathBuf::from(path), result);
            self
        }

        fn with_bind_result(mut self, addr: &str, result: Result<(), String>) -> Self {
            self.bind_results.insert(addr.to_string(), result);
            self
        }

        fn with_username(mut self, username: &str) -> Self {
            self.username = Some(username.to_string());
            self
        }
    }

    impl ProbeEnvironment for MockProbeEnvironment {
        fn command_output(&self, program: &str, args: &[&str]) -> Result<CommandSnapshot, String> {
            self.commands
                .get(&(
                    program.to_string(),
                    args.iter().map(|value| (*value).to_string()).collect(),
                ))
                .cloned()
                .unwrap_or_else(|| Err(format!("unexpected command: {program} {}", args.join(" "))))
        }

        fn read_to_string(&self, path: &Path) -> Result<String, String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("missing file: {}", path.display()))
        }

        fn path_exists(&self, path: &Path) -> bool {
            self.files.contains_key(path) || self.path_access.contains_key(path)
        }

        fn can_read_path(&self, path: &Path) -> Result<(), String> {
            self.path_access.get(path).cloned().unwrap_or(Ok(()))
        }

        fn current_username(&self) -> Option<String> {
            self.username.clone()
        }

        fn can_bind_addr(&self, addr: &str) -> Result<(), String> {
            self.bind_results.get(addr).cloned().unwrap_or(Ok(()))
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            api_endpoint: "https://api.stakpak.dev".to_string(),
            api_key: Some("key".to_string()),
            provider: ProviderType::Remote,
            mcp_server_host: None,
            machine_name: None,
            auto_append_gitignore: None,
            profile_name: "default".to_string(),
            config_path: "config.toml".to_string(),
            allowed_tools: None,
            auto_approve: None,
            subagent: None,
            rulebooks: None,
            warden: None,
            providers: HashMap::<String, ProviderConfig>::new(),
            model: None,
            system_prompt: None,
            max_turns: None,
            anonymous_id: None,
            collect_telemetry: None,
            editor: None,
            recent_models: Vec::new(),
        }
    }

    #[test]
    fn credentials_missing_is_blocking_failure() {
        let mut config = test_config();
        config.api_key = None;
        let ctx = AutopilotProbeContext {
            app_config: &config,
            bind_addr: None,
            server_reachable: false,
        };

        let result = probe_credentials(&ctx);
        assert!(result.is_blocking_failure());
        assert!(
            result
                .summary
                .contains("No provider credentials configured")
        );
    }

    #[test]
    fn docker_missing_is_blocking_failure() {
        let env = MockProbeEnvironment::default().with_command(
            "docker",
            &["--version"],
            Err("No such file or directory".to_string()),
        );

        let result = probe_docker_installed(&env);
        assert!(result.is_blocking_failure());
        assert_eq!(result.summary, "Docker is not installed");
    }

    #[test]
    fn docker_permission_denied_has_group_fix() {
        let env = MockProbeEnvironment::default().with_command(
            "docker",
            &["ps"],
            Ok(CommandSnapshot {
                success: false,
                stdout: String::new(),
                stderr: "permission denied while trying to connect to the Docker daemon socket"
                    .to_string(),
            }),
        );

        let result = probe_docker_accessible(&env);
        assert!(result.is_blocking_failure());
        let remediation = result.remediation.expect("remediation");
        match remediation {
            Remediation::Manual { command, .. } => {
                let command = command.expect("command");
                assert!(command.contains("usermod -aG docker"));
            }
            Remediation::Suggested { .. } => panic!("expected manual remediation"),
        }
    }

    #[test]
    fn docker_user_systemd_flags_stale_group_membership() {
        let env = MockProbeEnvironment::default()
            .with_file("/etc/os-release", "ID=ubuntu\n")
            .with_command(
                "systemd-run",
                &[
                    "--user",
                    "--pipe",
                    "--wait",
                    "--quiet",
                    "--collect",
                    "docker",
                    "ps",
                ],
                Ok(CommandSnapshot {
                    success: false,
                    stdout: String::new(),
                    stderr: "permission denied while trying to connect to the Docker daemon socket"
                        .to_string(),
                }),
            );

        let result = probe_docker_user_systemd(&env);
        assert!(result.is_blocking_failure());
        assert!(result.summary.contains("stale group membership"));
        let remediation = result.remediation.expect("remediation");
        match remediation {
            Remediation::Manual { command, .. } => {
                let command = command.expect("command");
                assert!(command.contains("systemctl restart user@"));
            }
            Remediation::Suggested { .. } => panic!("expected manual remediation"),
        }
    }

    #[test]
    fn docker_user_systemd_skips_when_systemd_run_missing() {
        let env = MockProbeEnvironment::default().with_file("/etc/os-release", "ID=ubuntu\n");

        let result = probe_docker_user_systemd(&env);
        assert_eq!(result.status, ProbeStatus::Skip);
    }

    #[test]
    fn docker_user_systemd_skips_on_non_linux() {
        let env = MockProbeEnvironment::default();

        let result = probe_docker_user_systemd(&env);
        assert_eq!(result.status, ProbeStatus::Skip);
        assert!(result.summary.contains("only available on Linux"));
    }

    #[test]
    fn parse_os_release_id_handles_quoting_and_case() {
        assert_eq!(
            parse_os_release_id("NAME=Ubuntu\nID=ubuntu\nVERSION=\"22.04\"\n").as_deref(),
            Some("ubuntu")
        );
        assert_eq!(
            parse_os_release_id("ID=\"amzn\"\n").as_deref(),
            Some("amzn")
        );
        assert_eq!(
            parse_os_release_id("ID='fedora'\n").as_deref(),
            Some("fedora")
        );
        assert_eq!(
            parse_os_release_id("ID=Debian\n").as_deref(),
            Some("debian"),
            "ID values should be lowercased for matching"
        );
    }

    #[test]
    fn parse_os_release_id_returns_none_when_missing() {
        assert_eq!(parse_os_release_id(""), None);
        assert_eq!(parse_os_release_id("NAME=Foo\n"), None);
        assert_eq!(parse_os_release_id("ID=\n"), None);
    }

    #[test]
    fn docker_install_remediation_picks_dnf_for_rhel_family() {
        for id in ["amzn", "rhel", "fedora", "rocky", "almalinux", "centos"] {
            let (_, command) = docker_install_remediation(Some(id));
            assert!(command.contains("dnf install"), "{id} should use dnf");
        }
    }

    #[test]
    fn docker_install_remediation_picks_apt_for_debian_family() {
        for id in ["ubuntu", "debian"] {
            let (_, command) = docker_install_remediation(Some(id));
            assert!(
                command.contains("apt-get install"),
                "{id} should use apt-get"
            );
            assert!(
                command.contains("apt-get update"),
                "{id} should refresh apt cache before install"
            );
        }
    }

    #[test]
    fn docker_install_remediation_falls_back_to_docs_link() {
        let (summary, command) = docker_install_remediation(None);
        assert!(summary.contains("for your distribution"));
        assert!(command.contains("docs.docker.com/engine/install"));
    }

    #[test]
    fn memory_blocks_on_small_host_without_swap() {
        let env = MockProbeEnvironment::default().with_file(
            LINUX_MEMINFO_PATH,
            "MemTotal:        988000 kB\nSwapTotal:            0 kB\n",
        );

        let result = probe_memory(&env);
        assert!(result.is_blocking_failure());
        assert!(result.summary.contains("Insufficient memory"));
        assert!(result.summary.contains("no swap configured"));
    }

    #[test]
    fn memory_warns_on_low_effective_memory() {
        let env = MockProbeEnvironment::default().with_file(
            LINUX_MEMINFO_PATH,
            "MemTotal:       1024000 kB\nSwapTotal:        512000 kB\n",
        );

        let result = probe_memory(&env);
        assert!(result.is_warning());
        assert!(result.summary.contains("1000MB RAM + 500MB swap"));
    }

    #[test]
    fn linger_disabled_is_warning() {
        let env = MockProbeEnvironment::default()
            .with_username("ubuntu")
            .with_command(
                "loginctl",
                &["show-user", "ubuntu", "--property=Linger", "--value"],
                Ok(CommandSnapshot {
                    success: true,
                    stdout: "no\n".to_string(),
                    stderr: String::new(),
                }),
            );

        let result = probe_systemd_linger(&env);
        assert!(result.is_warning());
        assert!(result.summary.contains("disabled"));
    }

    #[test]
    fn bind_port_warns_when_occupied() {
        let env = MockProbeEnvironment::default()
            .with_bind_result("127.0.0.1:4096", Err("Address already in use".to_string()));

        let result = probe_bind_port("127.0.0.1:4096", false, &env);
        assert!(result.is_warning());
        assert!(result.summary.contains("127.0.0.1:4096"));
    }

    #[test]
    fn runner_skips_docker_access_when_docker_missing() {
        let env = MockProbeEnvironment::default().with_command(
            "docker",
            &["--version"],
            Err("No such file or directory".to_string()),
        );
        let config = test_config();
        let ctx = AutopilotProbeContext {
            app_config: &config,
            bind_addr: Some("127.0.0.1:4096"),
            server_reachable: false,
        };

        let results = run_autopilot_probes(ProbeMode::Startup, &ctx, &env);
        let docker_access = results
            .iter()
            .find(|result| result.id == "docker_accessible")
            .expect("docker_accessible result");
        assert_eq!(docker_access.status, ProbeStatus::Skip);
    }

    #[test]
    fn disk_space_warns_when_low() {
        let env = MockProbeEnvironment::default().with_command(
            "df",
            &["-Pk", "/tmp"],
            Ok(CommandSnapshot {
                success: true,
                stdout: "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk1 1000000 900000 100000 90% /tmp\n".to_string(),
                stderr: String::new(),
            }),
        );

        let result = probe_disk_space(Path::new("/tmp"), &env);
        assert!(result.is_warning());
        assert!(result.summary.contains("Low disk space"));
    }

    #[test]
    fn parse_meminfo_rejects_missing_fields() {
        let error = parse_meminfo("MemTotal: 1024 kB\n").expect_err("missing swap total");
        assert!(error.contains("SwapTotal"));
    }

    #[test]
    fn parse_df_available_mb_rejects_malformed_output() {
        let error = parse_df_available_mb("Filesystem\ninvalid\n").expect_err("malformed df");
        assert!(error.contains("Unexpected df output row") || error.contains("Invalid"));
    }

    #[test]
    fn truncate_chars_handles_zero_limit() {
        assert_eq!(truncate_chars("hello", 0), "...");
    }

    #[test]
    fn sandbox_mount_inputs_warn_when_critical_mount_is_unreadable() {
        let mut config = test_config();
        config.warden = Some(WardenConfig {
            enabled: true,
            volumes: vec![
                "/tmp/stakpak-config.toml:/home/agent/.stakpak/config.toml:ro".to_string(),
            ],
        });

        let env = MockProbeEnvironment::default().with_path_access(
            "/tmp/stakpak-config.toml",
            Err("Permission denied".to_string()),
        );
        let ctx = AutopilotProbeContext {
            app_config: &config,
            bind_addr: None,
            server_reachable: false,
        };

        let result = probe_sandbox_mount_inputs(&ctx, &env);
        assert!(result.is_warning());
        assert!(result.summary.contains("critical sandbox mount input"));
        let details = result.details.expect("details");
        assert!(details.contains("/tmp/stakpak-config.toml"));
        assert!(details.contains("Permission denied"));
    }
}
