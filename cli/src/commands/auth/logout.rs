//! Logout command - remove provider credentials

use super::{CredentialSource, collect_all_credentials};
use crate::config::AppConfig;
use crate::onboarding::menu::select_option_no_header;
use crate::onboarding::navigation::NavResult;
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::oauth::ProviderRegistry;
use std::path::Path;

/// Handle the logout command
pub fn handle_logout(
    config_dir: &Path,
    provider: Option<&str>,
    profile: Option<&str>,
) -> Result<(), String> {
    let registry = ProviderRegistry::new();
    let all_credentials = collect_all_credentials(config_dir);

    // If no credentials exist, inform the user
    if all_credentials.is_empty()
        || all_credentials
            .values()
            .all(|providers| providers.is_empty())
    {
        println!("No credentials configured.");
        return Ok(());
    }

    // Determine which provider to remove
    let provider_id = match provider {
        Some(p) => p.to_string(),
        None => {
            // Interactive selection - list all configured providers
            let mut all_providers: Vec<String> = Vec::new();

            for providers in all_credentials.values() {
                for provider_id in providers.keys() {
                    if !all_providers.contains(provider_id) {
                        all_providers.push(provider_id.clone());
                    }
                }
            }

            if all_providers.is_empty() {
                println!("No credentials configured.");
                return Ok(());
            }

            all_providers.sort();

            let options: Vec<(String, &str, bool)> = all_providers
                .iter()
                .map(|id| {
                    let name = registry.get(id).map(|p| p.name()).unwrap_or(id.as_str());
                    (id.clone(), name, false)
                })
                .collect();

            // Need to create owned strings for display
            let options_with_names: Vec<(String, String, bool)> = options
                .into_iter()
                .map(|(id, name, recommended)| (id, name.to_string(), recommended))
                .collect();

            let options_refs: Vec<(String, &str, bool)> = options_with_names
                .iter()
                .map(|(id, name, recommended)| (id.clone(), name.as_str(), *recommended))
                .collect();

            println!();
            println!("Select provider to log out:");
            println!();

            match select_option_no_header(&options_refs, false) {
                NavResult::Forward(selected) => selected,
                NavResult::Back | NavResult::Cancel => {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
        }
    };

    // Determine which profile to remove from
    let target_profile = match profile {
        Some(p) => p.to_string(),
        None => {
            // Find all profiles that have this provider
            let profiles_with_provider: Vec<&String> = all_credentials
                .iter()
                .filter(|(_, providers)| providers.contains_key(&provider_id))
                .map(|(profile, _)| profile)
                .collect();

            if profiles_with_provider.is_empty() {
                let provider_name = registry
                    .get(&provider_id)
                    .map(|p| p.name())
                    .unwrap_or(&provider_id);
                println!("No credentials found for {}.", provider_name);
                return Ok(());
            }

            if profiles_with_provider.len() == 1 {
                // Only one profile has this provider, use it directly
                profiles_with_provider[0].clone()
            } else {
                // Multiple profiles - let user choose
                let options: Vec<(String, String, bool)> = profiles_with_provider
                    .iter()
                    .map(|p| {
                        let label = if *p == "all" {
                            "shared (all profiles)".to_string()
                        } else {
                            format!("profile '{}'", p)
                        };
                        ((*p).clone(), label, false)
                    })
                    .collect();

                let options_refs: Vec<(String, &str, bool)> = options
                    .iter()
                    .map(|(id, label, recommended)| (id.clone(), label.as_str(), *recommended))
                    .collect();

                println!();
                println!("Remove from which profile?");
                println!();

                match select_option_no_header(&options_refs, true) {
                    NavResult::Forward(selected) => selected,
                    NavResult::Back | NavResult::Cancel => {
                        println!("Cancelled.");
                        return Ok(());
                    }
                }
            }
        }
    };

    // Find the source of credentials
    let source = all_credentials
        .get(&target_profile)
        .and_then(|providers| providers.get(&provider_id))
        .map(|(_, source)| *source);

    let mut removed = false;

    // Remove from the appropriate source
    match source {
        Some(CredentialSource::ConfigToml) => {
            // Remove from config.toml
            let config_path = config_dir.join("config.toml");
            if let Ok(mut config_file) = AppConfig::load_config_file(&config_path)
                && let Some(profile_config) = config_file.profiles.get_mut(&target_profile)
                && let Some(provider_config) = profile_config.providers.get_mut(&provider_id)
            {
                provider_config.clear_auth();
                // Update readonly profile if we modified the default profile
                if target_profile == "default" {
                    config_file.update_readonly();
                }
                if let Err(e) = config_file.save_to(&config_path) {
                    return Err(format!("Failed to save config: {}", e));
                }
                removed = true;
            }
        }
        Some(CredentialSource::AuthToml) => {
            // Remove from auth.toml (legacy)
            if let Ok(mut auth_manager) = AuthManager::new(config_dir) {
                removed = auth_manager
                    .remove(&target_profile, &provider_id)
                    .map_err(|e| format!("Failed to remove credentials: {}", e))?;
            }
        }
        None => {
            // Try both sources
            // First try config.toml
            let config_path = config_dir.join("config.toml");
            if let Ok(mut config_file) = AppConfig::load_config_file(&config_path)
                && let Some(profile_config) = config_file.profiles.get_mut(&target_profile)
                && let Some(provider_config) = profile_config.providers.get_mut(&provider_id)
            {
                provider_config.clear_auth();
                // Update readonly profile if we modified the default profile
                if target_profile == "default" {
                    config_file.update_readonly();
                }
                if config_file.save_to(&config_path).is_ok() {
                    removed = true;
                }
            }
            // Then try auth.toml
            if !removed
                && let Ok(mut auth_manager) = AuthManager::new(config_dir)
                && let Ok(r) = auth_manager.remove(&target_profile, &provider_id)
            {
                removed = r;
            }
        }
    }

    let provider_name = registry
        .get(&provider_id)
        .map(|p| p.name())
        .unwrap_or(&provider_id);

    if removed {
        let profile_label = if target_profile == "all" {
            "shared defaults".to_string()
        } else {
            format!("profile '{}'", target_profile)
        };
        println!();
        println!(
            "Removed {} credentials from {}.",
            provider_name, profile_label
        );
    } else {
        println!();
        println!(
            "No {} credentials found in specified profile.",
            provider_name
        );
    }

    Ok(())
}
