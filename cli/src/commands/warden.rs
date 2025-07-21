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
    _config: AppConfig,
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

    // TODO: enable mTLS to work with Warden
    // Disable mTLS for the MCP server/client when running with warden
    cmd.arg("--disable-mcp-mtls");

    // Mount ~/.stakpak/config.toml if it exists as read-only volume
    if let Ok(home_dir) = std::env::var("HOME") {
        let config_path = Path::new(&home_dir).join(".stakpak").join("config.toml");
        if config_path.exists() {
            let config_path_str = config_path.to_string_lossy();
            let volume_mount = format!("{}:/home/agent/.stakpak/config.toml:ro", config_path_str);
            cmd.args(["--volume", &volume_mount]);
        }
    }

    // Add extra environment variables
    for env_var in extra_env {
        cmd.args(["--env", &env_var]);
    }

    // Add extra volume mounts
    for volume in extra_volumes {
        cmd.args(["--volume", &volume]);
    }

    // Execute the warden command with TTY support
    execute_warden_command(cmd, true)
}
