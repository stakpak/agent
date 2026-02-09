use crate::config::AppConfig;
use crate::utils::plugins::{PluginConfig, get_plugin_path};
use clap::Subcommand;
use std::io::{BufRead, BufReader};
use std::path::Path;
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

                // Prepare volumes from config first, then add user-specified volumes
                // User volumes come last to allow overrides
                for vol in prepare_volumes(&config, false) {
                    let expanded_vol = expand_volume_path(vol);
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

async fn get_warden_plugin_path() -> String {
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

/// Helper function to prepare volumes for warden container
/// Collects volumes from config and always appends stakpak config if it exists and isn't already mounted
/// If check_enabled is true, only adds volumes when warden is enabled in config
fn prepare_volumes(config: &AppConfig, check_enabled: bool) -> Vec<String> {
    let mut volumes_to_mount = Vec::new();

    // Add volumes from profile config
    if let Some(warden_config) = config.warden.as_ref()
        && (!check_enabled || warden_config.enabled)
    {
        volumes_to_mount.extend(warden_config.volumes.clone());
    }

    // Always append stakpak config and auth files if they exist and not already in the list
    if let Ok(home_dir) = std::env::var("HOME") {
        let stakpak_dir = Path::new(&home_dir).join(".stakpak");

        // Mount config.toml if it exists
        let config_path = stakpak_dir.join("config.toml");
        if config_path.exists() {
            let config_path_str = config_path.to_string_lossy();
            let stakpak_config_mount =
                format!("{}:/home/agent/.stakpak/config.toml:ro", config_path_str);

            // Check if stakpak config is already in the volume list
            let config_already_mounted = volumes_to_mount.iter().any(|v| {
                v.contains("/.stakpak/config.toml")
                    || v.ends_with(":/home/agent/.stakpak/config.toml:ro")
                    || v.ends_with(":/home/agent/.stakpak/config.toml")
            });

            if !config_already_mounted {
                volumes_to_mount.push(stakpak_config_mount);
            }
        }

        // Mount auth.toml if it exists (contains provider credentials)
        let auth_path = stakpak_dir.join("auth.toml");
        if auth_path.exists() {
            let auth_path_str = auth_path.to_string_lossy();
            let stakpak_auth_mount = format!("{}:/home/agent/.stakpak/auth.toml:ro", auth_path_str);

            // Check if auth.toml is already in the volume list
            let auth_already_mounted = volumes_to_mount.iter().any(|v| {
                v.contains("/.stakpak/auth.toml")
                    || v.ends_with(":/home/agent/.stakpak/auth.toml:ro")
                    || v.ends_with(":/home/agent/.stakpak/auth.toml")
            });

            if !auth_already_mounted {
                volumes_to_mount.push(stakpak_auth_mount);
            }
        }
    }

    volumes_to_mount
}

/// Helper function to expand tilde (~) in volume paths to home directory
fn expand_volume_path(volume: String) -> String {
    if volume.starts_with("~/") || volume.starts_with("~:") {
        if let Ok(home_dir) = std::env::var("HOME") {
            volume.replacen("~", &home_dir, 1)
        } else {
            volume
        }
    } else {
        volume
    }
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
    let stakpak_image = format!("ghcr.io/stakpak/agent:v{}", env!("CARGO_PKG_VERSION"));
    cmd.arg(&stakpak_image);

    // Enable TTY by default for convenience command
    cmd.arg("--tty");

    // Prepare and mount volumes
    for volume in prepare_volumes(&config, true) {
        let expanded_volume = expand_volume_path(volume);
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
    let stakpak_image = format!("ghcr.io/stakpak/agent:v{}", env!("CARGO_PKG_VERSION"));
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
        let expanded_volume = expand_volume_path(volume);
        cmd.args(["--volume", &expanded_volume]);
    }

    // Set environment variable to prevent infinite recursion
    cmd.args(["--env", "STAKPAK_SKIP_WARDEN=1"]);

    // If profile was specified, pass it through
    if let Ok(profile) = std::env::var("STAKPAK_PROFILE") {
        cmd.args(["--env", &format!("STAKPAK_PROFILE={}", profile)]);
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
