//! Configuration saving utilities

use crate::config::{ConfigFile, ProfileConfig};
use crate::onboarding::config_templates::config_to_toml_preview;
use crate::onboarding::styled_output;
use std::fs;
use std::path::PathBuf;

/// Save profile configuration to a named profile
pub fn save_to_profile(
    config_path: &str,
    profile_name: &str,
    profile: ProfileConfig,
) -> Result<(), String> {
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

    // Save to specified profile
    config_file
        .profiles
        .insert(profile_name.to_string(), profile);

    // Ensure config directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    // Write config file
    let config_str = toml::to_string_pretty(&config_file)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    fs::write(&path, config_str).map_err(|e| format!("Failed to write config file: {}", e))?;

    Ok(())
}

/// Show configuration preview and confirm before saving to a named profile
pub fn preview_and_save_to_profile(
    config_path: &str,
    profile_name: &str,
    profile: ProfileConfig,
) -> Result<(), String> {
    // Show preview
    styled_output::render_config_preview(&config_to_toml_preview(&profile));

    // Save
    save_to_profile(config_path, profile_name, profile)?;

    println!();
    styled_output::render_success(&format!(
        "✓ Configuration saved successfully to [profiles.{}]",
        profile_name
    ));
    println!();

    Ok(())
}
