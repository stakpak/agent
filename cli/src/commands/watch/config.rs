//! Configuration parsing and validation for the watch service.
//!
//! Handles loading and validating `watch.toml` configuration files.

use croner::Cron;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

/// Default path for watch configuration file.
pub const STAKPAK_WATCH_CONFIG_PATH: &str = "~/.stakpak/watch.toml";

/// Main watch configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Watch-level settings (database path, log directory).
    #[serde(default)]
    pub watch: WatchSettings,

    /// Default values for triggers.
    #[serde(default)]
    pub defaults: WatchDefaults,

    /// Optional outbound notifications routed through gateway API.
    #[serde(default)]
    pub notifications: Option<NotificationConfig>,

    /// List of scheduled triggers.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
}

/// Watch-level settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSettings {
    /// Path to SQLite database file.
    #[serde(default = "default_db_path")]
    pub db_path: String,

    /// Directory for log files.
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            log_dir: default_log_dir(),
        }
    }
}

fn default_db_path() -> String {
    "~/.stakpak/watch/watch.db".to_string()
}

fn default_log_dir() -> String {
    "~/.stakpak/watch/logs".to_string()
}

/// Determines which check script exit codes trigger the agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckTriggerOn {
    /// Trigger agent only on exit code 0 (default behavior).
    #[default]
    Success,
    /// Trigger agent on any non-zero exit code (1+).
    Failure,
    /// Trigger agent regardless of exit code (only timeout/error prevents trigger).
    Any,
}

impl CheckTriggerOn {
    /// Returns true if the given exit code should trigger the agent.
    pub fn should_trigger(&self, exit_code: i32) -> bool {
        match self {
            CheckTriggerOn::Success => exit_code == 0,
            CheckTriggerOn::Failure => exit_code != 0,
            CheckTriggerOn::Any => true,
        }
    }
}

impl std::fmt::Display for CheckTriggerOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckTriggerOn::Success => write!(f, "success"),
            CheckTriggerOn::Failure => write!(f, "failure"),
            CheckTriggerOn::Any => write!(f, "any"),
        }
    }
}

/// Default values applied to triggers when not specified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchDefaults {
    /// Default profile to use for agent invocation.
    #[serde(default = "default_profile")]
    pub profile: String,

    /// Default timeout for agent execution.
    #[serde(default = "default_timeout", with = "humantime_serde")]
    pub timeout: Duration,

    /// Default timeout for check script execution.
    #[serde(default = "default_check_timeout", with = "humantime_serde")]
    pub check_timeout: Duration,

    /// Enable Slack tools for agent (experimental).
    #[serde(default)]
    pub enable_slack_tools: bool,

    /// Enable subagents for agent.
    #[serde(default)]
    pub enable_subagents: bool,

    /// Pause when tools require approval instead of auto-approving.
    /// When true, the agent will pause and exit with code 10 when tools need approval.
    #[serde(default = "default_pause_on_approval")]
    pub pause_on_approval: bool,

    /// Determines which check script exit codes trigger the agent.
    /// - "success" (default): trigger on exit 0
    /// - "failure": trigger on non-zero exit codes (1+)
    /// - "any": trigger regardless of exit code
    #[serde(default)]
    pub check_trigger_on: CheckTriggerOn,
}

impl Default for WatchDefaults {
    fn default() -> Self {
        Self {
            profile: default_profile(),
            timeout: default_timeout(),
            check_timeout: default_check_timeout(),
            enable_slack_tools: false,
            enable_subagents: false,
            pause_on_approval: default_pause_on_approval(),
            check_trigger_on: CheckTriggerOn::default(),
        }
    }
}

fn default_profile() -> String {
    "default".to_string()
}

fn default_timeout() -> Duration {
    Duration::from_secs(30 * 60) // 30 minutes
}

fn default_check_timeout() -> Duration {
    Duration::from_secs(30) // 30 seconds
}

fn default_pause_on_approval() -> bool {
    false // Default to auto-approve, matching async mode default
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub gateway_url: String,
    #[serde(default)]
    pub gateway_token: Option<String>,
    #[serde(default)]
    pub notify_on: Option<NotifyOn>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub chat_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeliveryConfig {
    pub channel: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyOn {
    All,
    Completions,
    Failures,
    None,
}

impl NotificationConfig {
    pub fn should_notify(&self, trigger: &Trigger, success: bool) -> bool {
        let mode = trigger
            .notify_on
            .or(self.notify_on)
            .unwrap_or(NotifyOn::All);
        match mode {
            NotifyOn::All => true,
            NotifyOn::Completions => success,
            NotifyOn::Failures => !success,
            NotifyOn::None => false,
        }
    }

    pub fn default_delivery(&self) -> Option<DeliveryConfig> {
        let channel = self.channel.as_ref()?;
        let chat_id = self.chat_id.as_ref()?;
        Some(DeliveryConfig {
            channel: channel.clone(),
            chat_id: chat_id.clone(),
        })
    }
}

/// A scheduled trigger that can wake the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    /// Unique name for this trigger.
    pub name: String,

    /// Cron schedule expression (e.g., "*/15 * * * *").
    pub schedule: String,

    /// Optional path to check script.
    /// If provided, script must exit 0 to wake agent.
    pub check: Option<String>,

    /// Timeout for check script execution.
    /// Falls back to defaults.check_timeout if not specified.
    #[serde(default, with = "option_humantime_serde")]
    pub check_timeout: Option<Duration>,

    /// Determines which check script exit codes trigger the agent.
    /// Falls back to defaults.check_trigger_on if not specified.
    /// - "success" (default): trigger on exit 0
    /// - "failure": trigger on non-zero exit codes (1+)
    /// - "any": trigger regardless of exit code
    pub check_trigger_on: Option<CheckTriggerOn>,

    /// Prompt to pass to the agent when triggered.
    pub prompt: String,

    /// Profile to use for agent invocation.
    /// Falls back to defaults.profile if not specified.
    pub profile: Option<String>,

    /// Optional board ID for task tracking.
    pub board_id: Option<String>,

    /// Timeout for agent execution.
    /// Falls back to defaults.timeout if not specified.
    #[serde(default, with = "option_humantime_serde")]
    pub timeout: Option<Duration>,

    /// Enable Slack tools for agent (experimental).
    /// Falls back to defaults.enable_slack_tools if not specified.
    pub enable_slack_tools: Option<bool>,

    /// Enable subagents for agent.
    /// Falls back to defaults.enable_subagents if not specified.
    pub enable_subagents: Option<bool>,

    /// Pause when tools require approval instead of auto-approving.
    /// Falls back to defaults.pause_on_approval if not specified.
    pub pause_on_approval: Option<bool>,

    /// Notification mode override for this trigger.
    pub notify_on: Option<NotifyOn>,

    /// Notification delivery channel override.
    pub notify_channel: Option<String>,

    /// Notification chat target override.
    pub notify_chat_id: Option<String>,
}

impl Trigger {
    /// Get the effective profile, falling back to defaults.
    pub fn effective_profile<'a>(&'a self, defaults: &'a WatchDefaults) -> &'a str {
        self.profile.as_deref().unwrap_or(&defaults.profile)
    }

    /// Get the effective timeout, falling back to defaults.
    pub fn effective_timeout(&self, defaults: &WatchDefaults) -> Duration {
        self.timeout.unwrap_or(defaults.timeout)
    }

    /// Get the effective check timeout, falling back to defaults.
    pub fn effective_check_timeout(&self, defaults: &WatchDefaults) -> Duration {
        self.check_timeout.unwrap_or(defaults.check_timeout)
    }

    /// Get the effective check_trigger_on, falling back to defaults.
    pub fn effective_check_trigger_on(&self, defaults: &WatchDefaults) -> CheckTriggerOn {
        self.check_trigger_on.unwrap_or(defaults.check_trigger_on)
    }

    /// Get the effective enable_slack_tools, falling back to defaults.
    pub fn effective_enable_slack_tools(&self, defaults: &WatchDefaults) -> bool {
        self.enable_slack_tools
            .unwrap_or(defaults.enable_slack_tools)
    }

    /// Get the effective enable_subagents, falling back to defaults.
    pub fn effective_enable_subagents(&self, defaults: &WatchDefaults) -> bool {
        self.enable_subagents.unwrap_or(defaults.enable_subagents)
    }

    /// Get the effective pause_on_approval, falling back to defaults.
    pub fn effective_pause_on_approval(&self, defaults: &WatchDefaults) -> bool {
        self.pause_on_approval.unwrap_or(defaults.pause_on_approval)
    }

    /// Resolve notification delivery target using trigger overrides and global defaults.
    pub fn effective_delivery(&self, notifications: &NotificationConfig) -> Option<DeliveryConfig> {
        let channel = self
            .notify_channel
            .as_ref()
            .cloned()
            .or_else(|| notifications.channel.clone())?;

        let chat_id = self
            .notify_chat_id
            .as_ref()
            .cloned()
            .or_else(|| notifications.chat_id.clone())?;

        Some(DeliveryConfig { channel, chat_id })
    }
}

/// Custom serde module for Option<Duration> with humantime format.
mod option_humantime_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => {
                let s = humantime::format_duration(*d).to_string();
                s.serialize(serializer)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => humantime::parse_duration(&s)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

/// Errors that can occur during config loading and validation.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Invalid cron expression '{expression}' for trigger '{trigger}': {message}")]
    InvalidCron {
        trigger: String,
        expression: String,
        message: String,
    },

    #[error("Duplicate trigger name: '{0}'")]
    DuplicateTriggerName(String),

    #[error("Check script not found for trigger '{trigger}': {path}")]
    CheckScriptNotFound { trigger: String, path: String },

    #[error("Trigger '{0}' is missing required field: {1}")]
    MissingRequiredField(String, String),
}

impl WatchConfig {
    /// Load configuration from the default path (~/.stakpak/watch.toml).
    pub fn load_default() -> Result<Self, ConfigError> {
        let path = expand_tilde(STAKPAK_WATCH_CONFIG_PATH);
        Self::load(&path)
    }

    /// Load configuration from a specific path.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: WatchConfig = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Parse configuration from a string (useful for testing).
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        let config: WatchConfig = toml::from_str(content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_unique_trigger_names()?;
        self.validate_cron_expressions()?;
        self.validate_check_scripts()?;
        Ok(())
    }

    /// Ensure all trigger names are unique.
    fn validate_unique_trigger_names(&self) -> Result<(), ConfigError> {
        let mut seen = HashSet::new();
        for trigger in &self.triggers {
            if !seen.insert(&trigger.name) {
                return Err(ConfigError::DuplicateTriggerName(trigger.name.clone()));
            }
        }
        Ok(())
    }

    /// Validate all cron expressions are parseable.
    fn validate_cron_expressions(&self) -> Result<(), ConfigError> {
        for trigger in &self.triggers {
            if let Err(e) = Cron::from_str(&trigger.schedule) {
                return Err(ConfigError::InvalidCron {
                    trigger: trigger.name.clone(),
                    expression: trigger.schedule.clone(),
                    message: e.to_string(),
                });
            }
        }
        Ok(())
    }

    /// Validate check script paths exist (if specified).
    fn validate_check_scripts(&self) -> Result<(), ConfigError> {
        for trigger in &self.triggers {
            if let Some(check_path) = &trigger.check {
                let expanded = expand_tilde(check_path);
                if !expanded.exists() {
                    return Err(ConfigError::CheckScriptNotFound {
                        trigger: trigger.name.clone(),
                        path: check_path.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Get the expanded database path.
    pub fn db_path(&self) -> PathBuf {
        expand_tilde(&self.watch.db_path)
    }

    /// Get the expanded log directory path.
    pub fn log_dir(&self) -> PathBuf {
        expand_tilde(&self.watch.log_dir)
    }

    /// True when notifications are configured to local loopback gateway URL.
    pub fn notifications_points_to_local_loopback(&self) -> bool {
        let Some(notifications) = &self.notifications else {
            return false;
        };

        let url = notifications.gateway_url.to_ascii_lowercase();
        url.contains("127.0.0.1") || url.contains("localhost")
    }
}

/// Expand ~ to home directory in paths.
pub fn expand_tilde<P: AsRef<Path>>(path: P) -> PathBuf {
    let path_str = path.as_ref().to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path_str == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    path.as_ref().to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_config() {
        let config_str = r#"
[watch]
db_path = "~/.stakpak/watch/watch.db"
log_dir = "~/.stakpak/watch/logs"

[defaults]
profile = "production"
timeout = "1h"
check_timeout = "1m"

[[triggers]]
name = "disk-cleanup"
schedule = "*/15 * * * *"
prompt = "Check disk usage and clean up if needed"
profile = "maintenance"
timeout = "45m"

[[triggers]]
name = "health-check"
schedule = "0 * * * *"
prompt = "Run health checks"
board_id = "board_123"
"#;

        let config = WatchConfig::parse(config_str).expect("Should parse valid config");

        assert_eq!(config.watch.db_path, "~/.stakpak/watch/watch.db");
        assert_eq!(config.defaults.profile, "production");
        assert_eq!(config.defaults.timeout, Duration::from_secs(3600));
        assert_eq!(config.defaults.check_timeout, Duration::from_secs(60));
        assert_eq!(config.triggers.len(), 2);

        let trigger1 = &config.triggers[0];
        assert_eq!(trigger1.name, "disk-cleanup");
        assert_eq!(trigger1.schedule, "*/15 * * * *");
        assert_eq!(trigger1.profile, Some("maintenance".to_string()));
        assert_eq!(trigger1.timeout, Some(Duration::from_secs(45 * 60)));

        let trigger2 = &config.triggers[1];
        assert_eq!(trigger2.name, "health-check");
        assert_eq!(trigger2.board_id, Some("board_123".to_string()));
        // Should use defaults
        assert_eq!(trigger2.effective_profile(&config.defaults), "production");
    }

    #[test]
    fn test_parse_minimal_config() {
        let config_str = r#"
[[triggers]]
name = "simple"
schedule = "0 0 * * *"
prompt = "Do something"
"#;

        let config = WatchConfig::parse(config_str).expect("Should parse minimal config");

        // Check defaults are applied
        assert_eq!(config.watch.db_path, "~/.stakpak/watch/watch.db");
        assert_eq!(config.defaults.profile, "default");
        assert_eq!(config.defaults.timeout, Duration::from_secs(30 * 60));
        assert_eq!(config.triggers.len(), 1);
    }

    #[test]
    fn test_invalid_cron() {
        let config_str = r#"
[[triggers]]
name = "bad-cron"
schedule = "invalid cron expression"
prompt = "This should fail"
"#;

        let result = WatchConfig::parse(config_str);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::InvalidCron { .. }));
        if let ConfigError::InvalidCron { trigger, .. } = err {
            assert_eq!(trigger, "bad-cron");
        }
    }

    #[test]
    fn test_duplicate_trigger_names() {
        let config_str = r#"
[[triggers]]
name = "duplicate"
schedule = "0 * * * *"
prompt = "First"

[[triggers]]
name = "duplicate"
schedule = "0 0 * * *"
prompt = "Second"
"#;

        let result = WatchConfig::parse(config_str);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::DuplicateTriggerName(_)));
        if let ConfigError::DuplicateTriggerName(name) = err {
            assert_eq!(name, "duplicate");
        }
    }

    #[test]
    fn test_path_expansion() {
        let expanded = expand_tilde("~/test/path");
        assert!(!expanded.to_string_lossy().starts_with("~"));

        let home = dirs::home_dir().expect("Should have home dir");
        assert!(expanded.starts_with(&home));
        assert!(expanded.ends_with("test/path"));
    }

    #[test]
    fn test_default_values() {
        let config_str = r#"
[[triggers]]
name = "test"
schedule = "0 0 * * *"
prompt = "Test prompt"
"#;

        let config = WatchConfig::parse(config_str).expect("Should parse");
        let trigger = &config.triggers[0];

        // Verify defaults are used
        assert_eq!(trigger.effective_profile(&config.defaults), "default");
        assert_eq!(
            trigger.effective_timeout(&config.defaults),
            Duration::from_secs(30 * 60)
        );
        assert_eq!(
            trigger.effective_check_timeout(&config.defaults),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn test_various_cron_expressions() {
        // Test various valid cron expressions (standard 5-part: min hour day month weekday)
        let expressions = [
            "* * * * *",     // Every minute
            "*/5 * * * *",   // Every 5 minutes
            "0 0 * * *",     // Daily at midnight
            "0 0 * * 0",     // Weekly on Sunday
            "0 0 1 * *",     // Monthly on 1st
            "0 0 1 1 *",     // Yearly on Jan 1st
            "30 4 1,15 * *", // 4:30 AM on 1st and 15th
            "0 0-5 * * *",   // Midnight to 5 AM hourly
            "0 0 * * 1-5",   // Weekdays at midnight
            "0 9 * * 1-5",   // Weekdays at 9 AM
        ];

        for expr in expressions {
            let config_str = format!(
                r#"
[[triggers]]
name = "test"
schedule = "{}"
prompt = "Test"
"#,
                expr
            );

            let result = WatchConfig::parse(&config_str);
            assert!(
                result.is_ok(),
                "Should parse valid cron expression: {}",
                expr
            );
        }
    }

    #[test]
    fn test_humantime_durations() {
        let config_str = r#"
[defaults]
timeout = "2h 30m"
check_timeout = "45s"

[[triggers]]
name = "test"
schedule = "0 0 * * *"
prompt = "Test"
timeout = "1h 15m 30s"
check_timeout = "2m"
"#;

        let config = WatchConfig::parse(config_str).expect("Should parse humantime durations");

        assert_eq!(
            config.defaults.timeout,
            Duration::from_secs(2 * 3600 + 30 * 60)
        );
        assert_eq!(config.defaults.check_timeout, Duration::from_secs(45));

        let trigger = &config.triggers[0];
        assert_eq!(
            trigger.timeout,
            Some(Duration::from_secs(3600 + 15 * 60 + 30))
        );
        assert_eq!(trigger.check_timeout, Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_empty_triggers() {
        let config_str = r#"
[watch]
db_path = "/custom/path/watch.db"
"#;

        let config = WatchConfig::parse(config_str).expect("Should parse config with no triggers");
        assert!(config.triggers.is_empty());
    }

    #[test]
    fn test_check_script_not_found() {
        let config_str = r#"
[[triggers]]
name = "with-check"
schedule = "0 * * * *"
prompt = "Test"
check = "/nonexistent/path/to/script.sh"
"#;

        let result = WatchConfig::parse(config_str);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, ConfigError::CheckScriptNotFound { .. }));
        if let ConfigError::CheckScriptNotFound { trigger, path } = err {
            assert_eq!(trigger, "with-check");
            assert_eq!(path, "/nonexistent/path/to/script.sh");
        }
    }

    #[test]
    fn test_missing_required_field_name() {
        let config_str = r#"
[[triggers]]
schedule = "0 * * * *"
prompt = "Test"
"#;

        let result = WatchConfig::parse(config_str);
        assert!(result.is_err());
        // Should fail at TOML parsing level due to missing required field
        assert!(matches!(result.unwrap_err(), ConfigError::ParseError(_)));
    }

    #[test]
    fn test_missing_required_field_schedule() {
        let config_str = r#"
[[triggers]]
name = "test"
prompt = "Test"
"#;

        let result = WatchConfig::parse(config_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::ParseError(_)));
    }

    #[test]
    fn test_missing_required_field_prompt() {
        let config_str = r#"
[[triggers]]
name = "test"
schedule = "0 * * * *"
"#;

        let result = WatchConfig::parse(config_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::ParseError(_)));
    }

    #[test]
    fn test_check_trigger_on_should_trigger() {
        // Success mode: only exit 0 triggers
        assert!(CheckTriggerOn::Success.should_trigger(0));
        assert!(!CheckTriggerOn::Success.should_trigger(1));
        assert!(!CheckTriggerOn::Success.should_trigger(2));
        assert!(!CheckTriggerOn::Success.should_trigger(-1));

        // Failure mode: any non-zero triggers
        assert!(!CheckTriggerOn::Failure.should_trigger(0));
        assert!(CheckTriggerOn::Failure.should_trigger(1));
        assert!(CheckTriggerOn::Failure.should_trigger(2));
        assert!(CheckTriggerOn::Failure.should_trigger(-1));

        // Any mode: all exit codes trigger
        assert!(CheckTriggerOn::Any.should_trigger(0));
        assert!(CheckTriggerOn::Any.should_trigger(1));
        assert!(CheckTriggerOn::Any.should_trigger(2));
        assert!(CheckTriggerOn::Any.should_trigger(-1));
    }

    #[test]
    fn test_check_trigger_on_default() {
        assert_eq!(CheckTriggerOn::default(), CheckTriggerOn::Success);
    }

    #[test]
    fn test_check_trigger_on_display() {
        assert_eq!(CheckTriggerOn::Success.to_string(), "success");
        assert_eq!(CheckTriggerOn::Failure.to_string(), "failure");
        assert_eq!(CheckTriggerOn::Any.to_string(), "any");
    }

    #[test]
    fn test_check_trigger_on_parsing() {
        // Test parsing from TOML
        let config_str = r#"
[[triggers]]
name = "success-trigger"
schedule = "0 * * * *"
prompt = "Test"
check_trigger_on = "success"

[[triggers]]
name = "failure-trigger"
schedule = "0 * * * *"
prompt = "Test"
check_trigger_on = "failure"

[[triggers]]
name = "any-trigger"
schedule = "0 * * * *"
prompt = "Test"
check_trigger_on = "any"

[[triggers]]
name = "default-trigger"
schedule = "0 * * * *"
prompt = "Test"
"#;

        let config = WatchConfig::parse(config_str).expect("Should parse check_trigger_on values");
        assert_eq!(config.triggers.len(), 4);

        assert_eq!(
            config.triggers[0].check_trigger_on,
            Some(CheckTriggerOn::Success)
        );
        assert_eq!(
            config.triggers[1].check_trigger_on,
            Some(CheckTriggerOn::Failure)
        );
        assert_eq!(
            config.triggers[2].check_trigger_on,
            Some(CheckTriggerOn::Any)
        );
        assert_eq!(config.triggers[3].check_trigger_on, None);
    }

    #[test]
    fn test_check_trigger_on_defaults_fallback() {
        let config_str = r#"
[defaults]
check_trigger_on = "failure"

[[triggers]]
name = "uses-default"
schedule = "0 * * * *"
prompt = "Test"

[[triggers]]
name = "overrides-default"
schedule = "0 * * * *"
prompt = "Test"
check_trigger_on = "success"
"#;

        let config =
            WatchConfig::parse(config_str).expect("Should parse check_trigger_on with defaults");

        // First trigger should use default (failure)
        assert_eq!(
            config.triggers[0].effective_check_trigger_on(&config.defaults),
            CheckTriggerOn::Failure
        );

        // Second trigger should override to success
        assert_eq!(
            config.triggers[1].effective_check_trigger_on(&config.defaults),
            CheckTriggerOn::Success
        );
    }

    #[test]
    fn test_check_trigger_on_invalid_value() {
        let config_str = r#"
[[triggers]]
name = "invalid"
schedule = "0 * * * *"
prompt = "Test"
check_trigger_on = "invalid"
"#;

        let result = WatchConfig::parse(config_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::ParseError(_)));
    }
}
