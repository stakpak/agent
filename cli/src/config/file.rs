//! Configuration file structure and operations.

use config::ConfigError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{create_dir_all, write};
use std::path::Path;

use super::profile::ProfileConfig;
use super::types::{OldAppConfig, Settings};
use super::STAKPAK_API_ENDPOINT;

/// The complete configuration file structure.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigFile {
    /// Named profiles for different environments
    pub profiles: HashMap<String, ProfileConfig>,
    /// Global settings
    pub settings: Settings,
}

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            profiles: HashMap::new(),
            settings: Settings {
                machine_name: None,
                auto_append_gitignore: Some(true),
                anonymous_id: Some(uuid::Uuid::new_v4().to_string()),
                collect_telemetry: Some(true),
                editor: Some("nano".to_string()),
            },
        }
    }
}

impl ConfigFile {
    /// Create a config file with a default profile.
    pub(crate) fn with_default_profile() -> Self {
        ConfigFile {
            profiles: HashMap::from([(
                "default".into(),
                ProfileConfig::with_api_endpoint(STAKPAK_API_ENDPOINT),
            )]),
            settings: Settings {
                machine_name: None,
                auto_append_gitignore: Some(true),
                anonymous_id: Some(uuid::Uuid::new_v4().to_string()),
                collect_telemetry: Some(true),
                editor: Some("nano".to_string()),
            },
        }
    }

    /// Get a profile configuration by name.
    pub(crate) fn profile_config(&self, profile_name: &str) -> Option<&ProfileConfig> {
        self.profiles.get(profile_name)
    }

    /// Get a profile configuration or return an error.
    pub(crate) fn profile_config_ok_or(
        &self,
        profile_name: &str,
    ) -> Result<ProfileConfig, ConfigError> {
        self.profile_config(profile_name).cloned().ok_or_else(|| {
            ConfigError::Message(format!(
                "Profile '{}' not found in configuration",
                profile_name
            ))
        })
    }

    /// Get the specified profile with inheritance from "all" profile.
    pub(crate) fn resolved_profile_config(
        &self,
        profile_name: &str,
    ) -> Result<ProfileConfig, ConfigError> {
        let profile = self.profile_config_ok_or(profile_name)?;
        Ok(profile.merge(self.profile_config("all")))
    }

    /// Insert or update a profile from an AppConfig.
    pub(crate) fn insert_app_config(&mut self, config: super::AppConfig) {
        self.profiles
            .insert(config.profile_name.clone(), config.into());
    }

    /// Update settings from an AppConfig.
    pub(crate) fn set_app_config_settings(&mut self, config: super::AppConfig) {
        // Preserve existing anonymous_id and collect_telemetry if AppConfig values are None
        let existing_anonymous_id = self.settings.anonymous_id.clone();
        let existing_collect_telemetry = self.settings.collect_telemetry;
        let existing_editor = self.settings.editor.clone();

        self.settings = Settings {
            machine_name: config.machine_name,
            auto_append_gitignore: config.auto_append_gitignore,
            anonymous_id: config.anonymous_id.or(existing_anonymous_id),
            collect_telemetry: config.collect_telemetry.or(existing_collect_telemetry),
            editor: config.editor.or(existing_editor),
        };
    }

    /// Check if a readonly profile exists.
    pub(crate) fn contains_readonly(&self) -> bool {
        self.profiles.contains_key("readonly")
    }

    /// Ensure a readonly profile exists, creating one if needed.
    /// Returns true if a new profile was created.
    pub(crate) fn ensure_readonly(&mut self) -> bool {
        if self.contains_readonly() {
            false
        } else {
            self.profiles.insert(
                "readonly".into(),
                ProfileConfig::readonly_profile(self.profile_config("default")),
            );
            true
        }
    }

    /// Update the readonly profile to match the current default profile.
    /// This should be called after modifying the default profile.
    pub(crate) fn update_readonly(&mut self) {
        self.profiles.insert(
            "readonly".into(),
            ProfileConfig::readonly_profile(self.profile_config("default")),
        );
    }

    /// Save the config file to disk.
    ///
    /// Uses atomic write (temp file + rename) and sets 0600 permissions on Unix
    /// since config may contain sensitive credentials.
    pub(crate) fn save_to<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let path = path.as_ref();

        if let Some(parent) = path.parent() {
            create_dir_all(parent).map_err(|e| {
                ConfigError::Message(format!("Failed to create config directory: {}", e))
            })?;
        }

        let body = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Message(format!("Failed to serialize config file: {}", e)))?;

        // Write to temp file first for atomicity
        let temp_path = path.with_extension("toml.tmp");
        write(&temp_path, &body)
            .map_err(|e| ConfigError::Message(format!("Failed to write config file: {}", e)))?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        // This is important since config may contain API keys and OAuth tokens
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&temp_path, permissions).map_err(|e| {
                ConfigError::Message(format!("Failed to set config file permissions: {}", e))
            })?;
        }

        // Atomic rename
        std::fs::rename(&temp_path, path)
            .map_err(|e| ConfigError::Message(format!("Failed to save config file: {}", e)))
    }
}

impl From<OldAppConfig> for ConfigFile {
    fn from(old_config: OldAppConfig) -> Self {
        let settings: Settings = old_config.clone().into();
        ConfigFile {
            profiles: HashMap::from([(
                "default".to_string(),
                ProfileConfig::migrated_from_old_config(old_config),
            )]),
            settings,
        }
    }
}
