use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::router::{Binding, BindingMatch, DmScope, PeerMatch, PeerMatchKind, RouterConfig};

#[derive(Debug, Clone, Default)]
pub struct GatewayCliFlags {
    pub url: Option<String>,
    pub token: Option<String>,
    pub telegram_token: Option<String>,
    pub discord_token: Option<String>,
    pub slack_bot_token: Option<String>,
    pub slack_app_token: Option<String>,
    pub store: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub server: ServerConfig,
    pub gateway: GatewaySettings,
    pub routing: RoutingConfig,
    pub channels: ChannelConfigs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewaySettings {
    pub store_path: PathBuf,
    pub model: Option<String>,
    pub title_template: String,
    pub prune_after_hours: u64,
    pub delivery_context_ttl_hours: u64,
    pub approval_mode: ApprovalMode,
    pub approval_allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    #[default]
    AllowAll,
    DenyAll,
    Allowlist,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GatewayConfigValidationError {
    #[error("at least one channel must be configured")]
    MissingChannels,
    #[error("telegram token cannot be empty")]
    EmptyTelegramToken,
    #[error("discord token cannot be empty")]
    EmptyDiscordToken,
    #[error("slack bot_token cannot be empty")]
    EmptySlackBotToken,
    #[error("slack app_token cannot be empty")]
    EmptySlackAppToken,
    #[error("approval_mode=allowlist requires non-empty approval_allowlist")]
    EmptyApprovalAllowlist,
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            store_path: default_store_path(),
            model: None,
            title_template: "{channel} / {peer}".to_string(),
            prune_after_hours: 168,
            delivery_context_ttl_hours: 4,
            approval_mode: ApprovalMode::AllowAll,
            approval_allowlist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    pub dm_scope: DmScopeSerde,
    pub bindings: Vec<BindingConfig>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            dm_scope: DmScopeSerde::PerChannelPeer,
            bindings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DmScopeSerde {
    Main,
    PerPeer,
    #[default]
    PerChannelPeer,
}

impl From<DmScopeSerde> for DmScope {
    fn from(value: DmScopeSerde) -> Self {
        match value {
            DmScopeSerde::Main => DmScope::Main,
            DmScopeSerde::PerPeer => DmScope::PerPeer,
            DmScopeSerde::PerChannelPeer => DmScope::PerChannelPeer,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingConfig {
    pub channel: String,
    pub routing_key: String,
    #[serde(default)]
    pub direct: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelConfigs {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub token: String,
    #[serde(default)]
    pub require_mention: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub token: String,
    #[serde(default)]
    pub guilds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                url: "http://127.0.0.1:4096".to_string(),
                token: String::new(),
            },
            gateway: GatewaySettings::default(),
            routing: RoutingConfig::default(),
            channels: ChannelConfigs::default(),
        }
    }
}

impl GatewayConfig {
    pub fn load(config_path: &Path, cli: &GatewayCliFlags) -> Result<Self> {
        let config = Self::load_unvalidated(config_path, cli)?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_unvalidated(config_path: &Path, cli: &GatewayCliFlags) -> Result<Self> {
        let mut config = if config_path.exists() {
            let text = std::fs::read_to_string(config_path).map_err(|error| {
                anyhow!(
                    "failed to read gateway config {}: {error}",
                    config_path.display()
                )
            })?;
            let persisted: PersistedGatewayConfig = toml::from_str(&text).map_err(|error| {
                anyhow!(
                    "failed to parse gateway config {}: {error}",
                    config_path.display()
                )
            })?;
            persisted.into_runtime()
        } else {
            GatewayConfig::default()
        };

        config.apply_env_overrides();
        config.apply_cli_overrides(cli);
        config.normalize_paths();

        Ok(config)
    }

    pub fn save(&self, config_path: &Path) -> Result<()> {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                anyhow!("failed to create config dir {}: {error}", parent.display())
            })?;
        }

        let mut root = load_toml_root_table(config_path)?;

        {
            let server = ensure_subtable(&mut root, "server");
            server.insert(
                "url".to_string(),
                toml::Value::String(self.server.url.clone()),
            );
            server.insert(
                "token".to_string(),
                toml::Value::String(self.server.token.clone()),
            );
        }

        {
            let gateway = ensure_subtable(&mut root, "gateway");
            gateway.insert(
                "store".to_string(),
                toml::Value::String(self.gateway.store_path.to_string_lossy().to_string()),
            );
            match &self.gateway.model {
                Some(model) => {
                    gateway.insert("model".to_string(), toml::Value::String(model.clone()));
                }
                None => {
                    gateway.remove("model");
                }
            }
            gateway.insert(
                "title_template".to_string(),
                toml::Value::String(self.gateway.title_template.clone()),
            );
            gateway.insert(
                "prune_after_hours".to_string(),
                toml::Value::Integer(
                    i64::try_from(self.gateway.prune_after_hours)
                        .map_err(|_| anyhow!("prune_after_hours exceeds i64 range"))?,
                ),
            );
            gateway.insert(
                "delivery_context_ttl_hours".to_string(),
                toml::Value::Integer(
                    i64::try_from(self.gateway.delivery_context_ttl_hours)
                        .map_err(|_| anyhow!("delivery_context_ttl_hours exceeds i64 range"))?,
                ),
            );
            gateway.insert(
                "approval_mode".to_string(),
                toml::Value::try_from(&self.gateway.approval_mode)
                    .map_err(|error| anyhow!("failed to serialize approval_mode: {error}"))?,
            );
            gateway.insert(
                "approval_allowlist".to_string(),
                toml::Value::Array(
                    self.gateway
                        .approval_allowlist
                        .iter()
                        .cloned()
                        .map(toml::Value::String)
                        .collect(),
                ),
            );
        }

        {
            let routing = ensure_subtable(&mut root, "routing");
            routing.insert(
                "dm_scope".to_string(),
                toml::Value::try_from(&self.routing.dm_scope)
                    .map_err(|error| anyhow!("failed to serialize dm_scope: {error}"))?,
            );
            routing.insert(
                "bindings".to_string(),
                toml::Value::try_from(&self.routing.bindings)
                    .map_err(|error| anyhow!("failed to serialize bindings: {error}"))?,
            );
        }

        {
            let channels = ensure_subtable(&mut root, "channels");
            upsert_optional_subtable(channels, "telegram", &self.channels.telegram)?;
            upsert_optional_subtable(channels, "discord", &self.channels.discord)?;
            upsert_optional_subtable(channels, "slack", &self.channels.slack)?;
        }

        let text = toml::to_string_pretty(&toml::Value::Table(root))
            .map_err(|error| anyhow!("failed to serialize gateway config: {error}"))?;

        std::fs::write(config_path, text).map_err(|error| {
            anyhow!(
                "failed to write gateway config {}: {error}",
                config_path.display()
            )
        })?;

        Ok(())
    }

    pub fn validate_with_error(&self) -> std::result::Result<(), GatewayConfigValidationError> {
        if self.enabled_channels().is_empty() {
            return Err(GatewayConfigValidationError::MissingChannels);
        }

        if let Some(telegram) = &self.channels.telegram
            && telegram.token.trim().is_empty()
        {
            return Err(GatewayConfigValidationError::EmptyTelegramToken);
        }

        if let Some(discord) = &self.channels.discord
            && discord.token.trim().is_empty()
        {
            return Err(GatewayConfigValidationError::EmptyDiscordToken);
        }

        if let Some(slack) = &self.channels.slack {
            if slack.bot_token.trim().is_empty() {
                return Err(GatewayConfigValidationError::EmptySlackBotToken);
            }
            if slack.app_token.trim().is_empty() {
                return Err(GatewayConfigValidationError::EmptySlackAppToken);
            }
        }

        if matches!(self.gateway.approval_mode, ApprovalMode::Allowlist)
            && self.gateway.approval_allowlist.is_empty()
        {
            return Err(GatewayConfigValidationError::EmptyApprovalAllowlist);
        }

        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_with_error().map_err(anyhow::Error::new)
    }

    pub fn enabled_channels(&self) -> Vec<&str> {
        let mut channels = Vec::new();
        if self.channels.telegram.is_some() {
            channels.push("telegram");
        }
        if self.channels.discord.is_some() {
            channels.push("discord");
        }
        if self.channels.slack.is_some() {
            channels.push("slack");
        }
        channels
    }

    pub fn has_channels(&self) -> bool {
        !self.enabled_channels().is_empty()
    }

    pub fn router_config(&self) -> RouterConfig {
        let bindings = self
            .routing
            .bindings
            .iter()
            .map(binding_to_runtime)
            .collect();

        RouterConfig {
            dm_scope: self.routing.dm_scope.clone().into(),
            bindings,
        }
    }

    pub fn render_title_template(
        &self,
        channel: &str,
        peer: &str,
        chat_type: &str,
        chat_id: &str,
    ) -> String {
        self.gateway
            .title_template
            .replace("{channel}", channel)
            .replace("{peer}", peer)
            .replace("{chat_type}", chat_type)
            .replace("{chat_id}", chat_id)
    }

    pub fn with_server(mut self, url: String, token: String) -> Self {
        self.server.url = url;
        self.server.token = token;
        self
    }

    fn apply_env_overrides(&mut self) {
        if self.server.url.trim().is_empty()
            && let Ok(value) = std::env::var("STAKPAK_GATEWAY_URL")
        {
            self.server.url = value;
        }

        if self.server.token.trim().is_empty()
            && let Ok(value) = std::env::var("STAKPAK_GATEWAY_TOKEN")
        {
            self.server.token = value;
        }

        if self.channels.telegram.is_none()
            && let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN")
        {
            self.channels.telegram = Some(TelegramConfig {
                token,
                require_mention: false,
            });
        }

        if self.channels.discord.is_none()
            && let Ok(token) = std::env::var("DISCORD_BOT_TOKEN")
        {
            self.channels.discord = Some(DiscordConfig {
                token,
                guilds: Vec::new(),
            });
        }

        if self.channels.slack.is_none() {
            let bot = std::env::var("SLACK_BOT_TOKEN").ok();
            let app = std::env::var("SLACK_APP_TOKEN").ok();
            if let (Some(bot_token), Some(app_token)) = (bot, app) {
                self.channels.slack = Some(SlackConfig {
                    bot_token,
                    app_token,
                });
            }
        }
    }

    fn apply_cli_overrides(&mut self, cli: &GatewayCliFlags) {
        if let Some(url) = &cli.url {
            self.server.url = url.clone();
        }
        if let Some(token) = &cli.token {
            self.server.token = token.clone();
        }
        if let Some(path) = &cli.store {
            self.gateway.store_path = path.clone();
        }

        if let Some(token) = &cli.telegram_token {
            self.channels.telegram = Some(TelegramConfig {
                token: token.clone(),
                require_mention: self
                    .channels
                    .telegram
                    .as_ref()
                    .map(|value| value.require_mention)
                    .unwrap_or(false),
            });
        }

        if let Some(token) = &cli.discord_token {
            let guilds = self
                .channels
                .discord
                .as_ref()
                .map(|value| value.guilds.clone())
                .unwrap_or_default();
            self.channels.discord = Some(DiscordConfig {
                token: token.clone(),
                guilds,
            });
        }

        if let (Some(bot_token), Some(app_token)) = (&cli.slack_bot_token, &cli.slack_app_token) {
            self.channels.slack = Some(SlackConfig {
                bot_token: bot_token.clone(),
                app_token: app_token.clone(),
            });
        }
    }

    fn normalize_paths(&mut self) {
        self.gateway.store_path = expand_tilde_path(&self.gateway.store_path);
    }
}

fn load_toml_root_table(config_path: &Path) -> Result<toml::value::Table> {
    if !config_path.exists() {
        return Ok(toml::value::Table::new());
    }

    let text = std::fs::read_to_string(config_path).map_err(|error| {
        anyhow!(
            "failed to read gateway config {}: {error}",
            config_path.display()
        )
    })?;

    let value: toml::Value = toml::from_str(&text).map_err(|error| {
        anyhow!(
            "failed to parse gateway config {}: {error}",
            config_path.display()
        )
    })?;

    match value {
        toml::Value::Table(table) => Ok(table),
        _ => Err(anyhow!(
            "failed to parse gateway config {}: top-level value must be a TOML table",
            config_path.display()
        )),
    }
}

fn ensure_subtable<'a>(table: &'a mut toml::value::Table, key: &str) -> &'a mut toml::value::Table {
    if !matches!(table.get(key), Some(toml::Value::Table(_))) {
        table.insert(
            key.to_string(),
            toml::Value::Table(toml::value::Table::new()),
        );
    }

    match table.get_mut(key) {
        Some(toml::Value::Table(subtable)) => subtable,
        _ => unreachable!("subtable just inserted"),
    }
}

fn upsert_optional_subtable<T: Serialize>(
    table: &mut toml::value::Table,
    key: &str,
    value: &Option<T>,
) -> Result<()> {
    match value {
        Some(inner) => {
            let serialized = toml::Value::try_from(inner)
                .map_err(|error| anyhow!("failed to serialize {key} config: {error}"))?;
            table.insert(key.to_string(), serialized);
        }
        None => {
            table.remove(key);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct PersistedGatewayConfig {
    #[serde(default)]
    server: PersistedServerConfig,
    #[serde(default)]
    gateway: PersistedGatewaySettings,
    #[serde(default)]
    routing: PersistedRoutingConfig,
    #[serde(default)]
    channels: PersistedChannelConfigs,
}

impl PersistedGatewayConfig {
    fn into_runtime(self) -> GatewayConfig {
        GatewayConfig {
            server: ServerConfig {
                url: if self.server.url.is_empty() {
                    "http://127.0.0.1:4096".to_string()
                } else {
                    self.server.url
                },
                token: self.server.token,
            },
            gateway: GatewaySettings {
                store_path: self
                    .gateway
                    .store
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(default_store_path),
                model: self.gateway.model,
                title_template: self
                    .gateway
                    .title_template
                    .unwrap_or_else(|| "{channel} / {peer}".to_string()),
                prune_after_hours: self.gateway.prune_after_hours.unwrap_or(168),
                delivery_context_ttl_hours: self.gateway.delivery_context_ttl_hours.unwrap_or(4),
                approval_mode: self.gateway.approval_mode.unwrap_or_default(),
                approval_allowlist: self.gateway.approval_allowlist.unwrap_or_default(),
            },
            routing: RoutingConfig {
                dm_scope: self.routing.dm_scope.unwrap_or_default(),
                bindings: self.routing.bindings,
            },
            channels: ChannelConfigs {
                telegram: self.channels.telegram,
                discord: self.channels.discord,
                slack: self.channels.slack,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct PersistedServerConfig {
    #[serde(default)]
    url: String,
    #[serde(default)]
    token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct PersistedGatewaySettings {
    #[serde(default)]
    store: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    title_template: Option<String>,
    #[serde(default)]
    prune_after_hours: Option<u64>,
    #[serde(default)]
    delivery_context_ttl_hours: Option<u64>,
    #[serde(default)]
    approval_mode: Option<ApprovalMode>,
    #[serde(default)]
    approval_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct PersistedRoutingConfig {
    #[serde(default)]
    dm_scope: Option<DmScopeSerde>,
    #[serde(default)]
    bindings: Vec<BindingConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct PersistedChannelConfigs {
    #[serde(default)]
    telegram: Option<TelegramConfig>,
    #[serde(default)]
    discord: Option<DiscordConfig>,
    #[serde(default)]
    slack: Option<SlackConfig>,
}

fn binding_to_runtime(binding: &BindingConfig) -> Binding {
    let peer = binding
        .direct
        .as_ref()
        .map(|id| PeerMatch {
            kind: PeerMatchKind::Direct,
            id: id.clone(),
        })
        .or_else(|| {
            binding.group.as_ref().map(|id| PeerMatch {
                kind: PeerMatchKind::Group,
                id: id.clone(),
            })
        });

    Binding {
        match_rule: BindingMatch {
            channel: binding.channel.clone().into(),
            peer,
        },
        routing_key: binding.routing_key.clone(),
    }
}

fn default_store_path() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".stakpak").join("autopilot").join("gateway.db");
    }
    PathBuf::from(".stakpak/autopilot/gateway.db")
}

fn expand_tilde_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(stripped);
    }

    if path_str == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }

    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        ApprovalMode, ChannelConfigs, GatewayCliFlags, GatewayConfig, GatewaySettings,
        TelegramConfig,
    };

    #[test]
    fn default_dm_scope_is_per_channel_peer() {
        let config = GatewayConfig::default();
        assert!(matches!(
            config.routing.dm_scope,
            super::DmScopeSerde::PerChannelPeer
        ));
    }

    #[test]
    fn validate_requires_channels() {
        let config = GatewayConfig::default();
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn validate_allowlist_requires_items() {
        let mut config = GatewayConfig::default();
        config.channels.telegram = Some(TelegramConfig {
            token: "abc".to_string(),
            require_mention: false,
        });
        config.gateway.approval_mode = ApprovalMode::Allowlist;
        config.gateway.approval_allowlist.clear();

        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn cli_overrides_tokens() {
        let mut config = GatewayConfig::default();
        let cli = GatewayCliFlags {
            telegram_token: Some("123:ABC".to_string()),
            ..Default::default()
        };

        config.apply_cli_overrides(&cli);
        assert_eq!(
            config
                .channels
                .telegram
                .as_ref()
                .map(|value| value.token.clone()),
            Some("123:ABC".to_string())
        );
    }

    #[test]
    fn title_template_rendering() {
        let config = GatewayConfig {
            channels: ChannelConfigs {
                telegram: Some(TelegramConfig {
                    token: "123:ABC".to_string(),
                    require_mention: false,
                }),
                discord: None,
                slack: None,
            },
            gateway: GatewaySettings {
                title_template: "{channel}:{chat_type}:{peer}".to_string(),
                ..GatewaySettings::default()
            },
            ..GatewayConfig::default()
        };

        let title = config.render_title_template("telegram", "42", "group", "-100");
        assert_eq!(title, "telegram:group:42");
    }

    #[test]
    fn load_unvalidated_allows_empty_channels() {
        let dir_result = tempfile::tempdir();
        assert!(dir_result.is_ok());
        let dir = match dir_result {
            Ok(value) => value,
            Err(error) => panic!("failed to create temp dir: {error}"),
        };
        let path = dir.path().join("autopilot.toml");

        let write_result = fs::write(
            &path,
            r##"
[server]
url = "http://127.0.0.1:4096"
token = ""
"##,
        );
        assert!(write_result.is_ok());

        let cli = GatewayCliFlags::default();
        let config_result = GatewayConfig::load_unvalidated(&path, &cli);
        assert!(config_result.is_ok());

        let config = match config_result {
            Ok(value) => value,
            Err(error) => panic!("failed to load config: {error}"),
        };
        assert!(config.enabled_channels().is_empty());
        assert!(config.validate().is_err());
    }

    #[test]
    fn save_preserves_autopilot_sections() {
        let dir_result = tempfile::tempdir();
        assert!(dir_result.is_ok());
        let dir = match dir_result {
            Ok(value) => value,
            Err(error) => panic!("failed to create temp dir: {error}"),
        };
        let path = dir.path().join("autopilot.toml");

        let seed = r##"
[server]
listen = "127.0.0.1:4096"
show_token = false
no_auth = false

[watch]
db_path = "~/.stakpak/autopilot/autopilot.db"
log_dir = "~/.stakpak/autopilot/logs"

[defaults]
profile = "default"

[[schedules]]
name = "health-check"
cron = "*/5 * * * *"
prompt = "Check system health"

[notifications]
gateway_url = "http://127.0.0.1:4096"
channel = "slack"
chat_id = "#ops"
"##;
        let write_result = fs::write(&path, seed);
        assert!(write_result.is_ok());

        let mut config = GatewayConfig::default();
        config.server.url = "http://127.0.0.1:5001".to_string();
        config.server.token = "secret-token".to_string();
        config.channels.telegram = Some(TelegramConfig {
            token: "123:ABC".to_string(),
            require_mention: false,
        });

        let save_result = config.save(&path);
        assert!(save_result.is_ok());

        let reloaded = fs::read_to_string(&path);
        assert!(reloaded.is_ok());
        let reloaded = match reloaded {
            Ok(value) => value,
            Err(error) => panic!("failed to read config: {error}"),
        };

        assert!(reloaded.contains("[watch]"));
        assert!(reloaded.contains("[[schedules]]"));
        assert!(reloaded.contains("[notifications]"));
        assert!(reloaded.contains("listen = \"127.0.0.1:4096\""));
        assert!(reloaded.contains("url = \"http://127.0.0.1:5001\""));
        assert!(reloaded.contains("token = \"secret-token\""));
    }
}
