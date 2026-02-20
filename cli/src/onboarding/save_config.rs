//! Configuration saving utilities

use crate::config::{ConfigFile, ProfileConfig, ProviderType};
use crate::onboarding::config_templates::config_to_toml_preview;
use crate::onboarding::styled_output;
use stakpak_shared::telemetry::{TelemetryEvent, capture_event};
use std::fs;
use std::path::PathBuf;

/// Telemetry settings returned after saving a profile
#[derive(Clone, Debug, Default)]
pub struct TelemetrySettings {
    pub anonymous_id: Option<String>,
    pub collect_telemetry: Option<bool>,
}

/// Save profile configuration to a named profile
/// Returns telemetry settings that may have been generated
pub fn save_to_profile(
    config_path: &str,
    profile_name: &str,
    profile: ProfileConfig,
) -> Result<TelemetrySettings, String> {
    let path = PathBuf::from(config_path);

    // Load existing config or create new
    let mut config_file = if path.exists() {
        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read config file: {}", e))?;
        toml::from_str::<ConfigFile>(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?
    } else {
        ConfigFile::default()
    };

    let is_local_provider = matches!(profile.provider, Some(ProviderType::Local));
    let is_first_telemetry_setup = config_file.settings.anonymous_id.is_none();

    if is_local_provider && config_file.settings.anonymous_id.is_none() {
        config_file.settings.anonymous_id = Some(uuid::Uuid::new_v4().to_string());
    }
    if is_local_provider && config_file.settings.collect_telemetry.is_none() {
        config_file.settings.collect_telemetry = Some(true);
    }

    config_file
        .profiles
        .insert(profile_name.to_string(), profile);

    // Update readonly profile to match the current default profile
    // This ensures readonly always mirrors default's provider settings
    config_file.update_readonly();

    // Ensure config directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    // Write config file
    let config_str = toml::to_string_pretty(&config_file)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    fs::write(&path, config_str).map_err(|e| format!("Failed to write config file: {}", e))?;

    if is_local_provider
        && is_first_telemetry_setup
        && let Some(ref anonymous_id) = config_file.settings.anonymous_id
        && config_file.settings.collect_telemetry.unwrap_or(true)
    {
        capture_event(
            anonymous_id,
            config_file.settings.machine_name.as_deref(),
            true,
            TelemetryEvent::FirstOpen,
        );
    }

    Ok(TelemetrySettings {
        anonymous_id: config_file.settings.anonymous_id,
        collect_telemetry: config_file.settings.collect_telemetry,
    })
}

/// Show configuration preview and confirm before saving to a named profile
pub fn preview_and_save_to_profile(
    config_path: &str,
    profile_name: &str,
    profile: ProfileConfig,
) -> Result<TelemetrySettings, String> {
    // Show preview
    styled_output::render_config_preview(&config_to_toml_preview(&profile));

    // Save
    let telemetry_settings = save_to_profile(config_path, profile_name, profile)?;

    println!();
    styled_output::render_success(&format!(
        "✓ Configuration saved successfully to [profiles.{}]",
        profile_name
    ));
    println!();

    Ok(telemetry_settings)
}
