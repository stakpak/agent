//! List credentials command

use crate::config::AppConfig;
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::oauth::ProviderRegistry;
use std::collections::HashMap;
use std::path::Path;

/// Handle the list credentials command
pub fn handle_list(config_dir: &Path, profile: Option<&str>) -> Result<(), String> {
    let registry = ProviderRegistry::new();

    // Collect credentials from both config.toml and auth.toml (legacy)
    let mut all_credentials: HashMap<String, HashMap<String, ProviderAuth>> = HashMap::new();

    // 1. Read from config.toml (new format)
    let config_path = config_dir.join("config.toml");
    if let Ok(config_file) = AppConfig::load_config_file(&config_path) {
        for (profile_name, profile_config) in &config_file.profiles {
            for (provider_name, provider_config) in &profile_config.providers {
                if let Some(auth) = provider_config.get_auth() {
                    all_credentials
                        .entry(profile_name.clone())
                        .or_default()
                        .insert(provider_name.clone(), auth);
                }
            }
        }
    }

    // 2. Read from auth.toml (legacy, for users who haven't migrated)
    if let Ok(auth_manager) = AuthManager::new(config_dir) {
        for (profile_name, providers) in auth_manager.list() {
            for (provider_name, auth) in providers {
                // Only add if not already in config.toml
                let profile_creds = all_credentials.entry(profile_name.clone()).or_default();
                if !profile_creds.contains_key(provider_name.as_str()) {
                    profile_creds.insert(provider_name.clone(), auth.clone());
                }
            }
        }
    }

    if all_credentials.is_empty() {
        println!("No credentials configured.");
        println!();
        println!("Run 'stakpak auth login' to add credentials.");
        return Ok(());
    }

    println!("Configured credentials:");
    println!();

    // Sort profiles for consistent output
    let mut profile_names: Vec<_> = all_credentials.keys().collect();
    profile_names.sort();

    // Put "all" first if present
    profile_names.sort_by(|a, b| {
        if *a == "all" {
            std::cmp::Ordering::Less
        } else if *b == "all" {
            std::cmp::Ordering::Greater
        } else {
            a.cmp(b)
        }
    });

    for profile_name in profile_names {
        // Filter by profile if specified
        if let Some(filter_profile) = profile
            && profile_name != filter_profile
            && profile_name != "all"
        {
            continue;
        }

        let Some(providers) = all_credentials.get(profile_name) else {
            continue;
        };
        if providers.is_empty() {
            continue;
        }

        let profile_label = if profile_name == "all" {
            "shared (all profiles)".to_string()
        } else {
            format!("profile '{}'", profile_name)
        };

        println!("  {}:", profile_label);

        // Sort providers for consistent output
        let mut provider_ids: Vec<_> = providers.keys().collect();
        provider_ids.sort();

        for provider_id in provider_ids {
            let Some(auth) = providers.get(provider_id) else {
                continue;
            };

            let provider_name = registry
                .get(provider_id)
                .map(|p| p.name())
                .unwrap_or(provider_id.as_str());

            let auth_type = auth.auth_type_display();

            // For OAuth, show if tokens need refresh
            let status = if auth.is_oauth() {
                if auth.is_expired() {
                    " (expired)"
                } else if auth.needs_refresh() {
                    " (needs refresh)"
                } else {
                    ""
                }
            } else {
                ""
            };

            println!("    - {} ({}){}", provider_name, auth_type, status);
        }
        println!();
    }

    Ok(())
}
