//! Basic configuration types.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Provider type selection for the CLI.
#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    /// Use the Stakpak remote API
    #[default]
    Remote,
    /// Use local LLM providers directly
    Local,
}

/// Configuration for persistent shell sessions.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShellSessionsConfig {
    /// Enable persistent shell sessions (default: true)
    #[serde(default = "default_shell_sessions_enabled")]
    pub enabled: bool,

    /// Default shell for local sessions (auto-detect if not set)
    pub default_shell: Option<String>,

    /// Session timeout in seconds (0 = no timeout, default: 3600)
    #[serde(default = "default_session_timeout")]
    pub session_timeout: u64,

    /// Maximum concurrent sessions (default: 10)
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,

    /// Command completion timeout in seconds (default: 300)
    #[serde(default = "default_command_timeout")]
    pub command_timeout: u64,
}

fn default_shell_sessions_enabled() -> bool {
    true
}

fn default_session_timeout() -> u64 {
    3600
}

fn default_max_sessions() -> usize {
    10
}

fn default_command_timeout() -> u64 {
    300
}

impl Default for ShellSessionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_shell: None,
            session_timeout: 3600,
            max_sessions: 10,
            command_timeout: 300,
        }
    }
}

impl ShellSessionsConfig {
    /// Convert to the shared library's ShellSessionConfig
    pub fn to_shared_config(&self) -> stakpak_shared::shell_session::ShellSessionConfig {
        stakpak_shared::shell_session::ShellSessionConfig {
            enabled: self.enabled,
            default_shell: self.default_shell.clone(),
            session_timeout: Duration::from_secs(self.session_timeout),
            max_sessions: self.max_sessions,
            command_timeout: Duration::from_secs(self.command_timeout),
        }
    }
}

/// Global settings that apply across all profiles.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    /// Machine name for identification
    pub machine_name: Option<String>,
    /// Whether to auto-append .stakpak to .gitignore
    pub auto_append_gitignore: Option<bool>,
    /// Unique ID for anonymous telemetry (formerly user_id)
    #[serde(alias = "user_id")]
    pub anonymous_id: Option<String>,
    /// Whether to collect telemetry data
    pub collect_telemetry: Option<bool>,
    /// Preferred external editor (e.g. vim, nano, code)
    pub editor: Option<String>,
    /// Shell session configuration
    #[serde(default)]
    pub shell_sessions: Option<ShellSessionsConfig>,
}

/// Legacy configuration format for migration purposes.
#[derive(Deserialize, Clone)]
pub(crate) struct OldAppConfig {
    pub api_endpoint: String,
    pub api_key: Option<String>,
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
}

impl From<OldAppConfig> for Settings {
    fn from(old_config: OldAppConfig) -> Self {
        Settings {
            machine_name: old_config.machine_name,
            auto_append_gitignore: old_config.auto_append_gitignore,
            anonymous_id: Some(uuid::Uuid::new_v4().to_string()),
            collect_telemetry: Some(true),
            editor: Some("nano".to_string()),
            shell_sessions: None,
        }
    }
}
