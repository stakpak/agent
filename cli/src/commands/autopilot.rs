use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{
    config::AppConfig,
    onboarding::{OnboardingMode, run_onboarding},
};

#[derive(Args, PartialEq, Debug, Clone)]
pub struct SetupArgs {
    /// Overwrite existing generated files where applicable
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// Do not prompt; require env vars for setup
    #[arg(long, default_value_t = false)]
    pub non_interactive: bool,

    /// Assume yes for optional setup steps
    #[arg(long, default_value_t = false)]
    pub yes: bool,

    /// Skip installing OS service (systemd/launchd)
    #[arg(long, default_value_t = false)]
    pub skip_service_install: bool,

    /// Bind address for embedded server runtime (saved for autopilot starts)
    #[arg(long)]
    pub bind: Option<String>,

    /// Show generated auth token in stdout (saved for autopilot starts)
    #[arg(long, default_value_t = false)]
    pub show_token: bool,

    /// Disable auth checks for protected routes (saved for autopilot starts)
    #[arg(long, default_value_t = false)]
    pub no_auth: bool,

    /// Override default model for server runs (saved for autopilot starts)
    #[arg(long)]
    pub model: Option<String>,

    /// Auto-approve all tools (saved for autopilot starts)
    #[arg(long, default_value_t = false)]
    pub auto_approve_all: bool,

    /// Disable gateway runtime by default for autopilot starts
    #[arg(long, default_value_t = false)]
    pub no_gateway: bool,

    /// Disable watch scheduler by default for autopilot starts
    #[arg(long, default_value_t = false)]
    pub no_watch: bool,

    /// Path to gateway config file (saved for autopilot starts)
    #[arg(long)]
    pub gateway_config: Option<PathBuf>,

    /// Emit machine-readable JSON output
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Args, PartialEq, Debug, Clone)]
pub struct StartArgs {
    /// Bind address for embedded server runtime
    #[arg(long, default_value = "127.0.0.1:4096")]
    pub bind: String,

    /// Show generated auth token in stdout (local dev only)
    #[arg(long, default_value_t = false)]
    pub show_token: bool,

    /// Disable auth checks for protected routes (local dev only)
    #[arg(long, default_value_t = false)]
    pub no_auth: bool,

    /// Override default model for server runs (provider/model or model id)
    #[arg(long)]
    pub model: Option<String>,

    /// Auto-approve all tools (CI/headless only)
    #[arg(long, default_value_t = false)]
    pub auto_approve_all: bool,

    /// Don't start gateway runtime
    #[arg(long, default_value_t = false)]
    pub no_gateway: bool,

    /// Don't start watch scheduler
    #[arg(long, default_value_t = false)]
    pub no_watch: bool,

    /// Path to gateway config file
    #[arg(long)]
    pub gateway_config: Option<PathBuf>,

    /// Emit machine-readable JSON output
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Run in foreground instead of delegating to OS service
    #[arg(long, default_value_t = false)]
    pub foreground: bool,
}

#[derive(Args, PartialEq, Debug, Clone)]
pub struct StopArgs {
    /// Also remove installed OS service definition
    #[arg(long, default_value_t = false)]
    pub uninstall: bool,
}

#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum AutopilotCommands {
    /// One-time guided setup for autonomous 24/7 mode
    Setup {
        #[command(flatten)]
        args: SetupArgs,
    },

    /// Start autonomous 24/7 mode
    Start {
        #[command(flatten)]
        args: StartArgs,

        /// Internal flag used by service units to avoid recursive delegation
        #[arg(long, hide = true, default_value_t = false)]
        from_service: bool,
    },

    /// Stop autonomous 24/7 mode
    Stop {
        #[command(flatten)]
        args: StopArgs,
    },

    /// Show autonomous runtime status (service + server + gateway + watch)
    Status {
        /// Emit machine-readable JSON output
        #[arg(long, default_value_t = false)]
        json: bool,

        /// Include recent watch runs (count)
        #[arg(long)]
        watch_runs: Option<u32>,
    },

    /// Show/tail autonomous runtime logs
    Logs {
        /// Follow log output
        #[arg(short = 'f', long, default_value_t = true)]
        follow: bool,

        /// Number of lines to show initially
        #[arg(short = 'n', long)]
        lines: Option<u32>,
    },

    /// Run preflight checks for autopilot setup/runtime
    Doctor,
}

impl AutopilotCommands {
    pub async fn run(self, mut config: AppConfig) -> Result<(), String> {
        match self {
            AutopilotCommands::Setup { args } => {
                setup_autopilot(
                    &mut config,
                    SetupOptions {
                        force: args.force,
                        non_interactive: args.non_interactive,
                        yes: args.yes,
                        skip_service_install: args.skip_service_install,
                        bind: args.bind,
                        show_token: args.show_token,
                        no_auth: args.no_auth,
                        model: args.model,
                        auto_approve_all: args.auto_approve_all,
                        no_gateway: args.no_gateway,
                        no_watch: args.no_watch,
                        gateway_config: args.gateway_config,
                    },
                    OutputMode::from_json_flag(args.json),
                )
                .await
            }
            AutopilotCommands::Start { args, from_service } => {
                start_autopilot(
                    &config,
                    StartOptions {
                        bind: args.bind,
                        show_token: args.show_token,
                        no_auth: args.no_auth,
                        model: args.model,
                        auto_approve_all: args.auto_approve_all,
                        no_gateway: args.no_gateway,
                        no_watch: args.no_watch,
                        gateway_config: args.gateway_config,
                        foreground: args.foreground,
                        from_service,
                    },
                    OutputMode::from_json_flag(args.json),
                )
                .await
            }
            AutopilotCommands::Stop { args } => stop_autopilot(args.uninstall).await,
            AutopilotCommands::Status { json, watch_runs } => {
                status_autopilot(&config, OutputMode::from_json_flag(json), watch_runs).await
            }
            AutopilotCommands::Logs { follow, lines } => logs_autopilot(follow, lines).await,
            AutopilotCommands::Doctor => doctor_autopilot(&config).await,
        }
    }
}

#[derive(Debug, Clone)]
struct SetupOptions {
    force: bool,
    non_interactive: bool,
    yes: bool,
    skip_service_install: bool,
    bind: Option<String>,
    show_token: bool,
    no_auth: bool,
    model: Option<String>,
    auto_approve_all: bool,
    no_gateway: bool,
    no_watch: bool,
    gateway_config: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct StartOptions {
    bind: String,
    show_token: bool,
    no_auth: bool,
    model: Option<String>,
    auto_approve_all: bool,
    no_gateway: bool,
    no_watch: bool,
    gateway_config: Option<PathBuf>,
    foreground: bool,
    from_service: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Text,
    Json,
}

impl OutputMode {
    fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Text }
    }

    fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutopilotRuntimeConfig {
    bind: String,
    show_token: bool,
    no_auth: bool,
    model: Option<String>,
    auto_approve_all: bool,
    no_gateway: bool,
    no_watch: bool,
    gateway_config: Option<PathBuf>,
}

impl Default for AutopilotRuntimeConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:4096".to_string(),
            show_token: false,
            no_auth: false,
            model: None,
            auto_approve_all: false,
            no_gateway: false,
            no_watch: false,
            gateway_config: None,
        }
    }
}

impl AutopilotRuntimeConfig {
    fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".stakpak")
            .join("autopilot.toml")
    }

    fn load_or_default() -> Result<Self, String> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }

        Self::load_from_path(&path)
    }

    fn save(&self) -> Result<PathBuf, String> {
        let path = Self::path();
        self.save_to_path(&path)?;
        Ok(path)
    }

    fn from_setup_options(options: &SetupOptions) -> Self {
        Self {
            bind: options
                .bind
                .clone()
                .unwrap_or_else(|| "127.0.0.1:4096".to_string()),
            show_token: options.show_token,
            no_auth: options.no_auth,
            model: options.model.clone(),
            auto_approve_all: options.auto_approve_all,
            no_gateway: options.no_gateway,
            no_watch: options.no_watch,
            gateway_config: options.gateway_config.clone(),
        }
    }

    fn from_start_options(options: &StartOptions) -> Self {
        Self {
            bind: options.bind.clone(),
            show_token: options.show_token,
            no_auth: options.no_auth,
            model: options.model.clone(),
            auto_approve_all: options.auto_approve_all,
            no_gateway: options.no_gateway,
            no_watch: options.no_watch,
            gateway_config: options.gateway_config.clone(),
        }
    }

    fn load_from_path(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read autopilot config {}: {}", path.display(), e))?;

        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse autopilot config {}: {}", path.display(), e))
    }

    fn save_to_path(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create autopilot config dir: {}", e))?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize autopilot config: {}", e))?;

        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write autopilot config {}: {}", path.display(), e))
    }
}

impl StartOptions {
    fn with_runtime_config(mut self, runtime: &AutopilotRuntimeConfig) -> Self {
        self.bind = runtime.bind.clone();
        self.show_token = runtime.show_token;
        self.no_auth = runtime.no_auth;
        self.model = runtime.model.clone();
        self.auto_approve_all = runtime.auto_approve_all;
        self.no_gateway = runtime.no_gateway;
        self.no_watch = runtime.no_watch;
        self.gateway_config = runtime.gateway_config.clone();
        self
    }

    fn has_runtime_overrides(&self) -> bool {
        self.bind != "127.0.0.1:4096"
            || self.show_token
            || self.no_auth
            || self.model.is_some()
            || self.auto_approve_all
            || self.no_gateway
            || self.no_watch
            || self.gateway_config.is_some()
    }
}

#[derive(Debug, Serialize)]
struct StartJsonResult {
    command: &'static str,
    ok: bool,
    started_via: &'static str,
    profile: String,
    runtime_config_path: String,
    runtime_config_updated: bool,
    service_installed_now: bool,
    effective: AutopilotRuntimeConfig,
}

#[derive(Debug, Serialize)]
struct SetupJsonResult {
    command: &'static str,
    ok: bool,
    profile: String,
    runtime_config_path: String,
    runtime: AutopilotRuntimeConfig,
    service_installed: bool,
    gateway_config_path: String,
    watch_config_path: String,
}

#[derive(Debug, Serialize)]
struct AutopilotStatusJson {
    command: &'static str,
    ok: bool,
    profile: String,
    runtime: AutopilotRuntimeConfig,
    runtime_config_path: String,
    service: ServiceStatusJson,
    server: EndpointStatusJson,
    gateway: EndpointStatusJson,
    watch: WatchStatusJson,
}

#[derive(Debug, Serialize)]
struct ServiceStatusJson {
    installed: bool,
    active: bool,
    path: String,
}

#[derive(Debug, Serialize)]
struct EndpointStatusJson {
    expected_enabled: bool,
    reachable: bool,
    url: String,
}

#[derive(Debug, Serialize)]
struct WatchStatusJson {
    expected_enabled: bool,
    config_path: String,
    config_valid: bool,
    trigger_count: usize,
    running: bool,
    pid: Option<i64>,
    stale_pid: bool,
    db_path: Option<String>,
    error: Option<String>,
    recent_runs: Vec<WatchRunSummaryJson>,
}

#[derive(Debug, Serialize)]
struct WatchRunSummaryJson {
    id: i64,
    trigger_name: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
    error_message: Option<String>,
}

async fn setup_autopilot(
    config: &mut AppConfig,
    options: SetupOptions,
    output_mode: OutputMode,
) -> Result<(), String> {
    if output_mode.is_json() && !options.non_interactive {
        return Err("--json output for setup requires --non-interactive mode".to_string());
    }

    if !output_mode.is_json() {
        println!("Stakpak Autopilot setup");
        println!("Profile: {}", config.profile_name);
        println!();
    }

    let has_stakpak_key = config.get_stakpak_api_key().is_some();
    let has_provider_keys = !config.get_llm_provider_config().providers.is_empty();

    if !has_stakpak_key && !has_provider_keys {
        if options.non_interactive {
            return Err(
                "No provider credentials configured. Run with credentials in env or run interactive setup without --non-interactive.".to_string(),
            );
        }

        if !output_mode.is_json() {
            println!("No credentials found. Launching onboarding...");
        }
        run_onboarding(config, OnboardingMode::Default).await;
        if !output_mode.is_json() {
            println!();
        }
    }

    if !options.no_gateway {
        ensure_gateway_setup(config, options.force, options.non_interactive, output_mode).await?;
    } else if !output_mode.is_json() {
        println!("✓ Skipping gateway setup (--no-gateway)");
    }

    if !options.no_watch {
        ensure_watch_setup(options.force, output_mode).await?;
    } else if !output_mode.is_json() {
        println!("✓ Skipping watch setup (--no-watch)");
    }

    let runtime_config = AutopilotRuntimeConfig::from_setup_options(&options);
    let runtime_config_path = runtime_config.save()?;

    if !options.skip_service_install {
        install_autopilot_service(config)?;
        if !output_mode.is_json() {
            println!("✓ Autopilot service installed");
        }
    } else if !output_mode.is_json() {
        println!("✓ Skipped service installation (--skip-service-install)");
    }

    if output_mode.is_json() {
        if !options.yes {
            print_json(&SetupJsonResult {
                command: "autopilot.setup",
                ok: true,
                profile: config.profile_name.clone(),
                runtime_config_path: runtime_config_path.display().to_string(),
                runtime: runtime_config.clone(),
                service_installed: !options.skip_service_install,
                gateway_config_path: stakpak_gateway::config::default_gateway_config_path()
                    .display()
                    .to_string(),
                watch_config_path: default_watch_config_path().display().to_string(),
            })?;
        }
    } else {
        println!(
            "✓ Autopilot runtime defaults saved: {}",
            runtime_config_path.display()
        );
        println!();
        if options.yes {
            println!("Setup complete. Starting autopilot...");
        } else {
            println!("Next steps:");
            println!("  stakpak up         # alias for 'stakpak autopilot start'");
            println!("  stakpak down       # alias for 'stakpak autopilot stop'");
            println!("  stakpak autopilot status");
        }
    }

    if options.yes {
        start_autopilot(
            config,
            StartOptions {
                bind: "127.0.0.1:4096".to_string(),
                show_token: false,
                no_auth: false,
                model: None,
                auto_approve_all: false,
                no_gateway: false,
                no_watch: false,
                gateway_config: None,
                foreground: false,
                from_service: false,
            },
            output_mode,
        )
        .await?;
    }

    Ok(())
}

async fn ensure_gateway_setup(
    config: &AppConfig,
    force: bool,
    non_interactive: bool,
    output_mode: OutputMode,
) -> Result<(), String> {
    let gateway_path = stakpak_gateway::config::default_gateway_config_path();

    if gateway_path.exists() && !force {
        let loaded = stakpak_gateway::GatewayConfig::load(
            Some(gateway_path.as_path()),
            &stakpak_gateway::GatewayCliFlags::default(),
        )
        .map_err(|e| format!("Failed to validate gateway config: {e}"))?;

        if !output_mode.is_json() {
            println!(
                "✓ Gateway config ready: {} (channels: {})",
                gateway_path.display(),
                loaded.enabled_channels().join(", ")
            );
        }
        return Ok(());
    }

    let telegram_token = std::env::var("TELEGRAM_BOT_TOKEN").ok();
    let discord_token = std::env::var("DISCORD_BOT_TOKEN").ok();
    let slack_bot_token = std::env::var("SLACK_BOT_TOKEN").ok();
    let slack_app_token = std::env::var("SLACK_APP_TOKEN").ok();

    if non_interactive {
        if telegram_token.is_none()
            && discord_token.is_none()
            && (slack_bot_token.is_none() || slack_app_token.is_none())
        {
            return Err(
                "No gateway channels configured. For --non-interactive setup, set TELEGRAM_BOT_TOKEN, DISCORD_BOT_TOKEN, or both SLACK_BOT_TOKEN + SLACK_APP_TOKEN.".to_string(),
            );
        }

        let mut gateway_config = stakpak_gateway::GatewayConfig::default();
        if let Some(token) = telegram_token {
            gateway_config.channels.telegram = Some(stakpak_gateway::config::TelegramConfig {
                token,
                require_mention: false,
            });
        }
        if let Some(token) = discord_token {
            gateway_config.channels.discord = Some(stakpak_gateway::config::DiscordConfig {
                token,
                guilds: Vec::new(),
            });
        }
        if let (Some(bot_token), Some(app_token)) = (slack_bot_token, slack_app_token) {
            gateway_config.channels.slack = Some(stakpak_gateway::config::SlackConfig {
                bot_token,
                app_token,
            });
        }

        gateway_config
            .save(Some(gateway_path.as_path()))
            .map_err(|e| format!("Failed to save gateway config: {e}"))?;
    } else {
        crate::commands::gateway::GatewayCommands::Init {
            telegram_token,
            discord_token,
            slack_bot_token,
            slack_app_token,
            force: true,
        }
        .run(config.clone())
        .await
        .map_err(|e| format!("Failed to setup gateway: {e}"))?;
    }

    if !output_mode.is_json() {
        println!("✓ Gateway setup complete: {}", gateway_path.display());
    }
    Ok(())
}

async fn ensure_watch_setup(force: bool, output_mode: OutputMode) -> Result<(), String> {
    let watch_path = default_watch_config_path();

    if watch_path.exists() && !force {
        let config = crate::commands::watch::WatchConfig::load_default()
            .map_err(|e| format!("Failed to validate watch config: {e}"))?;
        if !output_mode.is_json() {
            println!(
                "✓ Watch config ready: {} ({} triggers)",
                watch_path.display(),
                config.triggers.len()
            );
        }
        return Ok(());
    }

    if output_mode.is_json() {
        write_default_watch_config(&watch_path, force)?;
    } else {
        crate::commands::watch::commands::init_config(force).await?;
    }

    if !output_mode.is_json() {
        println!("✓ Watch setup complete: {}", watch_path.display());
    }
    Ok(())
}

fn validate_start_output_mode(
    output_mode: OutputMode,
    options: &StartOptions,
) -> Result<(), String> {
    if output_mode.is_json() && options.foreground && !options.from_service {
        return Err("--json is not supported with --foreground mode".to_string());
    }

    Ok(())
}

async fn start_autopilot(
    config: &AppConfig,
    options: StartOptions,
    output_mode: OutputMode,
) -> Result<(), String> {
    let runtime_config_path = AutopilotRuntimeConfig::path();
    let saved_runtime_config = AutopilotRuntimeConfig::load_or_default()?;

    let has_runtime_overrides = options.has_runtime_overrides();
    let effective_runtime_config = if has_runtime_overrides {
        let runtime_config = AutopilotRuntimeConfig::from_start_options(&options);
        runtime_config.save()?;
        if !output_mode.is_json() {
            println!(
                "✓ Saved runtime overrides to {}",
                runtime_config_path.display()
            );
        }
        runtime_config
    } else {
        saved_runtime_config
    };

    let effective_options = options
        .clone()
        .with_runtime_config(&effective_runtime_config);

    validate_start_output_mode(output_mode, &effective_options)?;

    if effective_options.foreground || effective_options.from_service {
        return start_foreground_runtime(config, &effective_options).await;
    }

    let mut service_installed_now = false;
    if !autopilot_service_installed() {
        install_autopilot_service(config)?;
        service_installed_now = true;
        if !output_mode.is_json() {
            println!("✓ Installed autopilot service");
        }
    }

    start_autopilot_service()?;

    if output_mode.is_json() {
        print_json(&StartJsonResult {
            command: "autopilot.start",
            ok: true,
            started_via: "service",
            profile: config.profile_name.clone(),
            runtime_config_path: runtime_config_path.display().to_string(),
            runtime_config_updated: has_runtime_overrides,
            service_installed_now,
            effective: effective_runtime_config,
        })?;
    } else {
        println!("✓ Autopilot service started");
        println!("Run 'stakpak autopilot status' to inspect health.");
    }

    Ok(())
}

async fn start_foreground_runtime(
    config: &AppConfig,
    options: &StartOptions,
) -> Result<(), String> {
    let watch_task = if options.no_watch {
        None
    } else {
        Some(tokio::spawn(async {
            if let Err(error) = crate::commands::watch::commands::run_watch().await {
                eprintln!("Watch runtime exited: {}", error);
            }
        }))
    };

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to resolve current executable: {}", e))?;

    let mut serve_cmd = Command::new(current_exe);
    if config.profile_name != "default" {
        serve_cmd.arg("--profile").arg(&config.profile_name);
    }
    if !config.config_path.is_empty() {
        serve_cmd.arg("--config").arg(&config.config_path);
    }

    serve_cmd.arg("serve");
    serve_cmd.arg("--bind").arg(&options.bind);

    if options.show_token {
        serve_cmd.arg("--show-token");
    }
    if options.no_auth {
        serve_cmd.arg("--no-auth");
    }
    if options.auto_approve_all {
        serve_cmd.arg("--auto-approve-all");
    }
    if !options.no_gateway {
        serve_cmd.arg("--gateway");
    }
    if let Some(model) = options.model.as_ref() {
        serve_cmd.arg("--model").arg(model);
    }
    if let Some(path) = options.gateway_config.as_ref() {
        serve_cmd.arg("--gateway-config").arg(path);
    }

    serve_cmd.kill_on_drop(true);

    let mut child = serve_cmd
        .spawn()
        .map_err(|e| format!("Failed to start serve runtime: {}", e))?;

    println!("Autopilot running in foreground. Press Ctrl+C to stop.");

    tokio::select! {
        status = child.wait() => {
            if let Some(task) = watch_task {
                task.abort();
                let _ = task.await;
            }

            let status = status.map_err(|e| format!("Failed while waiting for serve runtime: {}", e))?;
            if !status.success() {
                return Err(format!(
                    "Serve runtime exited with status {}",
                    status
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".to_string())
                ));
            }
        }
        _ = wait_for_shutdown_signal() => {
            if let Some(pid) = child.id() {
                #[cfg(unix)]
                {
                    let _ = std::process::Command::new("kill")
                        .arg("-TERM")
                        .arg(pid.to_string())
                        .status();
                }
                #[cfg(windows)]
                {
                    let _ = std::process::Command::new("taskkill")
                        .args(["/PID", &pid.to_string()])
                        .status();
                }
            }

            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            let _ = child.start_kill();

            if let Some(task) = watch_task {
                task.abort();
                let _ = task.await;
            }
        }
    }

    Ok(())
}

async fn stop_autopilot(uninstall: bool) -> Result<(), String> {
    if autopilot_service_installed() {
        stop_autopilot_service()?;
        println!("✓ Autopilot service stopped");

        if uninstall {
            uninstall_autopilot_service()?;
            println!("✓ Autopilot service uninstalled");
        }
    } else {
        println!("Autopilot service is not installed.");
        println!("If running foreground mode, stop it with Ctrl+C.");
    }

    Ok(())
}

async fn status_autopilot(
    config: &AppConfig,
    output_mode: OutputMode,
    watch_runs: Option<u32>,
) -> Result<(), String> {
    let runtime = AutopilotRuntimeConfig::load_or_default()?;
    let runtime_config_path = AutopilotRuntimeConfig::path();
    let base_url = loopback_base_url_from_bind(&runtime.bind);

    let service_path = autopilot_service_path();
    let service = ServiceStatusJson {
        installed: autopilot_service_installed(),
        active: autopilot_service_active(),
        path: service_path.display().to_string(),
    };

    let server_url = format!("{}/v1/health", base_url);
    let server = EndpointStatusJson {
        expected_enabled: true,
        reachable: endpoint_ok(&server_url).await,
        url: server_url,
    };

    let gateway_url = format!("{}/v1/gateway/status", base_url);
    let gateway = EndpointStatusJson {
        expected_enabled: !runtime.no_gateway,
        reachable: endpoint_ok(&gateway_url).await,
        url: gateway_url,
    };

    let watch = if runtime.no_watch {
        WatchStatusJson {
            expected_enabled: false,
            config_path: default_watch_config_path().display().to_string(),
            config_valid: true,
            trigger_count: 0,
            running: false,
            pid: None,
            stale_pid: false,
            db_path: None,
            error: None,
            recent_runs: Vec::new(),
        }
    } else {
        collect_watch_status(watch_runs).await
    };

    if output_mode.is_json() {
        print_json(&AutopilotStatusJson {
            command: "autopilot.status",
            ok: true,
            profile: config.profile_name.clone(),
            runtime,
            runtime_config_path: runtime_config_path.display().to_string(),
            service,
            server,
            gateway,
            watch,
        })?;
        return Ok(());
    }

    println!("Autopilot status");
    println!("Profile: {}", config.profile_name);
    println!("Runtime config: {}", runtime_config_path.display());
    println!(
        "Service: {} ({})",
        if service.installed {
            if service.active {
                "active"
            } else {
                "installed but inactive"
            }
        } else {
            "not installed"
        },
        service.path
    );
    println!(
        "Server: {} ({})",
        if server.reachable {
            "reachable"
        } else {
            "unreachable"
        },
        server.url
    );
    println!(
        "Gateway: {} ({})",
        if gateway.reachable {
            "reachable"
        } else if !gateway.expected_enabled {
            "disabled by runtime config"
        } else {
            "unreachable"
        },
        gateway.url
    );

    println!();
    if !watch.expected_enabled {
        println!("Watch: disabled by runtime config");
    } else if watch.config_valid {
        let watch_state = if watch.running {
            format!("running (pid {})", watch.pid.unwrap_or_default())
        } else if watch.stale_pid {
            format!("stale pid {}", watch.pid.unwrap_or_default())
        } else {
            "not running".to_string()
        };

        println!("Watch: {}", watch_state);
        println!("Watch config: {}", watch.config_path);
        println!("Watch triggers: {}", watch.trigger_count);
        if !watch.recent_runs.is_empty() {
            println!();
            println!("Recent watch runs:");
            for run in &watch.recent_runs {
                println!(
                    "  #{} {:<16} {:<10} {}",
                    run.id, run.trigger_name, run.status, run.started_at
                );
            }
        }
    } else if let Some(error) = watch.error {
        println!("Watch: config invalid ({})", error);
        println!("Watch config: {}", watch.config_path);
    } else {
        println!("Watch: config invalid");
        println!("Watch config: {}", watch.config_path);
    }

    Ok(())
}

async fn logs_autopilot(follow: bool, lines: Option<u32>) -> Result<(), String> {
    match detect_platform() {
        Platform::Linux => {
            let mut cmd = std::process::Command::new("journalctl");
            cmd.args(["--user", "-u", AUTOPILOT_SYSTEMD_SERVICE]);
            if follow {
                cmd.arg("-f");
            }
            if let Some(lines) = lines {
                cmd.arg("-n").arg(lines.to_string());
            }

            let status = cmd
                .status()
                .map_err(|e| format!("Failed to run journalctl: {}", e))?;
            if !status.success() {
                return Err("Failed to read autopilot logs from journalctl".to_string());
            }
        }
        Platform::MacOS => {
            let log_dir = autopilot_log_dir();
            let stdout_log = log_dir.join("stdout.log");
            let stderr_log = log_dir.join("stderr.log");

            if follow {
                let mut cmd = std::process::Command::new("tail");
                cmd.arg("-f");
                if let Some(lines) = lines {
                    cmd.arg("-n").arg(lines.to_string());
                }
                cmd.arg(stdout_log);
                cmd.arg(stderr_log);

                let status = cmd
                    .status()
                    .map_err(|e| format!("Failed to tail autopilot logs: {}", e))?;
                if !status.success() {
                    return Err("Failed to tail autopilot logs".to_string());
                }
            } else {
                let mut cmd = std::process::Command::new("tail");
                if let Some(lines) = lines {
                    cmd.arg("-n").arg(lines.to_string());
                }
                cmd.arg(stdout_log);
                cmd.arg(stderr_log);

                let status = cmd
                    .status()
                    .map_err(|e| format!("Failed to read autopilot logs: {}", e))?;
                if !status.success() {
                    return Err("Failed to read autopilot logs".to_string());
                }
            }
        }
        Platform::Windows | Platform::Unknown => {
            return Err(
                "Autopilot logs are currently supported on Linux (journalctl) and macOS (tail)."
                    .to_string(),
            );
        }
    }

    Ok(())
}

async fn doctor_autopilot(config: &AppConfig) -> Result<(), String> {
    println!("Autopilot doctor");

    let mut failures = 0u32;

    let has_stakpak_key = config.get_stakpak_api_key().is_some();
    let has_provider_keys = !config.get_llm_provider_config().providers.is_empty();
    if has_stakpak_key || has_provider_keys {
        println!("✓ Credentials configured");
    } else {
        failures += 1;
        println!("✗ No credentials configured");
    }

    let runtime = match AutopilotRuntimeConfig::load_or_default() {
        Ok(runtime) => {
            println!(
                "✓ Autopilot runtime config loaded ({}, gateway={}, watch={})",
                runtime.bind,
                if runtime.no_gateway {
                    "disabled"
                } else {
                    "enabled"
                },
                if runtime.no_watch {
                    "disabled"
                } else {
                    "enabled"
                }
            );
            runtime
        }
        Err(e) => {
            failures += 1;
            println!("✗ Autopilot runtime config invalid: {}", e);
            AutopilotRuntimeConfig::default()
        }
    };

    if runtime.no_gateway {
        println!("✓ Gateway runtime disabled by autopilot config");
    } else {
        let gateway_path = stakpak_gateway::config::default_gateway_config_path();
        match stakpak_gateway::GatewayConfig::load(
            Some(gateway_path.as_path()),
            &stakpak_gateway::GatewayCliFlags::default(),
        ) {
            Ok(cfg) => println!(
                "✓ Gateway config valid ({}, channels: {})",
                gateway_path.display(),
                cfg.enabled_channels().join(", ")
            ),
            Err(e) => {
                failures += 1;
                println!("✗ Gateway config invalid: {}", e);
            }
        }
    }

    if runtime.no_watch {
        println!("✓ Watch runtime disabled by autopilot config");
    } else {
        let watch_status = collect_watch_status(None).await;
        if watch_status.config_valid {
            println!(
                "✓ Watch config valid ({} triggers)",
                watch_status.trigger_count
            );
        } else {
            failures += 1;
            println!(
                "✗ Watch config invalid: {}",
                watch_status
                    .error
                    .unwrap_or_else(|| "unknown watch configuration error".to_string())
            );
        }
    }

    if autopilot_service_installed() {
        println!("✓ Autopilot service installed");
    } else {
        failures += 1;
        println!("✗ Autopilot service not installed");
    }

    let base_url = loopback_base_url_from_bind(&runtime.bind);
    if endpoint_ok(&format!("{}/v1/health", base_url)).await {
        println!("✓ Server health endpoint reachable");
    } else {
        println!("⚠ Server health endpoint not reachable (not running is OK before start)");
    }

    if failures > 0 {
        return Err(format!("Doctor found {} blocking issue(s)", failures));
    }

    println!("✓ Doctor checks passed");
    Ok(())
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    let json = serde_json::to_string(value)
        .map_err(|e| format!("Failed to serialize JSON output: {}", e))?;
    println!("{}", json);
    Ok(())
}

fn default_watch_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".stakpak")
        .join("watch.toml")
}

fn write_default_watch_config(path: &Path, force: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create watch config directory: {}", e))?;
    }

    if force || !path.exists() {
        std::fs::write(path, DEFAULT_WATCH_CONFIG_TEMPLATE)
            .map_err(|e| format!("Failed to write watch config template: {}", e))?;
    }

    crate::commands::watch::WatchConfig::load_default()
        .map_err(|e| format!("Failed to validate watch config: {}", e))?;

    Ok(())
}

fn loopback_base_url_from_bind(bind: &str) -> String {
    match bind.parse::<SocketAddr>() {
        Ok(addr) => {
            let port = addr.port();
            match addr.ip() {
                IpAddr::V4(ip) => {
                    if ip.is_unspecified() {
                        format!("http://{}:{}", Ipv4Addr::LOCALHOST, port)
                    } else {
                        format!("http://{}:{}", ip, port)
                    }
                }
                IpAddr::V6(ip) => {
                    if ip.is_unspecified() {
                        format!("http://[{}]:{}", Ipv6Addr::LOCALHOST, port)
                    } else {
                        format!("http://[{}]:{}", ip, port)
                    }
                }
            }
        }
        Err(_) => "http://127.0.0.1:4096".to_string(),
    }
}

async fn collect_watch_status(watch_runs: Option<u32>) -> WatchStatusJson {
    let config_path = default_watch_config_path();

    let config = match crate::commands::watch::WatchConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            return WatchStatusJson {
                expected_enabled: true,
                config_path: config_path.display().to_string(),
                config_valid: false,
                trigger_count: 0,
                running: false,
                pid: None,
                stale_pid: false,
                db_path: None,
                error: Some(error.to_string()),
                recent_runs: Vec::new(),
            };
        }
    };

    let db_path = config.db_path();
    let db_path_str = db_path.to_string_lossy().to_string();

    let db = match db_path.to_str() {
        Some(path) => match crate::commands::watch::WatchDb::new(path).await {
            Ok(db) => db,
            Err(error) => {
                return WatchStatusJson {
                    expected_enabled: true,
                    config_path: config_path.display().to_string(),
                    config_valid: true,
                    trigger_count: config.triggers.len(),
                    running: false,
                    pid: None,
                    stale_pid: false,
                    db_path: Some(db_path_str),
                    error: Some(error.to_string()),
                    recent_runs: Vec::new(),
                };
            }
        },
        None => {
            return WatchStatusJson {
                expected_enabled: true,
                config_path: config_path.display().to_string(),
                config_valid: true,
                trigger_count: config.triggers.len(),
                running: false,
                pid: None,
                stale_pid: false,
                db_path: Some(db_path_str),
                error: Some("Invalid watch database path".to_string()),
                recent_runs: Vec::new(),
            };
        }
    };

    let watch_state = db.get_watch_state().await.ok().flatten();

    let (running, stale_pid, pid) = if let Some(state) = watch_state {
        let pid = state.pid;
        let running = u32::try_from(pid)
            .ok()
            .map(crate::commands::watch::is_process_running)
            .unwrap_or(false);
        (running, !running, Some(pid))
    } else {
        (false, false, None)
    };

    let recent_runs = if let Some(limit) = watch_runs.filter(|limit| *limit > 0) {
        match db
            .list_runs(&crate::commands::watch::ListRunsFilter {
                trigger_name: None,
                status: None,
                limit: Some(limit),
                offset: None,
            })
            .await
        {
            Ok(runs) => runs
                .into_iter()
                .map(|run| WatchRunSummaryJson {
                    id: run.id,
                    trigger_name: run.trigger_name,
                    status: run.status.to_string(),
                    started_at: run.started_at.to_rfc3339(),
                    finished_at: run.finished_at.map(|value| value.to_rfc3339()),
                    error_message: run.error_message,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    WatchStatusJson {
        expected_enabled: true,
        config_path: config_path.display().to_string(),
        config_valid: true,
        trigger_count: config.triggers.len(),
        running,
        pid,
        stale_pid,
        db_path: Some(db_path_str),
        error: None,
        recent_runs,
    }
}

async fn endpoint_ok(url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };

    match client.get(url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => {
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    MacOS,
    Linux,
    Windows,
    Unknown,
}

fn detect_platform() -> Platform {
    #[cfg(target_os = "macos")]
    {
        return Platform::MacOS;
    }
    #[cfg(target_os = "linux")]
    {
        return Platform::Linux;
    }
    #[cfg(target_os = "windows")]
    {
        return Platform::Windows;
    }
    #[allow(unreachable_code)]
    Platform::Unknown
}

const AUTOPILOT_SYSTEMD_SERVICE: &str = "stakpak-autopilot";
const AUTOPILOT_LAUNCHD_LABEL: &str = "dev.stakpak.autopilot";

fn autopilot_log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".stakpak")
        .join("autopilot")
        .join("logs")
}

fn autopilot_service_path() -> PathBuf {
    match detect_platform() {
        Platform::Linux => dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("systemd")
            .join("user")
            .join(format!("{}.service", AUTOPILOT_SYSTEMD_SERVICE)),
        Platform::MacOS => dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", AUTOPILOT_LAUNCHD_LABEL)),
        Platform::Windows | Platform::Unknown => PathBuf::new(),
    }
}

fn autopilot_service_installed() -> bool {
    let path = autopilot_service_path();
    !path.as_os_str().is_empty() && path.exists()
}

fn install_autopilot_service(config: &AppConfig) -> Result<(), String> {
    match detect_platform() {
        Platform::Linux => install_systemd_service(config),
        Platform::MacOS => install_launchd_service(config),
        Platform::Windows => Err("Windows autopilot service is not yet supported".to_string()),
        Platform::Unknown => Err("Unsupported platform for autopilot service".to_string()),
    }
}

fn uninstall_autopilot_service() -> Result<(), String> {
    match detect_platform() {
        Platform::Linux => uninstall_systemd_service(),
        Platform::MacOS => uninstall_launchd_service(),
        Platform::Windows => Err("Windows autopilot service is not yet supported".to_string()),
        Platform::Unknown => Err("Unsupported platform for autopilot service".to_string()),
    }
}

fn start_autopilot_service() -> Result<(), String> {
    match detect_platform() {
        Platform::Linux => {
            run_command(
                "systemctl",
                &["--user", "daemon-reload"],
                "Failed to reload systemd",
            )?;
            run_command(
                "systemctl",
                &["--user", "start", AUTOPILOT_SYSTEMD_SERVICE],
                "Failed to start autopilot service",
            )
        }
        Platform::MacOS => {
            let plist = autopilot_service_path();
            let load_output = std::process::Command::new("launchctl")
                .args(["load", plist.to_string_lossy().as_ref()])
                .output()
                .map_err(|e| format!("Failed to load launchd service: {}", e))?;

            if !load_output.status.success() {
                let stderr = String::from_utf8_lossy(&load_output.stderr);
                if !stderr.to_ascii_lowercase().contains("already loaded") {
                    return Err(format!("Failed to load launchd service: {}", stderr));
                }
            }

            run_command(
                "launchctl",
                &["start", AUTOPILOT_LAUNCHD_LABEL],
                "Failed to start launchd service",
            )
        }
        Platform::Windows => Err("Windows autopilot service is not yet supported".to_string()),
        Platform::Unknown => Err("Unsupported platform for autopilot service".to_string()),
    }
}

fn stop_autopilot_service() -> Result<(), String> {
    match detect_platform() {
        Platform::Linux => run_command(
            "systemctl",
            &["--user", "stop", AUTOPILOT_SYSTEMD_SERVICE],
            "Failed to stop autopilot service",
        ),
        Platform::MacOS => {
            let output = std::process::Command::new("launchctl")
                .args(["stop", AUTOPILOT_LAUNCHD_LABEL])
                .output()
                .map_err(|e| format!("Failed to stop launchd service: {}", e))?;

            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr
                    .to_ascii_lowercase()
                    .contains("could not find service")
                {
                    Ok(())
                } else {
                    Err(format!("Failed to stop launchd service: {}", stderr))
                }
            }
        }
        Platform::Windows => Err("Windows autopilot service is not yet supported".to_string()),
        Platform::Unknown => Err("Unsupported platform for autopilot service".to_string()),
    }
}

fn autopilot_service_active() -> bool {
    match detect_platform() {
        Platform::Linux => std::process::Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", AUTOPILOT_SYSTEMD_SERVICE])
            .status()
            .map(|status| status.success())
            .unwrap_or(false),
        Platform::MacOS => std::process::Command::new("launchctl")
            .args(["list", AUTOPILOT_LAUNCHD_LABEL])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false),
        Platform::Windows | Platform::Unknown => false,
    }
}

fn install_systemd_service(config: &AppConfig) -> Result<(), String> {
    let binary = std::env::current_exe()
        .map_err(|e| format!("Failed to resolve stakpak binary path: {}", e))?;
    let service_path = autopilot_service_path();

    if let Some(parent) = service_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create systemd directory: {}", e))?;
    }

    let log_dir = autopilot_log_dir();
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create autopilot log directory: {}", e))?;

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    let mut exec_parts = vec![binary.display().to_string()];
    if config.profile_name != "default" {
        exec_parts.push("--profile".to_string());
        exec_parts.push(config.profile_name.clone());
    }
    if !config.config_path.is_empty() {
        exec_parts.push("--config".to_string());
        exec_parts.push(config.config_path.clone());
    }
    exec_parts.extend([
        "autopilot".to_string(),
        "start".to_string(),
        "--foreground".to_string(),
        "--from-service".to_string(),
    ]);

    let unit = format!(
        "[Unit]\nDescription=Stakpak Autopilot Runtime\nAfter=network.target\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\nRestartSec=5\nWorkingDirectory={}\nEnvironment=HOME={}\nEnvironment=PATH=/usr/local/bin:/usr/bin:/bin\nStandardOutput=append:{}/stdout.log\nStandardError=append:{}/stderr.log\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n",
        shell_join(&exec_parts),
        home.display(),
        home.display(),
        log_dir.display(),
        log_dir.display(),
    );

    std::fs::write(&service_path, unit)
        .map_err(|e| format!("Failed to write systemd service file: {}", e))?;

    run_command(
        "systemctl",
        &["--user", "daemon-reload"],
        "Failed to reload systemd",
    )?;
    run_command(
        "systemctl",
        &["--user", "enable", AUTOPILOT_SYSTEMD_SERVICE],
        "Failed to enable autopilot service",
    )?;

    Ok(())
}

fn uninstall_systemd_service() -> Result<(), String> {
    let service_path = autopilot_service_path();

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "stop", AUTOPILOT_SYSTEMD_SERVICE])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", AUTOPILOT_SYSTEMD_SERVICE])
        .status();

    if service_path.exists() {
        std::fs::remove_file(&service_path)
            .map_err(|e| format!("Failed to remove systemd service file: {}", e))?;
    }

    run_command(
        "systemctl",
        &["--user", "daemon-reload"],
        "Failed to reload systemd",
    )?;

    Ok(())
}

fn install_launchd_service(config: &AppConfig) -> Result<(), String> {
    let binary = std::env::current_exe()
        .map_err(|e| format!("Failed to resolve stakpak binary path: {}", e))?;
    let plist_path = autopilot_service_path();

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create LaunchAgents directory: {}", e))?;
    }

    let log_dir = autopilot_log_dir();
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create autopilot log directory: {}", e))?;

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    let mut args = Vec::new();
    if config.profile_name != "default" {
        args.push("<string>--profile</string>".to_string());
        args.push(format!(
            "<string>{}</string>",
            xml_escape(&config.profile_name)
        ));
    }
    if !config.config_path.is_empty() {
        args.push("<string>--config</string>".to_string());
        args.push(format!(
            "<string>{}</string>",
            xml_escape(&config.config_path)
        ));
    }
    args.extend([
        "<string>autopilot</string>".to_string(),
        "<string>start</string>".to_string(),
        "<string>--foreground</string>".to_string(),
        "<string>--from-service</string>".to_string(),
    ]);

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        {}
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>WorkingDirectory</key>
    <string>{}</string>
    <key>StandardOutPath</key>
    <string>{}/stdout.log</string>
    <key>StandardErrorPath</key>
    <string>{}/stderr.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{}</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
</dict>
</plist>
"#,
        AUTOPILOT_LAUNCHD_LABEL,
        xml_escape(&binary.display().to_string()),
        args.join("\n        "),
        xml_escape(&home.display().to_string()),
        xml_escape(&log_dir.display().to_string()),
        xml_escape(&log_dir.display().to_string()),
        xml_escape(&home.display().to_string()),
    );

    std::fs::write(&plist_path, plist)
        .map_err(|e| format!("Failed to write launchd plist: {}", e))?;

    Ok(())
}

fn uninstall_launchd_service() -> Result<(), String> {
    let plist_path = autopilot_service_path();

    let _ = std::process::Command::new("launchctl")
        .args(["stop", AUTOPILOT_LAUNCHD_LABEL])
        .status();
    let _ = std::process::Command::new("launchctl")
        .args(["unload", plist_path.to_string_lossy().as_ref()])
        .status();

    if plist_path.exists() {
        std::fs::remove_file(&plist_path)
            .map_err(|e| format!("Failed to remove launchd plist: {}", e))?;
    }

    Ok(())
}

fn run_command(command: &str, args: &[&str], context: &str) -> Result<(), String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .map_err(|e| format!("{}: {}", context, e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}: {}",
            context,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn shell_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| {
            if part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.' | ':'))
            {
                part.clone()
            } else {
                format!("'{}'", part.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

const DEFAULT_WATCH_CONFIG_TEMPLATE: &str = r#"# Stakpak Watch Configuration

[defaults]
profile = "default"
timeout = "30m"
check_timeout = "30s"

[[triggers]]
name = "example-health-report"
schedule = "0 9 * * *"
prompt = """
Generate a concise health report for this environment.
Focus on read-only checks and summarize findings.
"""
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);

        std::env::temp_dir().join(format!(
            "stakpak-{}-{}-{}.toml",
            name,
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn runtime_config_roundtrip_save_load() {
        let path = temp_file_path("autopilot-runtime");

        let config = AutopilotRuntimeConfig {
            bind: "0.0.0.0:4111".to_string(),
            show_token: true,
            no_auth: true,
            model: Some("anthropic/claude-sonnet-4-5".to_string()),
            auto_approve_all: true,
            no_gateway: false,
            no_watch: true,
            gateway_config: Some(PathBuf::from("/tmp/gateway.toml")),
        };

        let save_result = config.save_to_path(&path);
        assert!(save_result.is_ok());

        let loaded = AutopilotRuntimeConfig::load_from_path(&path);
        assert!(loaded.is_ok());

        if let Ok(loaded) = loaded {
            assert_eq!(loaded.bind, "0.0.0.0:4111");
            assert!(loaded.show_token);
            assert!(loaded.no_auth);
            assert_eq!(loaded.model.as_deref(), Some("anthropic/claude-sonnet-4-5"));
            assert!(loaded.auto_approve_all);
            assert!(!loaded.no_gateway);
            assert!(loaded.no_watch);
            assert_eq!(
                loaded.gateway_config.as_deref(),
                Some(Path::new("/tmp/gateway.toml"))
            );
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loopback_base_url_resolves_unspecified_bind() {
        let v4 = loopback_base_url_from_bind("0.0.0.0:4096");
        let v6 = loopback_base_url_from_bind("[::]:4096");

        assert_eq!(v4, "http://127.0.0.1:4096");
        assert_eq!(v6, "http://[::1]:4096");
    }

    #[test]
    fn validate_start_output_mode_rejects_json_foreground() {
        let options = StartOptions {
            bind: "127.0.0.1:4096".to_string(),
            show_token: false,
            no_auth: false,
            model: None,
            auto_approve_all: false,
            no_gateway: false,
            no_watch: false,
            gateway_config: None,
            foreground: true,
            from_service: false,
        };

        let result = validate_start_output_mode(OutputMode::Json, &options);
        assert!(result.is_err());
        assert_eq!(
            result.err().as_deref(),
            Some("--json is not supported with --foreground mode")
        );
    }

    #[test]
    fn status_json_schema_contains_core_fields() {
        let payload = AutopilotStatusJson {
            command: "autopilot.status",
            ok: true,
            profile: "default".to_string(),
            runtime: AutopilotRuntimeConfig::default(),
            runtime_config_path: "/tmp/autopilot.toml".to_string(),
            service: ServiceStatusJson {
                installed: true,
                active: true,
                path: "/tmp/service".to_string(),
            },
            server: EndpointStatusJson {
                expected_enabled: true,
                reachable: true,
                url: "http://127.0.0.1:4096/v1/health".to_string(),
            },
            gateway: EndpointStatusJson {
                expected_enabled: true,
                reachable: false,
                url: "http://127.0.0.1:4096/v1/gateway/status".to_string(),
            },
            watch: WatchStatusJson {
                expected_enabled: true,
                config_path: "/tmp/watch.toml".to_string(),
                config_valid: true,
                trigger_count: 2,
                running: true,
                pid: Some(123),
                stale_pid: false,
                db_path: Some("/tmp/watch.db".to_string()),
                error: None,
                recent_runs: vec![WatchRunSummaryJson {
                    id: 1,
                    trigger_name: "example".to_string(),
                    status: "completed".to_string(),
                    started_at: "2026-01-01T00:00:00Z".to_string(),
                    finished_at: Some("2026-01-01T00:00:10Z".to_string()),
                    error_message: None,
                }],
            },
        };

        let json = serde_json::to_value(payload);
        assert!(json.is_ok());

        if let Ok(value) = json {
            assert_eq!(
                value.get("command").and_then(|v| v.as_str()),
                Some("autopilot.status")
            );
            assert!(value.get("runtime").is_some());
            assert!(value.get("service").is_some());
            assert!(value.get("server").is_some());
            assert!(value.get("gateway").is_some());
            assert!(value.get("watch").is_some());

            let watch_runs = value
                .get("watch")
                .and_then(|watch| watch.get("recent_runs"))
                .and_then(|runs| runs.as_array())
                .map(|runs| runs.len())
                .unwrap_or_default();
            assert_eq!(watch_runs, 1);
        }
    }
}
