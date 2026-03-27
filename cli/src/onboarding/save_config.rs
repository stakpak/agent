//! Configuration saving utilities

use crate::config::{ConfigFile, ProfileConfig, ProviderType};
use crate::onboarding::config_templates::config_to_toml_preview;
use crate::onboarding::styled_output;
use stakpak_shared::telemetry::{TelemetryEvent, capture_event, is_telemetry_enabled, is_telemetry_env_enabled};
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

    // SOVEREIGNTY GUARD: Only generate anonymous_id if telemetry is explicitly enabled
    // Both config AND environment variable must opt-in
    if is_local_provider 
        && config_file.settings.anonymous_id.is_none()
        && is_telemetry_enabled(config_file.settings.collect_telemetry)
        && is_telemetry_env_enabled()
    {
        config_file.settings.anonymous_id = Some(uuid::Uuid::new_v4().to_string());
    }

    // DO NOT set collect_telemetry to true by default - maintain opt-in only
    // If user hasn't explicitly set it, keep it as None (will default to false)
    if is_local_provider && config_file.settings.collect_telemetry.is_none() {
        // Never auto-enable telemetry - keep it disabled by default
        config_file.settings.collect_telemetry = Some(false);
    }

    config_file
        .profiles
        .insert(profile_name.to_string(), profile);

    // Update readonly profile to match the current default profile
    // This ensures readonly always mirrors default's provider settings
    config_file.update_readonly();

    // Save config file (uses atomic write with 0600 permissions for security)
    config_file
        .save_to(&path)
        .map_err(|e| format!("Failed to save config file: {}", e))?;

    // SOVEREIGNTY GUARD: Only capture telemetry if user explicitly enabled it
    // Requires: config opt-in AND env var opt-in AND anonymous_id generated
    if is_local_provider
        && is_first_telemetry_setup
        && is_telemetry_enabled(config_file.settings.collect_telemetry)
        && is_telemetry_env_enabled()
        && config_file.settings.collect_telemetry.unwrap_or(false)
        && let Some(ref anonymous_id) = config_file.settings.anonymous_id
    {
        capture_event(
            anonymous_id,
            config_file.settings.machine_name.as_deref(),
            true, // telemetry is enabled
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
    styled_output::render_config_preview(&config_to_toml_preview(&profile, profile_name));

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