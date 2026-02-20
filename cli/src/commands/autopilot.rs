use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::Utc;
use clap::{Args, Subcommand, ValueEnum};
use croner::Cron;
use serde::{Deserialize, Serialize};

use std::sync::Arc;

use stakpak_api::AgentProvider;

use crate::{
    config::AppConfig,
    onboarding::{OnboardingMode, run_onboarding},
};

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

    /// Run in foreground instead of delegating to OS service
    #[arg(long, default_value_t = false)]
    pub foreground: bool,

    /// Do not prompt; require env vars / pre-existing config for setup
    #[arg(long, default_value_t = false)]
    pub non_interactive: bool,

    /// Overwrite existing config (re-initialize from scratch)
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Args, PartialEq, Debug, Clone)]
pub struct StopArgs {
    /// Also remove installed OS service definition
    #[arg(long, default_value_t = false)]
    pub uninstall: bool,
}

#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum AutopilotCommands {
    /// Start autopilot and install as system service (runs setup on first use)
    #[command(name = "up")]
    Up {
        #[command(flatten)]
        args: StartArgs,

        /// Internal flag used by service units to avoid recursive delegation
        #[arg(long, hide = true, default_value_t = false)]
        from_service: bool,
    },

    /// Stop autopilot and remove system service
    #[command(name = "down")]
    Down {
        #[command(flatten)]
        args: StopArgs,
    },

    /// Show health, uptime, schedule/channel metadata, and recent activity
    Status {
        /// Emit machine-readable JSON output
        #[arg(long, default_value_t = false)]
        json: bool,

        /// Include recent schedule runs (count)
        #[arg(long)]
        recent_runs: Option<u32>,
    },

    /// Stream autopilot logs
    Logs {
        /// Follow log output
        #[arg(short = 'f', long)]
        follow: bool,

        /// Number of lines to show initially
        #[arg(short = 'n', long)]
        lines: Option<u32>,

        /// Filter logs by component
        #[arg(short = 'c', long, value_parser = ["scheduler", "server", "gateway"])]
        component: Option<String>,
    },

    /// Restart autopilot (reload config)
    Restart,

    /// Manage scheduled tasks
    #[command(subcommand)]
    Schedule(AutopilotScheduleCommands),

    /// Manage messaging channels (Slack, Telegram, Discord)
    #[command(subcommand)]
    Channel(AutopilotChannelCommands),

    /// Run preflight checks for autopilot setup/runtime
    Doctor,
}

#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum AutopilotScheduleCommands {
    /// List all schedules
    List,

    /// Add a schedule
    Add {
        /// Schedule name
        name: String,

        /// Cron expression
        #[arg(long)]
        cron: String,

        /// Prompt to run on trigger
        #[arg(long)]
        prompt: String,

        /// Check script path
        #[arg(long)]
        check: Option<String>,

        /// When to trigger after check
        #[arg(long, default_value_t = ScheduleTriggerOn::Failure)]
        trigger_on: ScheduleTriggerOn,

        // /// Working directory for this schedule
        // #[arg(long)]
        // workdir: Option<String>,
        /// Max agent steps
        #[arg(long, default_value_t = 50)]
        max_steps: u32,

        /// Report results to this channel
        #[arg(long)]
        channel: Option<String>,

        /// Require approval before acting
        #[arg(long, default_value_t = false)]
        pause_on_approval: bool,

        /// Run agent tool calls inside a sandboxed warden container
        #[arg(long, default_value_t = false)]
        sandbox: bool,

        /// Enable immediately
        #[arg(long, default_value_t = true)]
        enabled: bool,
    },

    /// Remove a schedule
    Remove { name: String },

    /// Enable a schedule
    Enable { name: String },

    /// Disable a schedule
    Disable { name: String },

    /// Show run history for a schedule
    History {
        /// Schedule name
        name: String,

        /// Number of rows to show
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
    },

    /// Manually trigger a schedule now
    Trigger {
        /// Schedule name
        name: String,

        /// Preview what would happen without actually triggering
        #[arg(long)]
        dry_run: bool,
    },

    /// Show details of a specific run
    Show {
        /// Run ID
        id: i64,
    },

    /// Clean up stale runs and optionally prune old history
    Clean {
        /// Also prune runs older than this many days
        #[arg(long)]
        older_than_days: Option<u32>,
    },
}

#[derive(Subcommand, PartialEq, Debug, Clone)]
pub enum AutopilotChannelCommands {
    /// List all channels
    List,

    /// Add a channel
    #[command(
        after_long_help = "HOW TO GET TOKENS:\n\n  Slack (requires both --bot-token and --app-token):\n    1. Create app at https://api.slack.com/apps\n    2. Enable Socket Mode → generate app-level token (xapp-...) with connections:write scope\n    3. OAuth & Permissions → add Bot Token Scopes:\n       app_mentions:read, channels:history, channels:read, chat:write,\n       groups:history, groups:read, im:history, im:read,\n       mpim:history, mpim:read, reactions:read, reactions:write\n    4. Event Subscriptions → subscribe to bot events:\n       message.channels, message.groups, message.im, app_mention\n    5. Install to Workspace → copy Bot User OAuth Token (xoxb-...)\n\n  Telegram:\n    1. Message @BotFather on Telegram\n    2. Send /newbot → choose name and username (must end in 'bot')\n    3. Copy the bot token (format: 123456789:ABCdef...)\n\n  Discord:\n    1. Create app at https://discord.com/developers/applications\n    2. Bot tab → copy the bot token\n    3. OAuth2 → enable bot scope and required permissions\n\n  Optional default notification target:\n    --target sets [notifications].channel/chat_id for watch alerts\n    Example: --target \"#engineering\" (Slack)\n"
    )]
    Add {
        /// Channel type (slack, telegram, discord)
        #[arg(value_enum)]
        channel_type: ChannelType,

        /// Bot token (Telegram bot token, Discord bot token)
        #[arg(long)]
        token: Option<String>,

        /// Slack bot token (xoxb-...)
        #[arg(long)]
        bot_token: Option<String>,

        /// Slack app token (xapp-...)
        #[arg(long)]
        app_token: Option<String>,

        /// Default notification target (Slack channel, Telegram chat_id, Discord channel_id)
        #[arg(long)]
        target: Option<String>,
    },

    /// Remove a channel
    Remove {
        /// Channel type (slack, telegram, discord)
        #[arg(value_enum)]
        channel_type: ChannelType,
    },

    /// Test channel connectivity
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleTriggerOn {
    Success,
    #[default]
    Failure,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Slack,
    Telegram,
    Discord,
    Whatsapp,
    Webhook,
}

impl AutopilotCommands {
    pub async fn run(self, mut config: AppConfig) -> Result<(), String> {
        match self {
            AutopilotCommands::Up { args, from_service } => {
                start_autopilot(
                    &mut config,
                    StartOptions {
                        bind: args.bind,
                        show_token: args.show_token,
                        no_auth: args.no_auth,
                        model: args.model,
                        auto_approve_all: args.auto_approve_all,
                        foreground: args.foreground,
                        from_service,
                        non_interactive: args.non_interactive,
                        force: args.force,
                    },
                )
                .await
            }
            AutopilotCommands::Down { args: _ } => stop_autopilot().await,
            AutopilotCommands::Status { json, recent_runs } => {
                status_autopilot(&config, json, recent_runs).await
            }
            AutopilotCommands::Logs {
                follow,
                lines,
                component,
            } => logs_autopilot(follow, lines, component).await,
            AutopilotCommands::Restart => restart_autopilot().await,
            AutopilotCommands::Schedule(command) => run_schedule_command(command).await,
            AutopilotCommands::Channel(command) => run_channel_command(command).await,
            AutopilotCommands::Doctor => doctor_autopilot(&config).await,
        }
    }
}

#[derive(Debug, Clone)]
struct StartOptions {
    bind: String,
    show_token: bool,
    no_auth: bool,
    model: Option<String>,
    auto_approve_all: bool,
    foreground: bool,
    from_service: bool,
    non_interactive: bool,
    force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AutopilotConfigFile {
    #[serde(default)]
    server: AutopilotServerConfig,
    #[serde(default)]
    schedules: Vec<AutopilotScheduleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutopilotServerConfig {
    #[serde(default = "default_server_listen")]
    listen: String,
    #[serde(default)]
    show_token: bool,
    #[serde(default)]
    no_auth: bool,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    auto_approve_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutopilotScheduleConfig {
    name: String,
    cron: String,
    prompt: String,
    #[serde(default)]
    check: Option<String>,
    #[serde(default)]
    trigger_on: ScheduleTriggerOn,
    // #[serde(default)]
    // workdir: Option<String>,
    #[serde(default = "default_schedule_max_steps")]
    max_steps: u32,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    pause_on_approval: bool,
    #[serde(default)]
    sandbox: bool,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

impl Default for AutopilotServerConfig {
    fn default() -> Self {
        Self {
            listen: default_server_listen(),
            show_token: false,
            no_auth: false,
            model: None,
            auto_approve_all: false,
        }
    }
}

impl AutopilotConfigFile {
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

    async fn load_or_default_async() -> Result<Self, String> {
        tokio::task::spawn_blocking(Self::load_or_default)
            .await
            .map_err(|e| format!("Failed to join config load task: {}", e))?
    }

    fn load_from_path(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read autopilot config {}: {}", path.display(), e))?;

        let config: Self = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse autopilot config {}: {}", path.display(), e))?;

        Ok(config)
    }

    fn save(&self) -> Result<PathBuf, String> {
        let path = Self::path();
        self.save_to_path(&path)?;
        Ok(path)
    }

    fn save_to_path(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create autopilot config dir: {}", e))?;
        }

        let mut root = load_toml_root_table(path)?;

        {
            let server = ensure_toml_table(&mut root, "server");
            server.insert(
                "listen".to_string(),
                toml::Value::String(self.server.listen.clone()),
            );
            server.insert(
                "show_token".to_string(),
                toml::Value::Boolean(self.server.show_token),
            );
            server.insert(
                "no_auth".to_string(),
                toml::Value::Boolean(self.server.no_auth),
            );
            match &self.server.model {
                Some(model) => {
                    server.insert("model".to_string(), toml::Value::String(model.clone()));
                }
                None => {
                    server.remove("model");
                }
            }
            server.insert(
                "auto_approve_all".to_string(),
                toml::Value::Boolean(self.server.auto_approve_all),
            );
        }

        root.insert(
            "schedules".to_string(),
            toml::Value::try_from(&self.schedules)
                .map_err(|e| format!("Failed to serialize schedules: {}", e))?,
        );

        write_toml_root_table(path, root)
    }

    fn find_schedule(&self, name: &str) -> Option<&AutopilotScheduleConfig> {
        self.schedules.iter().find(|schedule| schedule.name == name)
    }

    fn find_schedule_mut(&mut self, name: &str) -> Option<&mut AutopilotScheduleConfig> {
        self.schedules
            .iter_mut()
            .find(|schedule| schedule.name == name)
    }
}

impl std::fmt::Display for ScheduleTriggerOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleTriggerOn::Success => write!(f, "success"),
            ScheduleTriggerOn::Failure => write!(f, "failure"),
            ScheduleTriggerOn::Any => write!(f, "any"),
        }
    }
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelType::Slack => write!(f, "slack"),
            ChannelType::Telegram => write!(f, "telegram"),
            ChannelType::Discord => write!(f, "discord"),
            ChannelType::Whatsapp => write!(f, "whatsapp"),
            ChannelType::Webhook => write!(f, "webhook"),
        }
    }
}

fn default_server_listen() -> String {
    "127.0.0.1:4096".to_string()
}

fn default_enabled() -> bool {
    true
}

fn default_schedule_max_steps() -> u32 {
    50
}

fn load_toml_root_table(path: &Path) -> Result<toml::value::Table, String> {
    if !path.exists() {
        return Ok(toml::value::Table::new());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read autopilot config {}: {}", path.display(), e))?;

    let value: toml::Value = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse autopilot config {}: {}", path.display(), e))?;

    match value {
        toml::Value::Table(table) => Ok(table),
        _ => Err(format!(
            "Failed to parse autopilot config {}: top-level TOML value must be a table",
            path.display()
        )),
    }
}

fn ensure_toml_table<'a>(
    table: &'a mut toml::value::Table,
    key: &str,
) -> &'a mut toml::value::Table {
    if !matches!(table.get(key), Some(toml::Value::Table(_))) {
        table.insert(
            key.to_string(),
            toml::Value::Table(toml::value::Table::new()),
        );
    }

    match table.get_mut(key) {
        Some(toml::Value::Table(subtable)) => subtable,
        _ => unreachable!("table key was just initialized"),
    }
}

fn write_toml_root_table(path: &Path, root: toml::value::Table) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create autopilot config dir: {}", e))?;
    }

    let content = toml::to_string_pretty(&toml::Value::Table(root))
        .map_err(|e| format!("Failed to serialize autopilot config: {}", e))?;

    std::fs::write(path, content)
        .map_err(|e| format!("Failed to write autopilot config {}: {}", path.display(), e))
}

impl StartOptions {
    fn with_server_config(mut self, server: &AutopilotServerConfig) -> Self {
        self.bind = server.listen.clone();
        self.show_token = server.show_token;
        self.no_auth = server.no_auth;
        self.model = server.model.clone();
        self.auto_approve_all = server.auto_approve_all;
        self
    }

    fn has_runtime_overrides(&self) -> bool {
        self.bind != "127.0.0.1:4096"
            || self.show_token
            || self.no_auth
            || self.model.is_some()
            || self.auto_approve_all
    }
}

impl AutopilotServerConfig {
    fn from_start_options(options: &StartOptions) -> Self {
        Self {
            listen: options.bind.clone(),
            show_token: options.show_token,
            no_auth: options.no_auth,
            model: options.model.clone(),
            auto_approve_all: options.auto_approve_all,
        }
    }
}

#[derive(Debug, Serialize)]
struct AutopilotStatusJson {
    command: &'static str,
    ok: bool,
    profile: String,
    config_path: String,
    server_config: AutopilotServerConfig,
    service: ServiceStatusJson,
    server: EndpointStatusJson,
    gateway: EndpointStatusJson,
    scheduler: SchedulerStatusJson,
    schedules: Vec<AutopilotScheduleStatusJson>,
    channels: Vec<AutopilotChannelStatusJson>,
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
struct SchedulerStatusJson {
    expected_enabled: bool,
    config_path: String,
    config_valid: bool,
    trigger_count: usize,
    running: bool,
    pid: Option<i64>,
    stale_pid: bool,
    db_path: Option<String>,
    error: Option<String>,
    recent_runs: Vec<ScheduleRunSummaryJson>,
}

#[derive(Debug, Serialize)]
struct ScheduleRunSummaryJson {
    id: i64,
    schedule_name: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Serialize)]
struct AutopilotScheduleStatusJson {
    name: String,
    cron: String,
    enabled: bool,
    next_run: Option<String>,
}

#[derive(Debug, Serialize)]
struct AutopilotChannelStatusJson {
    name: String,
    channel_type: String,
    target: String,
    enabled: bool,
    alerts_only: bool,
}

async fn start_autopilot(config: &mut AppConfig, options: StartOptions) -> Result<(), String> {
    let autopilot_config_path = AutopilotConfigFile::path();
    let needs_setup = !autopilot_config_path.exists() || options.force;

    // ── First-run setup (merged from old `init`) ──────────────────────
    if needs_setup {
        println!("Stakpak Autopilot setup");
        println!("Profile: {}", config.profile_name);
        println!();

        // Credential check — interactive gets onboarding wizard, non-interactive gets error
        let has_stakpak_key = config.get_stakpak_api_key().is_some();
        let has_provider_keys = !config.get_llm_provider_config().providers.is_empty();

        if !has_stakpak_key && !has_provider_keys {
            if options.non_interactive {
                return Err(
                    "No provider credentials configured. Run with credentials in env or run interactive setup without --non-interactive.".to_string(),
                );
            }

            println!("No credentials found. Launching onboarding...");
            run_onboarding(config, OnboardingMode::Default).await;
            println!();
        }

        // Write default config template (or overwrite with --force)
        write_default_autopilot_config(&autopilot_config_path, options.force).await?;
        println!(
            "✓ Autopilot config created: {}",
            autopilot_config_path.display()
        );

        // Pick up channel tokens from environment
        let telegram_token = std::env::var("TELEGRAM_BOT_TOKEN").ok();
        let discord_token = std::env::var("DISCORD_BOT_TOKEN").ok();
        let slack_bot_token = std::env::var("SLACK_BOT_TOKEN").ok();
        let slack_app_token = std::env::var("SLACK_APP_TOKEN").ok();

        let has_env_channels = telegram_token.is_some()
            || discord_token.is_some()
            || (slack_bot_token.is_some() && slack_app_token.is_some());

        if has_env_channels {
            let mut gateway_config = stakpak_gateway::GatewayConfig::load(
                autopilot_config_path.as_path(),
                &stakpak_gateway::GatewayCliFlags::default(),
            )
            .unwrap_or_default();

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
                .save(autopilot_config_path.as_path())
                .map_err(|e| format!("Failed to save channel config: {e}"))?;

            let channels = gateway_config.enabled_channels();
            println!(
                "✓ Channels configured from environment: {}",
                channels.join(", ")
            );
        } else if !options.non_interactive {
            println!();
            println!("Channels let autopilot talk to you on Slack, Telegram, or Discord.");
            println!("You can add them now or later with: stakpak autopilot channel add");
            println!();
        }
    }

    // ── Load config and apply runtime overrides ──────────────────────
    let config_path = AutopilotConfigFile::path();
    let saved_config = AutopilotConfigFile::load_or_default()?;

    let has_runtime_overrides = options.has_runtime_overrides();
    let effective_server = if has_runtime_overrides || needs_setup {
        let server_config = AutopilotServerConfig::from_start_options(&options);
        let mut config_file = saved_config.clone();
        config_file.server = server_config.clone();
        config_file.save()?;
        if has_runtime_overrides && !needs_setup {
            println!("✓ Saved server overrides to {}", config_path.display());
        }
        server_config
    } else {
        saved_config.server.clone()
    };

    let effective_options = options.clone().with_server_config(&effective_server);

    if effective_options.foreground || effective_options.from_service {
        return start_foreground_runtime(config, &effective_options).await;
    }

    // Idempotency: if autopilot is already running, skip start
    if let Some(pid) = is_autopilot_running() {
        println!("Autopilot is already running (PID {}).", pid);
        println!();
        println!("  Status      stakpak autopilot status");
        println!("  Restart     stakpak autopilot restart");
        println!("  Stop        stakpak autopilot down");
        return Ok(());
    }

    if !autopilot_service_installed() {
        install_autopilot_service(config)?;
        println!("✓ Installed autopilot service");
    }

    start_autopilot_service()?;

    // Clean post-start status summary
    let schedule_count = saved_config.schedules.len();
    let channel_list = stakpak_gateway::GatewayConfig::load(
        config_path.as_path(),
        &stakpak_gateway::GatewayCliFlags::default(),
    )
    .map(|c| c.enabled_channels().join(", "))
    .unwrap_or_default();

    println!();
    println!("  Autopilot is running.");
    println!();
    println!("  Server      http://{}", effective_server.listen);
    println!(
        "  Schedules   {}",
        if schedule_count > 0 {
            format!("{} active", schedule_count)
        } else {
            "none (edit ~/.stakpak/autopilot.toml)".to_string()
        }
    );
    println!(
        "  Channels    {}",
        if channel_list.is_empty() {
            "none (stakpak autopilot channel add)".to_string()
        } else {
            channel_list
        }
    );
    println!();
    println!("  View logs   stakpak autopilot logs");
    println!("  Status      stakpak autopilot status");
    println!("  Stop        stakpak down");

    Ok(())
}

async fn start_foreground_runtime(
    config: &AppConfig,
    options: &StartOptions,
) -> Result<(), String> {
    // --- Per-component file logging ---
    // Each runtime gets its own log file under ~/.stakpak/autopilot/logs/.
    // Guards must be held for the lifetime of the runtime to ensure logs are flushed.
    let log_dir = autopilot_log_dir();
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create autopilot log directory: {}", e))?;

    // TODO: add log rotation (daily or size-based) via tracing_appender::rolling::daily()
    let scheduler_appender = tracing_appender::rolling::never(&log_dir, "scheduler.log");
    let server_appender = tracing_appender::rolling::never(&log_dir, "server.log");
    let gateway_appender = tracing_appender::rolling::never(&log_dir, "gateway.log");

    let (scheduler_nb, _scheduler_guard) = tracing_appender::non_blocking(scheduler_appender);
    let (server_nb, _server_guard) = tracing_appender::non_blocking(server_appender);
    let (gateway_nb, _gateway_guard) = tracing_appender::non_blocking(gateway_appender);

    {
        use tracing_subscriber::fmt::writer::MakeWriterExt;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        // Each component gets its own fmt layer with a target-based filter.
        let scheduler_layer = tracing_subscriber::fmt::layer()
            .with_writer(scheduler_nb.with_filter(|meta: &tracing::Metadata<'_>| {
                meta.target().starts_with("stakpak::commands::watch")
            }))
            .with_target(true)
            .with_ansi(false);

        let server_layer = tracing_subscriber::fmt::layer()
            .with_writer(server_nb.with_filter(|meta: &tracing::Metadata<'_>| {
                meta.target().starts_with("stakpak_server")
            }))
            .with_target(true)
            .with_ansi(false);

        let gateway_layer = tracing_subscriber::fmt::layer()
            .with_writer(gateway_nb.with_filter(|meta: &tracing::Metadata<'_>| {
                meta.target().starts_with("stakpak_gateway")
            }))
            .with_target(true)
            .with_ansi(false);

        let env_filter =
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "info,stakpak=info,stakpak_server=info,stakpak_gateway=info".into()
            });

        // Best-effort: if a global subscriber is already set (e.g. --debug), skip.
        let _ = tracing_subscriber::registry()
            .with(env_filter)
            .with(scheduler_layer)
            .with(server_layer)
            .with(gateway_layer)
            .try_init();
    }

    // --- 1. Server runtime (initialize before scheduler to avoid sqlite3Close/sqlite3_open
    //     race on libsql's global state when run_scheduler exits early) ---
    let bind = options.bind.clone();
    let (auth_config, generated_auth_token) = if options.no_auth {
        (stakpak_server::AuthConfig::disabled(), None)
    } else {
        let token = stakpak_shared::utils::generate_password(64, true);
        (
            stakpak_server::AuthConfig::token(token.clone()),
            Some(token),
        )
    };

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("Failed to bind {}: {}", bind, e))?;

    let listener_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to inspect listener address: {}", e))?;

    let runtime_client = crate::commands::build_agent_client(config).await?;
    let storage = runtime_client.session_storage().clone();

    let events = Arc::new(stakpak_server::EventLog::new(4096));
    let idempotency = Arc::new(stakpak_server::IdempotencyStore::new(
        std::time::Duration::from_secs(24 * 60 * 60),
    ));
    let inference = Arc::new(
        stakai::Inference::builder()
            .with_registry(runtime_client.stakai().registry().clone())
            .build()
            .map_err(|e| format!("Failed to initialize inference runtime: {}", e))?,
    );

    let mut models = runtime_client.list_models().await;
    let requested_model = options.model.clone().or(config.model.clone());
    let auto_approve_tools = config.auto_approve.clone();
    let allowed_tools = config.allowed_tools.clone();

    let requested_model_from_catalog = requested_model.as_deref().and_then(|name| {
        if let Some((provider, id)) = name.split_once('/') {
            return models
                .iter()
                .find(|model| model.provider == provider && model.id == id)
                .cloned();
        }
        models.iter().find(|model| model.id == name).cloned()
    });

    let requested_custom_model = requested_model.as_deref().and_then(|name| {
        name.split_once('/')
            .map(|(provider, id)| stakai::Model::custom(id, provider))
    });

    let default_model = requested_model_from_catalog
        .clone()
        .or(requested_custom_model)
        .or_else(|| models.first().cloned())
        .or_else(|| Some(stakai::Model::custom("gpt-4o-mini", "openai")));

    if let Some(requested) = requested_model.as_deref()
        && requested_model_from_catalog.is_none()
    {
        if requested.contains('/') {
            eprintln!(
                "⚠ Requested model '{}' is not in the catalog; using it as a custom model id.",
                requested
            );
        } else if let Some(resolved) = default_model.as_ref() {
            eprintln!(
                "⚠ Requested model '{}' not found in catalog; using fallback '{}/{}'.",
                requested, resolved.provider, resolved.id
            );
        }
    }

    if models.is_empty()
        && let Some(default_model) = default_model.clone()
    {
        models.push(default_model);
    }

    let tool_approval_policy = if options.auto_approve_all {
        stakpak_server::ToolApprovalPolicy::All
    } else {
        let policy = stakpak_server::ToolApprovalPolicy::with_defaults();
        if let Some(ref auto_approve_tools) = auto_approve_tools {
            policy.with_overrides(
                auto_approve_tools
                    .iter()
                    .cloned()
                    .map(|tool| (tool, stakpak_server::ToolApprovalAction::Approve)),
            )
        } else {
            policy
        }
    };

    let mcp_init_config = crate::commands::agent::run::mcp_init::McpInitConfig {
        redact_secrets: true, // applied in proxy layer
        privacy_mode: false,  // applied in proxy layer
        enabled_tools: stakpak_mcp_server::EnabledToolsConfig { slack: false },
        enable_mtls: true,
        enable_subagents: true,
        allowed_tools,
        subagent_config: stakpak_mcp_server::SubagentConfig {
            profile_name: Some(config.profile_name.clone()),
            config_path: Some(config.config_path.clone()),
        },
    };

    let mcp_init_result = crate::commands::agent::run::mcp_init::initialize_mcp_server_and_tools(
        config,
        mcp_init_config,
        None,
    )
    .await
    .map_err(|e| format!("Failed to initialize MCP stack: {}", e))?;

    let mcp_tools = mcp_init_result
        .mcp_tools
        .iter()
        .map(|tool| stakai::Tool {
            tool_type: "function".to_string(),
            function: stakai::ToolFunction {
                name: tool.name.as_ref().to_string(),
                description: tool
                    .description
                    .as_ref()
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                parameters: serde_json::Value::Object((*tool.input_schema).clone()),
            },
            provider_options: None,
        })
        .collect();

    let app_state = stakpak_server::AppState::new(
        storage,
        events,
        idempotency,
        inference,
        models,
        default_model,
        tool_approval_policy,
    )
    .with_mcp(
        mcp_init_result.client,
        mcp_tools,
        Some(mcp_init_result.server_shutdown_tx),
        Some(mcp_init_result.proxy_shutdown_tx),
    );

    // --- 1b. Sandbox configuration (warden + container image) ---
    let warden_path = crate::commands::warden::get_warden_plugin_path().await;
    let stakpak_image = crate::commands::warden::stakpak_agent_image();
    let volumes = crate::commands::warden::prepare_volumes(config, false);
    // Pre-create named volumes to prevent race conditions when parallel sandboxes start
    stakpak_shared::container::ensure_named_volumes_exist();
    let sandbox_config = stakpak_server::SandboxConfig {
        warden_path,
        image: stakpak_image.clone(),
        volumes,
    };
    tracing::info!(image = %stakpak_image, warden = %sandbox_config.warden_path, "Sandbox config initialized");
    let app_state = app_state.with_sandbox(sandbox_config);

    // --- 2. Loopback connection for schedule + gateway runtimes ---
    let loopback_url = loopback_server_url(listener_addr);
    let loopback_token = if options.no_auth {
        String::new()
    } else {
        generated_auth_token.clone().unwrap_or_default()
    };

    // --- 3. Gateway runtime ---
    let config_path = AutopilotConfigFile::path();

    let gateway_cli = stakpak_gateway::GatewayCliFlags {
        url: Some(loopback_url.clone()),
        token: Some(loopback_token.clone()),
        ..Default::default()
    };

    let mut gateway_cfg = stakpak_gateway::GatewayConfig::load(config_path.as_path(), &gateway_cli)
        .unwrap_or_default();

    if options.auto_approve_all {
        gateway_cfg.gateway.approval_mode = stakpak_gateway::ApprovalMode::AllowAll;
        gateway_cfg.gateway.approval_allowlist.clear();
    } else if let Some(auto_approve_tools) = auto_approve_tools.as_ref() {
        gateway_cfg.gateway.approval_mode = stakpak_gateway::ApprovalMode::Allowlist;
        gateway_cfg.gateway.approval_allowlist =
            expand_gateway_approval_allowlist(auto_approve_tools);
    }

    let gateway_runtime = if gateway_cfg.has_channels() {
        match stakpak_gateway::Gateway::new(gateway_cfg).await {
            Ok(gw) => Some(Arc::new(gw)),
            Err(e) => {
                eprintln!(
                    "⚠ Failed to initialize gateway: {}. Continuing without channels.",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    // --- Build HTTP app ---
    let refresh_state = app_state.clone();
    let (refresh_shutdown_tx, mut refresh_shutdown_rx) = tokio::sync::watch::channel(false);
    let refresh_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                    if let Err(error) = refresh_state.refresh_mcp_tools().await {
                        eprintln!("[mcp-refresh] {}", error);
                    }
                }
                changed = refresh_shutdown_rx.changed() => {
                    if changed.is_err() || *refresh_shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });

    let shutdown_state = app_state.clone();
    let shutdown_refresh_tx = refresh_shutdown_tx.clone();
    let server_model_id = app_state
        .default_model
        .as_ref()
        .map(|m| format!("{}/{}", m.provider, m.id));

    let base_app = stakpak_server::router(app_state, auth_config);
    let app = if let Some(gateway_runtime) = gateway_runtime.as_ref() {
        let gateway_routes = gateway_runtime.api_router();
        base_app.nest_service("/v1/gateway", gateway_routes.into_service())
    } else {
        base_app
    };

    let gateway_cancel = tokio_util::sync::CancellationToken::new();
    let gateway_task = if let Some(gateway_runtime) = gateway_runtime.as_ref() {
        let gateway_runtime = gateway_runtime.clone();
        let cancel = gateway_cancel.clone();
        Some(tokio::spawn(
            async move { gateway_runtime.run(cancel).await },
        ))
    } else {
        None
    };
    let gateway_cancel_for_shutdown = gateway_cancel.clone();

    // --- 4. Schedule runtime (spawned AFTER all SQLite initialization to avoid
    //     sqlite3Close/sqlite3_open race in libsql on musl) ---
    let schedule_server = crate::commands::watch::AgentServerConnection {
        url: loopback_url,
        token: loopback_token,
        model: server_model_id,
    };
    let schedule_task = tokio::spawn(async move {
        if let Err(error) = crate::commands::watch::commands::run_scheduler(schedule_server).await {
            eprintln!("Schedule runtime exited: {}", error);
        }
    });

    // --- Print status ---
    println!("Autopilot running in foreground. Press Ctrl+C to stop.");
    println!();
    println!("  Server      http://{}", bind);
    if let Some(ref token) = generated_auth_token {
        if options.show_token {
            println!("  Auth token  Bearer {}", token);
        } else {
            println!("  Auth        enabled");
        }
    } else if options.no_auth {
        println!("  Auth        disabled");
    }
    if gateway_runtime.is_some() {
        println!("  Gateway     enabled");
    } else {
        println!("  Gateway     no channels configured");
    }

    // --- Shutdown handler ---
    let shutdown = async move {
        wait_for_shutdown_signal().await;

        gateway_cancel_for_shutdown.cancel();

        for (session_id, run_id) in shutdown_state.run_manager.running_runs().await {
            let _ = shutdown_state
                .run_manager
                .cancel_run(session_id, run_id)
                .await;
        }

        let drain_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if shutdown_state.run_manager.running_runs().await.is_empty()
                || tokio::time::Instant::now() >= drain_deadline
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let _ = shutdown_refresh_tx.send(true);

        if let Some(tx) = shutdown_state.mcp_server_shutdown_tx.as_ref() {
            let _ = tx.send(());
        }
        if let Some(tx) = shutdown_state.mcp_proxy_shutdown_tx.as_ref() {
            let _ = tx.send(());
        }
    };

    // --- Run server ---
    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;

    // --- Cleanup ---
    gateway_cancel.cancel();
    if let Some(task) = gateway_task {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("Gateway runtime error: {}", e),
            Err(e) => eprintln!("Gateway runtime task failed: {}", e),
        }
    }

    let _ = refresh_shutdown_tx.send(true);
    if !refresh_task.is_finished() {
        refresh_task.abort();
    }
    let _ = refresh_task.await;

    // Abort the schedule task
    schedule_task.abort();
    let _ = schedule_task.await;

    println!();
    println!("Shutting down...");

    serve_result.map_err(|e| format!("Server error: {}", e))?;

    Ok(())
}

fn loopback_server_url(listener_addr: std::net::SocketAddr) -> String {
    let port = listener_addr.port();
    if listener_addr.ip().is_ipv6() {
        format!("http://[::1]:{port}")
    } else {
        format!("http://127.0.0.1:{port}")
    }
}

fn expand_gateway_approval_allowlist(tools: &[String]) -> Vec<String> {
    let mut normalized = std::collections::BTreeSet::new();
    for tool in tools {
        let trimmed = tool.trim();
        if trimmed.is_empty() {
            continue;
        }
        normalized.insert(trimmed.to_string());
        if !trimmed.starts_with("stakpak__") {
            normalized.insert(format!("stakpak__{trimmed}"));
        }
    }
    normalized.into_iter().collect()
}

async fn stop_autopilot() -> Result<(), String> {
    let mut stopped = false;

    // Stop and uninstall the system service
    if autopilot_service_installed() {
        // Ignore errors from stop — service might not be running
        let _ = stop_autopilot_service();
        uninstall_autopilot_service()?;
        stopped = true;
        println!("✓ Autopilot service stopped and uninstalled");
    }

    // Also kill any foreground process via PID file
    if let Some(pid) = is_autopilot_running() {
        #[cfg(unix)]
        {
            // Send SIGTERM to the entire process group to kill child processes too
            // Try negative PID first (process group), fall back to single PID
            let pgid_result = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(format!("-{}", pid))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            // If process group kill failed (e.g., not a group leader), kill the process directly
            if pgid_result.is_err() || !pgid_result.unwrap_or_default().success() {
                let _ = std::process::Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
        #[cfg(windows)]
        {
            // /T flag kills the process tree (parent + children)
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T"])
                .status();
        }

        // Wait briefly for process to exit
        for _ in 0..10 {
            if !crate::commands::watch::is_process_running(pid) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // If still running after SIGTERM, force kill
        if crate::commands::watch::is_process_running(pid) {
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("kill")
                    .arg("-9")
                    .arg(format!("-{}", pid))
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                let _ = std::process::Command::new("kill")
                    .arg("-9")
                    .arg(pid.to_string())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .status();
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // Clean up PID file if process is gone
        if !crate::commands::watch::is_process_running(pid) {
            let config = crate::commands::watch::ScheduleConfig::load_default().ok();
            if let Some(config) = config {
                let pid_file = config
                    .db_path()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("autopilot.pid");
                let _ = std::fs::remove_file(&pid_file);
            }
        }

        if !stopped {
            println!("✓ Autopilot process stopped (PID {})", pid);
        }
        stopped = true;
    }

    if !stopped {
        println!("Autopilot is not running.");
    }

    Ok(())
}

async fn restart_autopilot() -> Result<(), String> {
    // 1. Validate the autopilot config (server + schedules)
    println!("Validating autopilot configuration...");
    let autopilot_config = AutopilotConfigFile::load_or_default()?;

    for schedule in &autopilot_config.schedules {
        validate_schedule(schedule)?;
    }
    let config_path = AutopilotConfigFile::path();
    let channel_count = gateway_channel_count(config_path.as_path())?;

    println!(
        "  ✓ {} schedule(s), {} channel(s), server listen={}",
        autopilot_config.schedules.len(),
        channel_count,
        autopilot_config.server.listen,
    );

    // 2. Validate the watch/scheduler config (cron parsing, check scripts, db/log paths)
    match crate::commands::watch::ScheduleConfig::load_default() {
        Ok(config) => {
            println!(
                "  ✓ Scheduler config valid ({} schedules)",
                config.schedules.len()
            );
        }
        Err(e) => {
            return Err(format!(
                "Scheduler configuration error: {}\nFix {} and try again.",
                e,
                AutopilotConfigFile::path().display(),
            ));
        }
    }

    // 3. Restart: service path or foreground PID
    if autopilot_service_installed() {
        println!("\nRestarting autopilot service...");
        stop_autopilot_service()?;
        // Wait for the old process to fully exit before starting the new one.
        // launchctl stop is async — the process may still be shutting down.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        start_autopilot_service()?;
        println!("✓ Autopilot service restarted (scheduler, server, gateway)");
    } else if let Some(pid) = is_autopilot_running() {
        println!("\nAutopilot is running in foreground mode (PID {}).", pid);
        println!(
            "Stop it with Ctrl-C or `stakpak autopilot down`, then start again with `stakpak up`."
        );
        println!("Configuration has been validated and will take effect on next start.");
    } else {
        return Err("Autopilot is not running. Start it with `stakpak up`.".to_string());
    }

    Ok(())
}

async fn run_schedule_command(command: AutopilotScheduleCommands) -> Result<(), String> {
    match command {
        AutopilotScheduleCommands::List => list_schedules().await,
        AutopilotScheduleCommands::Add {
            name,
            cron,
            prompt,
            check,
            trigger_on,
            // workdir,
            max_steps,
            channel,
            pause_on_approval,
            sandbox,
            enabled,
        } => {
            let mut config = AutopilotConfigFile::load_or_default_async().await?;
            let schedule = AutopilotScheduleConfig {
                name: name.clone(),
                cron,
                prompt,
                check,
                trigger_on,
                // workdir,
                max_steps,
                channel,
                pause_on_approval,
                sandbox,
                enabled,
            };
            add_schedule_in_config(&mut config, schedule)?;
            config.save()?;

            let signaled = signal_scheduler_reload().await;
            print_schedule_mutation_feedback(&name, "added", signaled);
            Ok(())
        }
        AutopilotScheduleCommands::Remove { name } => {
            let mut config = AutopilotConfigFile::load_or_default_async().await?;
            remove_schedule_in_config(&mut config, &name)?;
            config.save()?;

            let signaled = signal_scheduler_reload().await;
            print_schedule_mutation_feedback(&name, "removed", signaled);
            Ok(())
        }
        AutopilotScheduleCommands::Enable { name } => {
            let mut config = AutopilotConfigFile::load_or_default_async().await?;
            set_schedule_enabled_in_config(&mut config, &name, true)?;
            config.save()?;

            let signaled = signal_scheduler_reload().await;
            print_schedule_mutation_feedback(&name, "enabled", signaled);
            Ok(())
        }
        AutopilotScheduleCommands::Disable { name } => {
            let mut config = AutopilotConfigFile::load_or_default_async().await?;
            set_schedule_enabled_in_config(&mut config, &name, false)?;
            config.save()?;

            let signaled = signal_scheduler_reload().await;
            print_schedule_mutation_feedback(&name, "disabled", signaled);
            Ok(())
        }
        AutopilotScheduleCommands::Trigger { name, dry_run } => {
            // Validate the schedule exists in config
            let config = AutopilotConfigFile::load_or_default_async().await?;
            if config.find_schedule(&name).is_none() {
                return Err(format!("Schedule '{}' not found", name));
            }
            // Delegate to the watch module's fire_schedule
            match crate::commands::watch::commands::schedule::fire_schedule(&name, dry_run).await {
                Ok(()) => Ok(()),
                Err(e) if e.contains("not found") || e.contains("not running") => Err(format!(
                    "Cannot trigger '{}': autopilot is not running. Start it with: stakpak up",
                    name
                )),
                Err(e) => Err(e),
            }
        }
        AutopilotScheduleCommands::History { name, limit } => {
            crate::commands::watch::commands::history::show_history(Some(&name), Some(limit)).await
        }
        AutopilotScheduleCommands::Show { id } => {
            crate::commands::watch::commands::history::show_run(id).await
        }
        AutopilotScheduleCommands::Clean { older_than_days } => {
            let config = crate::commands::watch::ScheduleConfig::load_default()
                .map_err(|e| format!("Failed to load watch config: {}", e))?;
            let db_path = config.db_path();
            let db_path_str = db_path
                .to_str()
                .ok_or_else(|| "Invalid database path".to_string())?;
            let db = crate::commands::watch::ScheduleDb::new(db_path_str)
                .await
                .map_err(|e| format!("Failed to open database: {}", e))?;

            // Clean stale running runs
            let cleaned = db
                .clean_stale_runs()
                .await
                .map_err(|e| format!("Failed to clean stale runs: {}", e))?;
            if cleaned > 0 {
                println!(
                    "✓ Marked {} stale run{} as failed",
                    cleaned,
                    if cleaned == 1 { "" } else { "s" }
                );
            } else {
                println!("No stale runs found");
            }

            // Optionally prune old history
            if let Some(days) = older_than_days {
                let pruned = db
                    .prune_runs(days)
                    .await
                    .map_err(|e| format!("Failed to prune runs: {}", e))?;
                if pruned > 0 {
                    println!(
                        "✓ Pruned {} run{} older than {} day{}",
                        pruned,
                        if pruned == 1 { "" } else { "s" },
                        days,
                        if days == 1 { "" } else { "s" }
                    );
                } else {
                    println!("No runs older than {} days to prune", days);
                }
            }

            Ok(())
        }
    }
}

fn require_non_empty_token(token: String, error_message: &str) -> Result<String, String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(error_message.to_string());
    }
    Ok(trimmed.to_string())
}

fn add_channel_with_optional_target(
    config_path: &Path,
    channel_type: ChannelType,
    token: Option<String>,
    bot_token: Option<String>,
    app_token: Option<String>,
    target: Option<String>,
) -> Result<Option<String>, String> {
    let had_target = target.is_some();
    let normalized_target = target
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if had_target && normalized_target.is_none() {
        return Err("Target cannot be empty".to_string());
    }

    let mut root = load_toml_root_table(config_path)?;

    {
        let channels = ensure_toml_table(&mut root, "channels");

        match &channel_type {
            ChannelType::Telegram => {
                let raw_token = token.or_else(|| std::env::var("TELEGRAM_BOT_TOKEN").ok()).ok_or(
                    "Telegram token required. Use --token or set TELEGRAM_BOT_TOKEN\n\n  To get a token: message @BotFather on Telegram → /newbot → copy the bot token",
                )?;
                let tok = require_non_empty_token(
                    raw_token,
                    "Telegram token cannot be empty. Use --token or set TELEGRAM_BOT_TOKEN",
                )?;

                let mut telegram = toml::value::Table::new();
                telegram.insert("token".to_string(), toml::Value::String(tok));
                telegram.insert("require_mention".to_string(), toml::Value::Boolean(false));
                channels.insert("telegram".to_string(), toml::Value::Table(telegram));
            }
            ChannelType::Discord => {
                let raw_token = token.or_else(|| std::env::var("DISCORD_BOT_TOKEN").ok()).ok_or(
                    "Discord token required. Use --token or set DISCORD_BOT_TOKEN\n\n  To get a token: https://discord.com/developers/applications → create app → Bot tab → copy token",
                )?;
                let tok = require_non_empty_token(
                    raw_token,
                    "Discord token cannot be empty. Use --token or set DISCORD_BOT_TOKEN",
                )?;

                let mut discord = toml::value::Table::new();
                discord.insert("token".to_string(), toml::Value::String(tok));
                discord.insert("guilds".to_string(), toml::Value::Array(Vec::new()));
                channels.insert("discord".to_string(), toml::Value::Table(discord));
            }
            ChannelType::Slack => {
                let raw_bot = bot_token.or_else(|| std::env::var("SLACK_BOT_TOKEN").ok()).ok_or(
                    "Slack bot token required. Use --bot-token or set SLACK_BOT_TOKEN\n\n  To get tokens: https://api.slack.com/apps → create app → enable Socket Mode\n  → OAuth & Permissions → Install to Workspace → copy Bot User OAuth Token (xoxb-...)",
                )?;
                let raw_app = app_token.or_else(|| std::env::var("SLACK_APP_TOKEN").ok()).ok_or(
                    "Slack app token required. Use --app-token or set SLACK_APP_TOKEN\n\n  To get tokens: https://api.slack.com/apps → your app → Socket Mode\n  → generate app-level token with connections:write scope (xapp-...)",
                )?;
                let bot = require_non_empty_token(
                    raw_bot,
                    "Slack bot token cannot be empty. Use --bot-token or set SLACK_BOT_TOKEN",
                )?;
                let app = require_non_empty_token(
                    raw_app,
                    "Slack app token cannot be empty. Use --app-token or set SLACK_APP_TOKEN",
                )?;

                let mut slack = toml::value::Table::new();
                slack.insert("bot_token".to_string(), toml::Value::String(bot));
                slack.insert("app_token".to_string(), toml::Value::String(app));
                channels.insert("slack".to_string(), toml::Value::Table(slack));
            }
            _ => return Err(format!("{:?} is not supported yet", channel_type)),
        }
    }

    if let Some(target_value) = normalized_target.as_deref() {
        apply_default_notification_target(&mut root, &channel_type.to_string(), target_value)?;
    }

    write_toml_root_table(config_path, root)?;

    Ok(normalized_target)
}

fn remove_channel(config_path: &Path, channel_type: ChannelType) -> Result<(), String> {
    let mut config = stakpak_gateway::GatewayConfig::load_unvalidated(
        config_path,
        &stakpak_gateway::GatewayCliFlags::default(),
    )
    .map_err(|e| format!("Failed to load config: {e}"))?;

    match &channel_type {
        ChannelType::Telegram => config.channels.telegram = None,
        ChannelType::Discord => config.channels.discord = None,
        ChannelType::Slack => config.channels.slack = None,
        _ => return Err(format!("{:?} is not supported yet", channel_type)),
    }

    config
        .save(config_path)
        .map_err(|e| format!("Failed to save config: {e}"))?;

    Ok(())
}

async fn run_channel_command(command: AutopilotChannelCommands) -> Result<(), String> {
    let config_path = AutopilotConfigFile::path();
    match command {
        AutopilotChannelCommands::List => {
            let config = load_gateway_config_allowing_no_channels(config_path.as_path())?;

            let channels = config.enabled_channels();
            if channels.is_empty() {
                println!("No channels configured.");
                println!(
                    "  Add one: stakpak autopilot channel add slack --bot-token X --app-token Y"
                );
                return Ok(());
            }

            println!("{:<15} STATUS", "CHANNEL");
            if config.channels.telegram.is_some() {
                println!("{:<15} configured", "telegram");
            }
            if config.channels.discord.is_some() {
                println!("{:<15} configured", "discord");
            }
            if config.channels.slack.is_some() {
                println!("{:<15} configured", "slack");
            }
            Ok(())
        }
        AutopilotChannelCommands::Add {
            channel_type,
            token,
            bot_token,
            app_token,
            target,
        } => {
            let saved_target = add_channel_with_optional_target(
                config_path.as_path(),
                channel_type.clone(),
                token,
                bot_token,
                app_token,
                target,
            )?;

            if let Some(target_value) = saved_target {
                println!(
                    "✓ Default notification target set for {}: {}",
                    channel_type, target_value
                );
            }

            println!("✓ Channel {} added", channel_type);
            Ok(())
        }
        AutopilotChannelCommands::Remove { channel_type } => {
            remove_channel(config_path.as_path(), channel_type.clone())?;
            println!("✓ Channel {} removed", channel_type);
            Ok(())
        }
        AutopilotChannelCommands::Test => {
            let config = stakpak_gateway::GatewayConfig::load(
                config_path.as_path(),
                &stakpak_gateway::GatewayCliFlags::default(),
            )
            .map_err(|e| format!("Failed to load config: {e}"))?;

            let channels = stakpak_gateway::build_channels(&config)
                .map_err(|e| format!("Failed to build channels: {e}"))?;

            if channels.is_empty() {
                return Err("No channels configured. Add one first: stakpak autopilot channel add slack --bot-token X --app-token Y".to_string());
            }

            for channel in channels.values() {
                match channel.test().await {
                    Ok(result) => println!(
                        "  ✓ {}: {} ({})",
                        result.channel, result.identity, result.details
                    ),
                    Err(error) => println!("  ✗ {}: {}", channel.display_name(), error),
                }
            }
            Ok(())
        }
    }
}

async fn list_schedules() -> Result<(), String> {
    let config = AutopilotConfigFile::load_or_default_async().await?;
    if config.schedules.is_empty() {
        println!("No schedules configured.");
        return Ok(());
    }

    println!(
        "{:<20} {:<16} {:<10} {:<24}",
        "NAME", "CRON", "STATUS", "NEXT RUN"
    );

    for schedule in &config.schedules {
        let next_run =
            next_run_for_cron(&schedule.cron, schedule.enabled).unwrap_or_else(|| "-".to_string());
        println!(
            "{:<20} {:<16} {:<10} {:<24}",
            truncate_text(&schedule.name, 20),
            truncate_text(&schedule.cron, 16),
            if schedule.enabled {
                "enabled"
            } else {
                "disabled"
            },
            truncate_text(&next_run, 24)
        );
    }

    Ok(())
}

fn validate_schedule(schedule: &AutopilotScheduleConfig) -> Result<(), String> {
    if schedule.name.trim().is_empty() {
        return Err("Schedule name cannot be empty".to_string());
    }

    if schedule.name.trim() == crate::commands::watch::RELOAD_SENTINEL {
        return Err(format!(
            "Schedule name '{}' is reserved",
            crate::commands::watch::RELOAD_SENTINEL
        ));
    }

    Cron::from_str(&schedule.cron)
        .map_err(|e| format!("Invalid cron expression '{}': {}", schedule.cron, e))?;

    if schedule.prompt.trim().is_empty() {
        return Err("Schedule prompt cannot be empty".to_string());
    }

    Ok(())
}

fn add_schedule_in_config(
    config: &mut AutopilotConfigFile,
    schedule: AutopilotScheduleConfig,
) -> Result<(), String> {
    validate_schedule(&schedule)?;

    if config.find_schedule(&schedule.name).is_some() {
        return Err(format!("Schedule '{}' already exists", schedule.name));
    }

    config.schedules.push(schedule);
    Ok(())
}

fn remove_schedule_in_config(config: &mut AutopilotConfigFile, name: &str) -> Result<(), String> {
    let initial_len = config.schedules.len();
    config.schedules.retain(|schedule| schedule.name != name);

    if config.schedules.len() == initial_len {
        return Err(format!("Schedule '{}' not found", name));
    }

    Ok(())
}

fn set_schedule_enabled_in_config(
    config: &mut AutopilotConfigFile,
    name: &str,
    enabled: bool,
) -> Result<(), String> {
    let schedule = config
        .find_schedule_mut(name)
        .ok_or_else(|| format!("Schedule '{}' not found", name))?;

    schedule.enabled = enabled;
    Ok(())
}

fn print_schedule_mutation_feedback(name: &str, action: &str, signaled: bool) {
    if is_autopilot_running().is_some() {
        if signaled {
            println!("✓ Schedule '{}' {} (takes effect within ~1s)", name, action);
        } else {
            println!("✓ Schedule '{}' {} (takes effect within ~5s)", name, action);
        }
    } else {
        println!(
            "✓ Schedule '{}' {} (takes effect when autopilot starts)",
            name, action
        );
    }
}

async fn signal_scheduler_reload() -> bool {
    let db_path = match autopilot_db_path() {
        Ok(path) => path,
        Err(_) => return false,
    };

    let db = match crate::commands::watch::ScheduleDb::new(&db_path).await {
        Ok(db) => db,
        Err(_) => return false,
    };

    db.request_config_reload().await.is_ok()
}

fn autopilot_db_path() -> Result<String, String> {
    let config = crate::commands::watch::ScheduleConfig::load_default()
        .map_err(|error| format!("Failed to load watch config: {}", error))?;
    let db_path = config.db_path();

    db_path
        .to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| "Invalid db path".to_string())
}

#[derive(Debug, Clone)]
struct NotificationDefaults {
    channel: String,
    chat_id: Option<String>,
}

fn load_notification_defaults(path: &Path) -> Result<NotificationDefaults, String> {
    let root = load_toml_root_table(path)?;
    let notifications = root
        .get("notifications")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "Notifications are not configured".to_string())?;

    let channel = notifications
        .get("channel")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "Notifications channel is not configured".to_string())?
        .to_string();

    let chat_id = notifications
        .get("chat_id")
        .and_then(toml::Value::as_str)
        .map(str::to_string);

    Ok(NotificationDefaults { channel, chat_id })
}

fn resolve_default_gateway_url(root: &toml::value::Table) -> String {
    root.get("server")
        .and_then(toml::Value::as_table)
        .and_then(|server| server.get("listen"))
        .and_then(toml::Value::as_str)
        .map(loopback_base_url_from_bind)
        .unwrap_or_else(|| "http://127.0.0.1:4096".to_string())
}

fn apply_default_notification_target(
    root: &mut toml::value::Table,
    channel: &str,
    target: &str,
) -> Result<(), String> {
    if channel.trim().is_empty() {
        return Err("Channel cannot be empty".to_string());
    }

    if target.trim().is_empty() {
        return Err("Target cannot be empty".to_string());
    }

    let default_gateway_url = resolve_default_gateway_url(root);

    let notifications = ensure_toml_table(root, "notifications");
    if !notifications.contains_key("gateway_url") {
        notifications.insert(
            "gateway_url".to_string(),
            toml::Value::String(default_gateway_url),
        );
    }
    notifications.insert(
        "channel".to_string(),
        toml::Value::String(channel.trim().to_string()),
    );
    notifications.insert(
        "chat_id".to_string(),
        toml::Value::String(target.trim().to_string()),
    );

    Ok(())
}

#[cfg(test)]
fn set_default_notification_target(path: &Path, channel: &str, target: &str) -> Result<(), String> {
    let mut root = load_toml_root_table(path)?;
    apply_default_notification_target(&mut root, channel, target)?;
    write_toml_root_table(path, root)
}

fn load_gateway_config_allowing_no_channels(
    config_path: &Path,
) -> Result<stakpak_gateway::GatewayConfig, String> {
    let cli_flags = stakpak_gateway::GatewayCliFlags::default();
    let config = stakpak_gateway::GatewayConfig::load_unvalidated(config_path, &cli_flags)
        .map_err(|e| format!("Failed to load channel config: {e}"))?;

    match config.validate_with_error() {
        Ok(()) | Err(stakpak_gateway::config::GatewayConfigValidationError::MissingChannels) => {
            Ok(config)
        }
        Err(error) => Err(format!("Channel config invalid: {error}")),
    }
}

fn gateway_channel_count(config_path: &Path) -> Result<usize, String> {
    let config = load_gateway_config_allowing_no_channels(config_path)?;
    Ok(config.enabled_channels().len())
}

async fn status_autopilot(
    config: &AppConfig,
    json: bool,
    recent_runs: Option<u32>,
) -> Result<(), String> {
    let autopilot_config = AutopilotConfigFile::load_or_default_async().await?;
    let server_config = autopilot_config.server.clone();
    let config_path = AutopilotConfigFile::path();
    let base_url = loopback_base_url_from_bind(&server_config.listen);
    let probe_client = build_probe_http_client();

    let schedules = build_schedule_statuses(&autopilot_config.schedules);
    let gateway_config = load_gateway_config_allowing_no_channels(config_path.as_path())?;
    let notification_defaults = load_notification_defaults(config_path.as_path()).ok();
    let channels = build_channel_statuses(&gateway_config, notification_defaults.as_ref());

    let service_path = autopilot_service_path();
    let service = ServiceStatusJson {
        installed: autopilot_service_installed(),
        active: autopilot_service_active(),
        path: service_path.display().to_string(),
    };

    let server_url = format!("{}/v1/health", base_url);
    let server_reachable = if let Some(client) = probe_client.as_ref() {
        endpoint_ok(client, &server_url).await
    } else {
        false
    };
    let server = EndpointStatusJson {
        expected_enabled: true,
        reachable: server_reachable,
        url: server_url,
    };

    let gateway_url = format!("{}/v1/gateway/status", base_url);
    let gateway_reachable = if let Some(client) = probe_client.as_ref() {
        endpoint_ok(client, &gateway_url).await
    } else {
        false
    };
    let gateway = EndpointStatusJson {
        expected_enabled: true,
        reachable: gateway_reachable,
        url: gateway_url,
    };

    let scheduler = collect_scheduler_status(recent_runs).await;

    if json {
        print_json(&AutopilotStatusJson {
            command: "autopilot.status",
            ok: true,
            profile: config.profile_name.clone(),
            config_path: config_path.display().to_string(),
            server_config: server_config.clone(),
            service,
            server,
            gateway,
            scheduler,
            schedules,
            channels,
        })?;
        return Ok(());
    }

    println!("Autopilot status");
    println!();
    println!("  Profile         {}", config.profile_name);
    println!("  Config          {}", config_path.display());
    println!(
        "  Service         {}",
        if service.installed {
            if service.active {
                "active"
            } else {
                "installed (inactive)"
            }
        } else {
            "not installed"
        }
    );
    println!(
        "  Server          {}",
        if server.reachable {
            format!("✓ reachable ({})", server.url)
        } else {
            format!("✗ unreachable ({})", server.url)
        }
    );
    println!(
        "  Channels        {}",
        if gateway.reachable {
            format!("✓ reachable ({})", gateway.url)
        } else {
            format!("✗ unreachable ({})", gateway.url)
        }
    );

    // Scheduler status
    let config_exists = AutopilotConfigFile::path().exists();
    if !config_exists {
        println!("  Scheduler       not configured (run: stakpak up)");
    } else if scheduler.config_valid {
        let sched_state = if scheduler.running {
            format!("✓ running (pid {})", scheduler.pid.unwrap_or_default())
        } else if scheduler.stale_pid {
            format!("⚠ stale (pid {})", scheduler.pid.unwrap_or_default())
        } else {
            "stopped".to_string()
        };
        println!(
            "  Scheduler       {} — {} schedules",
            sched_state, scheduler.trigger_count
        );
    } else {
        println!(
            "  Scheduler       ✗ config error: {}",
            scheduler.error.as_deref().unwrap_or("unknown")
        );
    }

    // Schedules table
    if !schedules.is_empty() {
        println!();
        println!("  Schedules:");
        println!(
            "    {:<20} {:<16} {:<10} {:<20}",
            "NAME", "CRON", "STATUS", "NEXT RUN"
        );
        for schedule in &schedules {
            println!(
                "    {:<20} {:<16} {:<10} {:<20}",
                truncate_text(&schedule.name, 20),
                truncate_text(&schedule.cron, 16),
                if schedule.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                schedule.next_run.as_deref().unwrap_or("-")
            );
        }
    }

    // Channels table
    if !channels.is_empty() {
        println!();
        println!("  Channels:");
        println!(
            "    {:<20} {:<10} {:<24} {:<10}",
            "NAME", "TYPE", "TARGET", "STATUS"
        );
        for channel in &channels {
            println!(
                "    {:<20} {:<10} {:<24} {:<10}",
                truncate_text(&channel.name, 20),
                truncate_text(&channel.channel_type, 10),
                truncate_text(&channel.target, 24),
                if channel.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
    }

    // Recent runs
    if !scheduler.recent_runs.is_empty() {
        println!();
        println!("  Recent runs:");
        for run in &scheduler.recent_runs {
            println!(
                "    #{} {:<16} {:<10} {}",
                run.id, run.schedule_name, run.status, run.started_at
            );
        }
    }

    Ok(())
}

fn tail_log_files(files: &[PathBuf], follow: bool, lines: Option<u32>) -> Result<(), String> {
    let mut cmd = std::process::Command::new("tail");
    if follow {
        cmd.arg("-f");
    }
    if let Some(n) = lines {
        cmd.arg("-n").arg(n.to_string());
    }
    for file in files {
        cmd.arg(file);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("Failed to read autopilot logs: {}", e))?;
    if !status.success() {
        return Err("Failed to read autopilot logs".to_string());
    }
    Ok(())
}

async fn logs_autopilot(
    follow: bool,
    lines: Option<u32>,
    component: Option<String>,
) -> Result<(), String> {
    let log_dir = autopilot_log_dir();

    // Resolve which log files to show
    let log_files: Vec<PathBuf> = if let Some(ref name) = component {
        let file = log_dir.join(format!("{}.log", name));
        if !file.exists() {
            return Err(format!(
                "Component log file not found: {}\nAutopilot may not have run yet.",
                file.display()
            ));
        }
        vec![file]
    } else {
        // Show all component logs plus legacy stdout/stderr
        [
            "scheduler.log",
            "server.log",
            "gateway.log",
            "stdout.log",
            "stderr.log",
        ]
        .iter()
        .map(|f| log_dir.join(f))
        .filter(|p| p.exists())
        .collect()
    };

    if log_files.is_empty() {
        return Err(format!(
            "No log files found in {}.\nAutopilot may not have run yet.",
            log_dir.display()
        ));
    }

    match detect_platform() {
        Platform::Linux => {
            // If a component filter is set, use tail on the specific file instead of journalctl
            if component.is_some() {
                tail_log_files(&log_files, follow, lines)?;
            } else {
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
        }
        Platform::MacOS => {
            tail_log_files(&log_files, follow, lines)?;
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

    let autopilot_config = match AutopilotConfigFile::load_or_default() {
        Ok(cfg) => {
            println!("✓ Autopilot config loaded (listen={})", cfg.server.listen);
            cfg
        }
        Err(e) => {
            failures += 1;
            println!("✗ Autopilot config invalid: {}", e);
            AutopilotConfigFile::default()
        }
    };
    let _ = &autopilot_config; // used below for schedule count

    let gateway_path = AutopilotConfigFile::path();
    match load_gateway_config_allowing_no_channels(gateway_path.as_path()) {
        Ok(cfg) => {
            let channels = cfg.enabled_channels();
            if channels.is_empty() {
                println!("✓ No channels configured (add with: stakpak autopilot channel add)");
            } else {
                println!("✓ Channel config valid (channels: {})", channels.join(", "));
            }
        }
        Err(e) => {
            failures += 1;
            println!("✗ Channel config invalid: {}", e);
        }
    }

    let scheduler_status = collect_scheduler_status(None).await;
    if scheduler_status.config_valid {
        if scheduler_status.trigger_count == 0 {
            println!("✓ No schedules configured (edit ~/.stakpak/autopilot.toml to add)");
        } else {
            println!(
                "✓ Schedule config valid ({} schedules)",
                scheduler_status.trigger_count
            );
        }
    } else {
        failures += 1;
        println!(
            "✗ Schedule config invalid: {}",
            scheduler_status
                .error
                .unwrap_or_else(|| "unknown configuration error".to_string())
        );
    }

    if autopilot_service_installed() {
        println!("✓ Autopilot service installed");
    } else {
        failures += 1;
        println!("✗ Autopilot service not installed");
    }

    let base_url = loopback_base_url_from_bind(&autopilot_config.server.listen);
    let server_health_url = format!("{}/v1/health", base_url);
    let probe_client = build_probe_http_client();
    let server_reachable = if let Some(client) = probe_client.as_ref() {
        endpoint_ok(client, &server_health_url).await
    } else {
        false
    };

    if server_reachable {
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

async fn write_default_autopilot_config(path: &Path, force: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create autopilot config directory: {}", e))?;
    }

    if force || !path.exists() {
        let default_config = AutopilotConfigFile::default();
        let content = toml::to_string_pretty(&default_config)
            .map_err(|e| format!("Failed to serialize default autopilot config: {}", e))?;

        let header = "# Stakpak Autopilot Configuration\n\
                      # Add schedules:  stakpak autopilot schedule add <name> --cron '...' --prompt '...'\n\
                      # Add channels:   stakpak autopilot channel add slack --bot-token X --app-token Y\n\
                      # Start:          stakpak up\n\n";

        tokio::fs::write(path, format!("{}{}", header, content))
            .await
            .map_err(|e| format!("Failed to write autopilot config: {}", e))?;
    }

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

async fn collect_scheduler_status(recent_runs: Option<u32>) -> SchedulerStatusJson {
    let config_path = AutopilotConfigFile::path();

    let schedule_count = AutopilotConfigFile::load_or_default()
        .map(|c| c.schedules.len())
        .unwrap_or(0);

    let config_valid = config_path.exists();

    // Watch runtime uses ~/.stakpak/autopilot/autopilot.db regardless of config format
    let db_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".stakpak/autopilot/autopilot.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    let db = match db_path.to_str() {
        Some(path) => match crate::commands::watch::ScheduleDb::new(path).await {
            Ok(db) => db,
            Err(error) => {
                return SchedulerStatusJson {
                    expected_enabled: true,
                    config_path: config_path.display().to_string(),
                    config_valid,
                    trigger_count: schedule_count,
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
            return SchedulerStatusJson {
                expected_enabled: true,
                config_path: config_path.display().to_string(),
                config_valid,
                trigger_count: schedule_count,
                running: false,
                pid: None,
                stale_pid: false,
                db_path: Some(db_path_str),
                error: Some("Invalid scheduler database path".to_string()),
                recent_runs: Vec::new(),
            };
        }
    };

    let scheduler_state = db.get_autopilot_state().await.ok().flatten();

    let (running, stale_pid, pid) = if let Some(state) = scheduler_state {
        let pid = state.pid;
        let running = u32::try_from(pid)
            .ok()
            .map(crate::commands::watch::is_process_running)
            .unwrap_or(false);
        (running, !running, Some(pid))
    } else {
        (false, false, None)
    };

    let recent_runs = if let Some(limit) = recent_runs.filter(|limit| *limit > 0) {
        match db
            .list_runs(&crate::commands::watch::ListRunsFilter {
                schedule_name: None,
                status: None,
                limit: Some(limit),
                offset: None,
            })
            .await
        {
            Ok(runs) => runs
                .into_iter()
                .map(|run| ScheduleRunSummaryJson {
                    id: run.id,
                    schedule_name: run.schedule_name,
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

    SchedulerStatusJson {
        expected_enabled: true,
        config_path: config_path.display().to_string(),
        config_valid,
        trigger_count: schedule_count,
        running,
        pid,
        stale_pid,
        db_path: Some(db_path_str),
        error: None,
        recent_runs,
    }
}

fn build_schedule_statuses(
    schedules: &[AutopilotScheduleConfig],
) -> Vec<AutopilotScheduleStatusJson> {
    schedules
        .iter()
        .map(|schedule| AutopilotScheduleStatusJson {
            name: schedule.name.clone(),
            cron: schedule.cron.clone(),
            enabled: schedule.enabled,
            next_run: next_run_for_cron(&schedule.cron, schedule.enabled),
        })
        .collect()
}

fn build_channel_statuses(
    gateway_config: &stakpak_gateway::GatewayConfig,
    notification_defaults: Option<&NotificationDefaults>,
) -> Vec<AutopilotChannelStatusJson> {
    let mut channels = Vec::new();

    if gateway_config.channels.telegram.is_some() {
        channels.push(build_single_channel_status(
            "telegram",
            notification_defaults,
        ));
    }

    if gateway_config.channels.discord.is_some() {
        channels.push(build_single_channel_status(
            "discord",
            notification_defaults,
        ));
    }

    if gateway_config.channels.slack.is_some() {
        channels.push(build_single_channel_status("slack", notification_defaults));
    }

    channels
}

fn build_single_channel_status(
    channel_name: &str,
    notification_defaults: Option<&NotificationDefaults>,
) -> AutopilotChannelStatusJson {
    let target = notification_defaults
        .filter(|defaults| defaults.channel == channel_name)
        .and_then(|defaults| defaults.chat_id.clone())
        .unwrap_or_else(|| "-".to_string());

    AutopilotChannelStatusJson {
        name: channel_name.to_string(),
        channel_type: channel_name.to_string(),
        target,
        enabled: true,
        alerts_only: false,
    }
}

fn next_run_for_cron(cron: &str, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }

    let expression = Cron::from_str(cron).ok()?;
    let next = expression.find_next_occurrence(&Utc::now(), false).ok()?;
    Some(next.format("%Y-%m-%d %H:%M").to_string())
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
fn bounded_history_limit(limit: u32) -> u32 {
    limit.clamp(1, 1000)
}

fn build_probe_http_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()
}

async fn endpoint_ok(client: &reqwest::Client, url: &str) -> bool {
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

/// Check if the autopilot process is currently running via PID file + process check.
fn is_autopilot_running() -> Option<u32> {
    let config = crate::commands::watch::ScheduleConfig::load_default().ok()?;
    let pid_file = config
        .db_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("autopilot.pid");
    let pid_str = std::fs::read_to_string(&pid_file).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;
    if crate::commands::watch::is_process_running(pid) {
        Some(pid)
    } else {
        // Stale PID file — clean it up
        let _ = std::fs::remove_file(&pid_file);
        None
    }
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
    if !config.profile_name.is_empty() {
        exec_parts.push("--profile".to_string());
        exec_parts.push(config.profile_name.clone());
    }
    if !config.config_path.is_empty() {
        exec_parts.push("--config".to_string());
        exec_parts.push(config.config_path.clone());
    }
    exec_parts.extend([
        "autopilot".to_string(),
        "up".to_string(),
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
    if !config.profile_name.is_empty() {
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
        "<string>up</string>".to_string(),
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
    fn config_roundtrip_save_load() {
        let path = temp_file_path("autopilot-config");

        let mut config = AutopilotConfigFile::default();
        config.server.listen = "0.0.0.0:4111".to_string();
        config.server.show_token = true;
        config.server.no_auth = true;
        config.server.model = Some("anthropic/claude-sonnet-4-5".to_string());
        config.server.auto_approve_all = true;

        let save_result = config.save_to_path(&path);
        assert!(save_result.is_ok());

        let loaded = AutopilotConfigFile::load_from_path(&path);
        assert!(loaded.is_ok());

        if let Ok(loaded) = loaded {
            assert_eq!(loaded.server.listen, "0.0.0.0:4111");
            assert!(loaded.server.show_token);
            assert!(loaded.server.no_auth);
            assert_eq!(
                loaded.server.model.as_deref(),
                Some("anthropic/claude-sonnet-4-5")
            );
            assert!(loaded.server.auto_approve_all);
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

    fn sample_schedule(name: &str) -> AutopilotScheduleConfig {
        AutopilotScheduleConfig {
            name: name.to_string(),
            cron: "*/5 * * * *".to_string(),
            prompt: "Check infra".to_string(),
            check: None,
            trigger_on: ScheduleTriggerOn::Failure,
            // workdir: None,
            max_steps: 50,
            channel: None,
            pause_on_approval: false,
            sandbox: false,
            enabled: true,
        }
    }

    #[test]
    fn schedule_add_remove_enable_disable_happy_path() {
        let mut config = AutopilotConfigFile::default();

        let add_result = add_schedule_in_config(&mut config, sample_schedule("health-check"));
        assert!(add_result.is_ok());
        assert_eq!(config.schedules.len(), 1);

        let disable_result = set_schedule_enabled_in_config(&mut config, "health-check", false);
        assert!(disable_result.is_ok());
        assert!(!config.schedules[0].enabled);

        let enable_result = set_schedule_enabled_in_config(&mut config, "health-check", true);
        assert!(enable_result.is_ok());
        assert!(config.schedules[0].enabled);

        let remove_result = remove_schedule_in_config(&mut config, "health-check");
        assert!(remove_result.is_ok());
        assert!(config.schedules.is_empty());
    }

    #[test]
    fn schedule_duplicate_name_rejected() {
        let mut config = AutopilotConfigFile::default();

        let first = add_schedule_in_config(&mut config, sample_schedule("drift-detect"));
        assert!(first.is_ok());

        let duplicate = add_schedule_in_config(&mut config, sample_schedule("drift-detect"));
        assert!(duplicate.is_err());
    }

    #[test]
    fn schedule_invalid_cron_rejected() {
        let mut config = AutopilotConfigFile::default();
        let mut schedule = sample_schedule("broken");
        schedule.cron = "invalid cron".to_string();

        let result = add_schedule_in_config(&mut config, schedule);
        assert!(result.is_err());
    }

    #[test]
    fn schedule_reserved_name_rejected() {
        let mut config = AutopilotConfigFile::default();
        let schedule = sample_schedule(crate::commands::watch::RELOAD_SENTINEL);

        let result = add_schedule_in_config(&mut config, schedule);
        assert!(result.is_err());
        let message = result.expect_err("reserved schedule name should be rejected");
        assert!(message.contains("reserved"));
    }

    #[test]
    fn history_limit_is_bounded() {
        assert_eq!(bounded_history_limit(0), 1);
        assert_eq!(bounded_history_limit(20), 20);
        assert_eq!(bounded_history_limit(10_000), 1000);
    }

    #[test]
    fn load_ignores_gateway_channel_schema() {
        let path = temp_file_path("autopilot-gateway-channels");
        let write_result = std::fs::write(
            &path,
            r##"
[server]
listen = "127.0.0.1:4096"

[channels.slack]
bot_token = "xoxb-test"
app_token = "xapp-test"
"##,
        );
        assert!(write_result.is_ok());

        let loaded = AutopilotConfigFile::load_from_path(&path);
        assert!(loaded.is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn server_config_save_preserves_gateway_and_notifications_sections() {
        let path = temp_file_path("autopilot-preserve");
        let write_result = std::fs::write(
            &path,
            r##"
[server]
listen = "127.0.0.1:4096"
url = "http://127.0.0.1:4096"
token = "gateway-token"

[notifications]
gateway_url = "http://127.0.0.1:4096"
channel = "slack"
chat_id = "#infra"

[channels.slack]
bot_token = "xoxb-old"
app_token = "xapp-old"
"##,
        );
        assert!(write_result.is_ok());

        let load_result = AutopilotConfigFile::load_from_path(&path);
        assert!(load_result.is_ok());
        let mut loaded = match load_result {
            Ok(value) => value,
            Err(error) => panic!("failed to load config: {error}"),
        };

        loaded.server.auto_approve_all = true;
        let save_updated = loaded.save_to_path(&path);
        assert!(save_updated.is_ok());

        let reloaded = std::fs::read_to_string(&path);
        assert!(reloaded.is_ok());
        let reloaded = match reloaded {
            Ok(value) => value,
            Err(error) => panic!("failed to read config: {error}"),
        };

        assert!(reloaded.contains("[channels.slack]"));
        assert!(reloaded.contains("bot_token = \"xoxb-old\""));
        assert!(reloaded.contains("[notifications]"));
        assert!(reloaded.contains("channel = \"slack\""));
        assert!(reloaded.contains("chat_id = \"#infra\""));
        assert!(reloaded.contains("auto_approve_all = true"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn set_default_notification_target_merges_without_overwrite() {
        let path = temp_file_path("autopilot-notification-target");
        let write_result = std::fs::write(
            &path,
            r##"
[server]
listen = "127.0.0.1:4096"

[[schedules]]
name = "health-check"
cron = "*/5 * * * *"
prompt = "Check system health"

[channels.slack]
bot_token = "xoxb-test"
app_token = "xapp-test"
"##,
        );
        assert!(write_result.is_ok());

        let set_result = set_default_notification_target(path.as_path(), "slack", "#ops");
        assert!(set_result.is_ok());

        let reloaded = std::fs::read_to_string(&path);
        assert!(reloaded.is_ok());
        let reloaded = match reloaded {
            Ok(value) => value,
            Err(error) => panic!("failed to read config: {error}"),
        };

        assert!(reloaded.contains("[[schedules]]"));
        assert!(reloaded.contains("[channels.slack]"));
        assert!(reloaded.contains("[notifications]"));
        assert!(reloaded.contains("channel = \"slack\""));
        assert!(reloaded.contains("chat_id = \"#ops\""));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn channel_add_with_target_updates_notifications() {
        let path = temp_file_path("autopilot-channel-add-target");
        let write_result = std::fs::write(
            &path,
            r##"
[server]
listen = "127.0.0.1:4096"

[[schedules]]
name = "health-check"
cron = "*/5 * * * *"
prompt = "Check system health"
"##,
        );
        assert!(write_result.is_ok());

        let add_result = add_channel_with_optional_target(
            path.as_path(),
            ChannelType::Slack,
            None,
            Some("xoxb-test".to_string()),
            Some("xapp-test".to_string()),
            Some("#eng".to_string()),
        );
        assert!(add_result.is_ok());
        assert_eq!(add_result.ok(), Some(Some("#eng".to_string())));

        let reloaded = std::fs::read_to_string(&path);
        assert!(reloaded.is_ok());
        let reloaded = match reloaded {
            Ok(value) => value,
            Err(error) => panic!("failed to read config: {error}"),
        };

        assert!(reloaded.contains("[channels.slack]"));
        assert!(reloaded.contains("bot_token = \"xoxb-test\""));
        assert!(reloaded.contains("app_token = \"xapp-test\""));
        assert!(reloaded.contains("[notifications]"));
        assert!(reloaded.contains("channel = \"slack\""));
        assert!(reloaded.contains("chat_id = \"#eng\""));
        assert!(reloaded.contains("[[schedules]]"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn channel_add_with_invalid_target_is_atomic() {
        let path = temp_file_path("autopilot-channel-add-invalid-target");
        let write_result = std::fs::write(
            &path,
            r##"
[server]
listen = "127.0.0.1:4096"

[[schedules]]
name = "health-check"
cron = "*/5 * * * *"
prompt = "Check system health"
"##,
        );
        assert!(write_result.is_ok());

        let add_result = add_channel_with_optional_target(
            path.as_path(),
            ChannelType::Slack,
            None,
            Some("xoxb-test".to_string()),
            Some("xapp-test".to_string()),
            Some("   ".to_string()),
        );
        assert!(add_result.is_err());

        let reloaded = std::fs::read_to_string(&path);
        assert!(reloaded.is_ok());
        let reloaded = match reloaded {
            Ok(value) => value,
            Err(error) => panic!("failed to read config: {error}"),
        };

        assert!(!reloaded.contains("[channels.slack]"));
        assert!(!reloaded.contains("[notifications]"));
        assert!(reloaded.contains("[[schedules]]"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn channel_add_rejects_empty_tokens() {
        let path = temp_file_path("autopilot-channel-add-empty-token");

        let empty_telegram_result = add_channel_with_optional_target(
            path.as_path(),
            ChannelType::Telegram,
            Some("   ".to_string()),
            None,
            None,
            None,
        );
        assert!(empty_telegram_result.is_err());

        let empty_discord_result = add_channel_with_optional_target(
            path.as_path(),
            ChannelType::Discord,
            Some("   ".to_string()),
            None,
            None,
            None,
        );
        assert!(empty_discord_result.is_err());

        let empty_bot_result = add_channel_with_optional_target(
            path.as_path(),
            ChannelType::Slack,
            None,
            Some("   ".to_string()),
            Some("xapp-test".to_string()),
            None,
        );
        assert!(empty_bot_result.is_err());

        let empty_app_result = add_channel_with_optional_target(
            path.as_path(),
            ChannelType::Slack,
            None,
            Some("xoxb-test".to_string()),
            Some("   ".to_string()),
            None,
        );
        assert!(empty_app_result.is_err());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn channel_remove_recovers_from_invalid_channel_config() {
        let path = temp_file_path("autopilot-channel-remove-invalid");
        let write_result = std::fs::write(
            &path,
            r##"
[channels.slack]
bot_token = ""
app_token = "xapp-test"
"##,
        );
        assert!(write_result.is_ok());

        let remove_result = remove_channel(path.as_path(), ChannelType::Slack);
        assert!(remove_result.is_ok());

        let reloaded = std::fs::read_to_string(&path);
        assert!(reloaded.is_ok());
        let reloaded = match reloaded {
            Ok(value) => value,
            Err(error) => panic!("failed to read config: {error}"),
        };
        assert!(!reloaded.contains("[channels.slack]"));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn schedule_add_writes_to_config() {
        // Schedule add now works — it writes to the config file.
        // We can't easily test it here without a temp config path,
        // so just verify the helper functions work correctly.
        let mut config = AutopilotConfigFile::default();
        let schedule = AutopilotScheduleConfig {
            name: "demo".to_string(),
            cron: "*/5 * * * *".to_string(),
            prompt: "hello".to_string(),
            check: None,
            trigger_on: ScheduleTriggerOn::Failure,
            // workdir: None,
            max_steps: 50,
            channel: None,
            pause_on_approval: false,
            sandbox: false,
            enabled: true,
        };
        let result = add_schedule_in_config(&mut config, schedule);
        assert!(result.is_ok());
        assert!(config.find_schedule("demo").is_some());
    }

    #[test]
    fn gateway_channel_count_surfaces_invalid_channel_config() {
        let path = temp_file_path("autopilot-invalid-gateway-channel");
        let write_result = std::fs::write(
            &path,
            r##"
[channels.slack]
bot_token = ""
app_token = "xapp-test"
"##,
        );
        assert!(write_result.is_ok());

        let count_result = gateway_channel_count(path.as_path());
        assert!(count_result.is_err());

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn channel_add_requires_token() {
        // Channel add without token should fail with a helpful message
        let result = run_channel_command(AutopilotChannelCommands::Add {
            channel_type: ChannelType::Telegram,
            token: None,
            bot_token: None,
            app_token: None,
            target: None,
        })
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Telegram token required"));
    }

    #[test]
    fn status_json_schema_contains_core_fields() {
        let payload = AutopilotStatusJson {
            command: "autopilot.status",
            ok: true,
            profile: "default".to_string(),
            config_path: "/tmp/autopilot.toml".to_string(),
            server_config: AutopilotServerConfig::default(),
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
            scheduler: SchedulerStatusJson {
                expected_enabled: true,
                config_path: "/tmp/autopilot.toml".to_string(),
                config_valid: true,
                trigger_count: 2,
                running: true,
                pid: Some(123),
                stale_pid: false,
                db_path: Some("/tmp/autopilot.db".to_string()),
                error: None,
                recent_runs: vec![ScheduleRunSummaryJson {
                    id: 1,
                    schedule_name: "example".to_string(),
                    status: "completed".to_string(),
                    started_at: "2026-01-01T00:00:00Z".to_string(),
                    finished_at: Some("2026-01-01T00:00:10Z".to_string()),
                    error_message: None,
                }],
            },
            schedules: vec![AutopilotScheduleStatusJson {
                name: "health-check".to_string(),
                cron: "*/5 * * * *".to_string(),
                enabled: true,
                next_run: Some("2026-01-01 00:05".to_string()),
            }],
            channels: vec![AutopilotChannelStatusJson {
                name: "slack".to_string(),
                channel_type: "slack".to_string(),
                target: "#infra".to_string(),
                enabled: true,
                alerts_only: false,
            }],
        };

        let json = serde_json::to_value(payload);
        assert!(json.is_ok());

        if let Ok(value) = json {
            assert_eq!(
                value.get("command").and_then(|v| v.as_str()),
                Some("autopilot.status")
            );
            assert!(value.get("server_config").is_some());
            assert!(value.get("service").is_some());
            assert!(value.get("server").is_some());
            assert!(value.get("gateway").is_some());
            assert!(value.get("scheduler").is_some());
            assert!(value.get("schedules").is_some());
            assert!(value.get("channels").is_some());

            let scheduler_runs = value
                .get("scheduler")
                .and_then(|s| s.get("recent_runs"))
                .and_then(|runs| runs.as_array())
                .map(|runs| runs.len())
                .unwrap_or_default();
            assert_eq!(scheduler_runs, 1);
        }
    }
}
