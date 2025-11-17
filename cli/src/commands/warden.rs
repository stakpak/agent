use crate::config::AppConfig;
use crate::utils::plugins::{PluginConfig, get_plugin_path};
use clap::Subcommand;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

#[derive(Subcommand, PartialEq)]
pub enum WardenCommands {
    /// Run coding agent in a container and apply security policies
    Run {
        /// Container image to run
        #[arg(short, long)]
        image: Option<String>,
        /// Environment variables to pass to container
        #[arg(short, long, action = clap::ArgAction::Append)]
        env: Vec<String>,
        /// Additional volumes to mount
        #[arg(short, long, action = clap::ArgAction::Append)]
        volume: Vec<String>,
        /// Enable TTY allocation for interactive terminal applications
        #[arg(short, long)]
        tty: bool,
        /// Container runtime: docker or podman
        #[arg(short, long)]
        runtime: Option<String>,
        /// Command to run inside the container
        #[arg()]
        command: Option<String>,
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
    pub async fn run(self, _config: AppConfig) -> Result<(), String> {
        // Get warden path (will download if not available)
        let warden_path = get_warden_plugin_path().await;

        let mut cmd = Command::new(warden_path);
        let mut needs_tty = false;

        match self {
            WardenCommands::Run {
                image,
                env,
                volume,
                tty,
                runtime,
                command,
            } => {
                cmd.arg("run");

                if let Some(image) = image {
                    cmd.args(["--image", &image]);
                }

                for env_var in env {
                    cmd.args(["--env", &env_var]);
                }

                for vol in volume {
                    cmd.args(["--volume", &vol]);
                }

                if tty {
                    cmd.arg("--tty");
                    needs_tty = true;
                }

                if let Some(runtime) = runtime {
                    cmd.args(["--runtime", &runtime]);
                }

                if let Some(command) = command {
                    cmd.arg(command);
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

    // Always append stakpak config if it exists and not already in the list
    if let Ok(home_dir) = std::env::var("HOME") {
        let config_path = Path::new(&home_dir).join(".stakpak").join("config.toml");
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

/// Helper function to escape an argument for shell usage
/// Wraps arguments containing spaces or special characters in quotes
fn shell_escape_arg(arg: &str) -> String {
    // If the argument contains spaces, quotes, or other special characters, quote it
    if arg.contains(' ')
        || arg.contains('\'')
        || arg.contains('"')
        || arg.contains('$')
        || arg.contains('\\')
    {
        // Escape any existing quotes and wrap in double quotes
        let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        arg.to_string()
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

    // Run warden with default configuration
    let mut cmd = Command::new(warden_path);
    cmd.arg("run");

    // Use stakpak image with current CLI version
    let stakpak_image = format!(
        "ghcr.io/stakpak/agent-warden:v{}",
        env!("CARGO_PKG_VERSION")
    );
    cmd.args(["--image", &stakpak_image]);

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

    // Execute the warden command with TTY support
    execute_warden_command(cmd, true)
}

/// Re-execute the stakpak command inside warden container
pub async fn run_stakpak_in_warden(config: AppConfig, args: &[String]) -> Result<(), String> {
    // Get warden path (will download if not available)
    let warden_path = get_warden_plugin_path().await;

    // Build warden command
    let mut cmd = Command::new(warden_path);
    cmd.arg("run");

    // Use stakpak image with current CLI version
    let stakpak_image = format!(
        "ghcr.io/stakpak/agent-warden:v{}",
        env!("CARGO_PKG_VERSION")
    );
    cmd.args(["--image", &stakpak_image]);

    // Enable TTY for interactive mode
    cmd.arg("--tty");

    // Prepare and mount volumes (don't check enabled flag for this function)
    for volume in prepare_volumes(&config, false) {
        let expanded_volume = expand_volume_path(volume);
        cmd.args(["--volume", &expanded_volume]);
    }

    // Build the stakpak command to run inside container
    // We need to reconstruct the original command but add STAKPAK_SKIP_WARDEN to prevent recursion
    let mut stakpak_args = vec!["stakpak".to_string()];

    // Skip the first arg (program name) and add the rest
    for arg in args.iter().skip(1) {
        stakpak_args.push(arg.clone());
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

    // Join all stakpak arguments into a single command string, properly escaping arguments
    let stakpak_cmd = stakpak_args
        .iter()
        .map(|arg| shell_escape_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    cmd.arg(stakpak_cmd);

    // Execute the warden command with TTY support
    execute_warden_command(cmd, true)
}
