use crate::config::AppConfig;
use crate::utils::plugins::{PluginConfig, get_plugin_path};
use clap::Subcommand;
// Re-export container constants so existing callers (autopilot.rs) don't need to change imports.
pub use stakpak_shared::container::{
    expand_volume_path, stakpak_agent_default_mounts, stakpak_agent_image,
};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;

#[derive(Subcommand, PartialEq)]
pub enum WardenCommands {
    /// Run any container image through Warden's network firewall (sidecar pattern)
    Wrap {
        /// Container image to run
        image: String,
        /// Environment variables to pass to container (-e KEY=VALUE)
        #[arg(short, long, action = clap::ArgAction::Append)]
        env: Vec<String>,
        /// Additional volumes to mount (-v /host:/container)
        #[arg(short, long, action = clap::ArgAction::Append)]
        volume: Vec<String>,
        /// Working directory inside the container
        #[arg(short, long)]
        workdir: Option<String>,
        /// Enable TTY allocation for interactive use
        #[arg(short, long)]
        tty: bool,
        /// Container runtime: docker or podman
        #[arg(short, long)]
        runtime: Option<String>,
        /// Command and arguments to run inside the container
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Display and analyze request logs with filtering options
    Logs {
        /// Show only blocked requests
        #[arg(short, long)]
        blocked_only: bool,
        /// Limit number of records to show
        #[arg(short, long)]
        limit: Option<u32>,
        /// Show detailed request/response data including headers and bodies
        #[arg(short, long)]
        detailed: bool,
    },
    /// Remove all stored request logs from the database
    ClearLogs,
    /// Display version information and project links
    Version,
}

impl WardenCommands {
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        // Get warden path (will download if not available)
        let warden_path = get_warden_plugin_path().await;

        let mut cmd = Command::new(warden_path);
        let mut needs_tty = false;

        match self {
            WardenCommands::Wrap {
                image,
                env,
                volume,
                workdir,
                tty,
                runtime,
                command,
            } => {
                cmd.arg("wrap");

                // Image is positional first argument
                cmd.arg(&image);

                for env_var in env {
                    cmd.args(["--env", &env_var]);
                }

                // Pre-create named volumes to prevent race conditions
                stakpak_shared::container::ensure_named_volumes_exist();

                // Prepare volumes from config first, then add user-specified volumes
                // User volumes come last to allow overrides
                for vol in prepare_volumes(&config, false) {
                    let expanded_vol = expand_volume_path(&vol);
                    cmd.args(["--volume", &expanded_vol]);
                }

                for vol in volume {
                    cmd.args(["--volume", &vol]);
                }

                if let Some(workdir) = workdir {
                    cmd.args(["--workdir", &workdir]);
                }

                if tty {
                    cmd.arg("--tty");
                    needs_tty = true;
                }

                if let Some(runtime) = runtime {
                    cmd.args(["--runtime", &runtime]);
                }

                // Command comes after -- separator
                if !command.is_empty() {
                    cmd.arg("--");
                    cmd.args(&command);
                }
            }
            WardenCommands::Logs {
                blocked_only,
                limit,
                detailed,
            } => {
                cmd.arg("logs");

                if blocked_only {
                    cmd.arg("--blocked-only");
                }

                if let Some(limit) = limit {
                    cmd.args(["--limit", &limit.to_string()]);
                }

                if detailed {
                    cmd.arg("--detailed");
                }
            }
            WardenCommands::ClearLogs => {
                cmd.arg("clear-logs");
            }
            WardenCommands::Version => {
                cmd.arg("version");
            }
        }

        // Execute the warden command with proper TTY handling
        execute_warden_command(cmd, needs_tty)
    }
}

pub async fn get_warden_plugin_path() -> String {
    let warden_config = PluginConfig {
        name: "warden".to_string(),
        base_url: "https://warden-cli-releases.s3.amazonaws.com/".to_string(),
        targets: vec![
            "linux-x86_64".to_string(),
            "darwin-x86_64".to_string(),
            "darwin-aarch64".to_string(),
            "windows-x86_64".to_string(),
        ],
        version: None,
    };

    get_plugin_path(warden_config).await
}

/// Helper function to prepare volumes for warden container.
///
/// Collects volumes from the profile config, then ensures every entry from
/// [`stakpak_agent_default_mounts`] is present (deduped by container-side path).
/// If `check_enabled` is true, profile volumes are only included when
/// `warden.enabled` is true.
pub fn prepare_volumes(config: &AppConfig, check_enabled: bool) -> Vec<String> {
    let mut volumes_to_mount = Vec::new();

    // Add volumes from profile config
    if let Some(warden_config) = config.warden.as_ref()
        && (!check_enabled || warden_config.enabled)
    {
        volumes_to_mount.extend(warden_config.volumes.clone());
    }

    // Append every default mount that isn't already covered by the profile.
    // Dedup by the container-side path (the part after the first `:`).
    for default_vol in stakpak_agent_default_mounts() {
        let container_path = default_vol.split(':').nth(1).unwrap_or(&default_vol);
        let already_mounted = volumes_to_mount.iter().any(|v| v.contains(container_path));
        if !already_mounted {
            volumes_to_mount.push(default_vol);
        }
    }

    volumes_to_mount
}

/// Execute warden command with proper TTY handling and streaming
fn execute_warden_command(mut cmd: Command, needs_tty: bool) -> Result<(), String> {
    if needs_tty {
        // For TTY mode, use spawn with inherited stdio for proper interactive handling
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn warden process: {}", e))?;

        let status = child
            .wait()
            .map_err(|e| format!("Failed to wait for warden process: {}", e))?;

        if !status.success() {
            return Err(format!(
                "warden command failed with exit code: {:?}",
                status.code()
            ));
        }
    } else {
        // For non-TTY mode, pipe stdout and stderr and stream them to user
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn warden process: {}", e))?;

        // Handle stdout streaming
        let stdout_handle = if let Some(stdout) = child.stdout.take() {
            let stdout_reader = BufReader::new(stdout);
            Some(thread::spawn(move || {
                for line in stdout_reader.lines() {
                    match line {
                        Ok(line) => println!("{}", line),
                        Err(_) => break,
                    }
                }
            }))
        } else {
            None
        };

        // Handle stderr streaming
        let stderr_handle = if let Some(stderr) = child.stderr.take() {
            let stderr_reader = BufReader::new(stderr);
            Some(thread::spawn(move || {
                for line in stderr_reader.lines() {
                    match line {
                        Ok(line) => eprintln!("{}", line),
                        Err(_) => break,
                    }
                }
            }))
        } else {
            None
        };

        // Wait for the process to complete
        let status = child
            .wait()
            .map_err(|e| format!("Failed to wait for warden process: {}", e))?;

        // Wait for streaming threads to complete
        if let Some(handle) = stdout_handle {
            let _ = handle.join();
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.join();
        }

        if !status.success() {
            return Err(format!(
                "warden command failed with exit code: {:?}",
                status.code()
            ));
        }
    }

    Ok(())
}

/// Run warden with preconfigured setup (convenience command)
pub async fn run_default_warden(
    config: AppConfig,
    extra_volumes: Vec<String>,
    extra_env: Vec<String>,
) -> Result<(), String> {
    // Get warden path (will download if not available)
    let warden_path = get_warden_plugin_path().await;

    // Run warden wrap with default configuration
    let mut cmd = Command::new(warden_path);
    cmd.arg("wrap");

    // Use standard stakpak image with current CLI version (no special warden image needed)
    let stakpak_image = stakpak_agent_image();
    cmd.arg(&stakpak_image);

    // Enable TTY by default for convenience command
    cmd.arg("--tty");

    // Prepare and mount volumes
    for volume in prepare_volumes(&config, true) {
        let expanded_volume = expand_volume_path(&volume);
        cmd.args(["--volume", &expanded_volume]);
    }

    // Add extra environment variables
    for env_var in extra_env {
        cmd.args(["--env", &env_var]);
    }

    // Add extra volume mounts (these override/extend profile volumes)
    for volume in extra_volumes {
        cmd.args(["--volume", &volume]);
    }

    // Command comes after -- separator
    cmd.args(["--", "stakpak"]);

    // Execute the warden command with TTY support
    execute_warden_command(cmd, true)
}

/// Re-execute the stakpak command inside warden container
pub async fn run_stakpak_in_warden(config: AppConfig, args: &[String]) -> Result<(), String> {
    // Get warden path (will download if not available)
    let warden_path = get_warden_plugin_path().await;

    // Build warden wrap command
    let mut cmd = Command::new(warden_path);
    cmd.arg("wrap");

    // Use standard stakpak image with current CLI version (no special warden image needed)
    let stakpak_image = stakpak_agent_image();
    cmd.arg(&stakpak_image);

    // Determine if we need TTY (interactive mode) based on CLI args.
    // For async/single-step modes (-a/--async or -p/--print), we avoid TTY so warden
    // can run in non-interactive batch mode and exit cleanly.
    let needs_tty = !args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-a" | "--async" | "-p" | "--print"));

    // Enable TTY only when we are in fully interactive mode
    if needs_tty {
        cmd.arg("--tty");
    }

    // Prepare and mount volumes (don't check enabled flag for this function)
    for volume in prepare_volumes(&config, false) {
        let expanded_volume = expand_volume_path(&volume);
        cmd.args(["--volume", &expanded_volume]);
    }

    // Set environment variable to prevent infinite recursion
    cmd.args(["--env", "STAKPAK_SKIP_WARDEN=1"]);

    // Pass the profile through from config (skip only when empty to avoid broken command)
    if !config.profile_name.is_empty() {
        cmd.args(["--env", &format!("STAKPAK_PROFILE={}", config.profile_name)]);
    }

    // Pass through API key if set
    if let Ok(api_key) = std::env::var("STAKPAK_API_KEY") {
        cmd.args(["--env", &format!("STAKPAK_API_KEY={}", api_key)]);
    }

    // Pass through API endpoint if set
    if let Ok(api_endpoint) = std::env::var("STAKPAK_API_ENDPOINT") {
        cmd.args(["--env", &format!("STAKPAK_API_ENDPOINT={}", api_endpoint)]);
    }

    // Build the stakpak command arguments to run inside container
    // Skip the first arg (program name) and pass the rest as separate arguments after --
    cmd.arg("--");
    cmd.arg("stakpak");
    for arg in args.iter().skip(1) {
        cmd.arg(arg);
    }

    // Execute the warden command with appropriate TTY handling
    execute_warden_command(cmd, needs_tty)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderType, WardenConfig};
    use std::collections::HashMap;

    /// Minimal AppConfig for testing prepare_volumes.
    fn test_config(warden: Option<WardenConfig>) -> AppConfig {
        AppConfig {
            api_endpoint: "https://test".into(),
            api_key: None,
            mcp_server_host: None,
            machine_name: None,
            auto_append_gitignore: None,
            profile_name: "test".into(),
            config_path: "/tmp/test".into(),
            allowed_tools: None,
            auto_approve: None,
            rulebooks: None,
            warden,
            provider: ProviderType::Remote,
            providers: HashMap::new(),
            smart_model: None,
            eco_model: None,
            recovery_model: None,
            model: None,
            anonymous_id: None,
            collect_telemetry: None,
            editor: None,
        }
    }

    fn has_aqua_cache(volumes: &[String]) -> bool {
        volumes.iter().any(|v| v.contains("aquaproj-aqua"))
    }

    // ── Interactive / async mode (run_stakpak_in_warden path) ──────────
    // Uses prepare_volumes(config, false) — warden enabled flag is ignored.

    #[test]
    fn aqua_cache_present_when_warden_enabled_check_disabled() {
        let config = test_config(Some(WardenConfig {
            enabled: true,
            volumes: vec!["/tmp:/tmp:ro".into()],
        }));
        let vols = prepare_volumes(&config, false);
        assert!(has_aqua_cache(&vols), "aqua cache missing: {vols:?}");
    }

    #[test]
    fn aqua_cache_present_when_warden_disabled_check_disabled() {
        let config = test_config(Some(WardenConfig {
            enabled: false,
            volumes: vec!["/tmp:/tmp:ro".into()],
        }));
        let vols = prepare_volumes(&config, false);
        assert!(has_aqua_cache(&vols), "aqua cache missing: {vols:?}");
    }

    #[test]
    fn aqua_cache_present_when_no_warden_config() {
        let config = test_config(None);
        let vols = prepare_volumes(&config, false);
        assert!(has_aqua_cache(&vols), "aqua cache missing: {vols:?}");
    }

    // ── Default warden command (run_default_warden path) ───────────────
    // Uses prepare_volumes(config, true) — only includes profile volumes
    // when warden.enabled is true.

    #[test]
    fn aqua_cache_present_when_warden_enabled_check_enabled() {
        let config = test_config(Some(WardenConfig {
            enabled: true,
            volumes: vec!["./:/agent:ro".into()],
        }));
        let vols = prepare_volumes(&config, true);
        assert!(has_aqua_cache(&vols), "aqua cache missing: {vols:?}");
    }

    #[test]
    fn aqua_cache_present_when_warden_disabled_check_enabled() {
        // check_enabled=true + enabled=false → profile volumes skipped,
        // but aqua cache must still be present.
        let config = test_config(Some(WardenConfig {
            enabled: false,
            volumes: vec!["./:/agent:ro".into()],
        }));
        let vols = prepare_volumes(&config, true);
        assert!(has_aqua_cache(&vols), "aqua cache missing: {vols:?}");
    }

    // ── Agent server / subagent sandbox path ───────────────────────────
    // Autopilot calls prepare_volumes(config, false) and passes the result
    // into SandboxConfig.volumes. Same as interactive mode.

    #[test]
    fn aqua_cache_present_for_agent_server_sandbox() {
        let config = test_config(Some(WardenConfig {
            enabled: true,
            volumes: WardenConfig::readonly_profile().volumes,
        }));
        let vols = prepare_volumes(&config, false);
        assert!(has_aqua_cache(&vols), "aqua cache missing: {vols:?}");
    }

    // ── Dedup: user already has a custom aqua mount ────────────────────

    #[test]
    fn aqua_cache_not_duplicated_when_user_provides_custom_mount() {
        let custom = "/my/aqua:/home/agent/.local/share/aquaproj-aqua".to_string();
        let config = test_config(Some(WardenConfig {
            enabled: true,
            volumes: vec![custom.clone()],
        }));
        let vols = prepare_volumes(&config, false);
        let aqua_count = vols.iter().filter(|v| v.contains("aquaproj-aqua")).count();
        assert_eq!(
            aqua_count, 1,
            "should keep user mount, not add a second: {vols:?}"
        );
        assert!(vols.contains(&custom), "user mount should be preserved");
    }

    // ── expand_volume_path ─────────────────────────────────────────────

    #[test]
    fn expand_volume_path_leaves_named_volumes_unchanged() {
        let named = "stakpak-aqua-cache:/home/agent/.local/share/aquaproj-aqua";
        assert_eq!(expand_volume_path(named), named);
    }

    #[test]
    fn expand_volume_path_expands_tilde() {
        if let Ok(home) = std::env::var("HOME") {
            let expanded = expand_volume_path("~/data:/data:ro");
            assert!(
                expanded.starts_with(&home),
                "tilde not expanded: {expanded}"
            );
            assert!(
                !expanded.starts_with('~'),
                "tilde still present: {expanded}"
            );
        }
    }
}
