//! Basic configuration types.

use serde::{Deserialize, Serialize};

/// Provider type selection for the CLI.
#[derive(Default, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    /// Use the Stakpak remote API
    #[default]
    Remote,
    /// Use local LLM providers directly
    Local,
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
    /// **DEFAULT: false (opt-in required for privacy)**
    /// Users must explicitly set this to `true` to enable telemetry
    pub collect_telemetry: Option<bool>,
    /// Preferred external editor (e.g. vim, nano, code)
    pub editor: Option<String>,
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
            // Do NOT generate anonymous_id by default - only if telemetry is enabled
            anonymous_id: None,
            // DEFAULT: Telemetry is OPT-IN, never OPT-OUT
            collect_telemetry: Some(false),
            editor: Some("nano".to_string()),
        }
    }
}