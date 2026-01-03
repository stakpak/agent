//! Login command - authenticate with LLM providers

use crate::onboarding::menu::{prompt_password, select_option_no_header};
use crate::onboarding::navigation::NavResult;
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::oauth::{
    AuthMethodType, OAuthFlow, OAuthProvider, ProviderRegistry,
};
use std::io::{self, Write};
use std::path::Path;

/// Handle the login command
pub async fn handle_login(
    config_dir: &Path,
    provider: Option<&str>,
    profile: Option<&str>,
) -> Result<(), String> {
    let profile = profile.unwrap_or("all");
    let registry = ProviderRegistry::new();

    // Select provider if not specified
    let provider_id = match provider {
        Some(p) => p.to_string(),
        None => {
            let providers = registry.list();
            let options: Vec<(String, String, bool)> = providers
                .iter()
                .map(|p| (p.id().to_string(), p.name().to_string(), false))
                .collect();

            let options_refs: Vec<(String, &str, bool)> = options
                .iter()
                .map(|(id, name, recommended)| (id.clone(), name.as_str(), *recommended))
                .collect();

            println!();
            println!("Select provider:");
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

    let provider = registry
        .get(&provider_id)
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?;

    // Select authentication method
    let methods = provider.auth_methods();
    let options: Vec<(String, String, bool)> = methods
        .iter()
        .enumerate()
        .map(|(i, m)| (m.id.clone(), m.display(), i == 0)) // First option is recommended
        .collect();

    let options_refs: Vec<(String, &str, bool)> = options
        .iter()
        .map(|(id, display, recommended)| (id.clone(), display.as_str(), *recommended))
        .collect();

    println!();
    println!("Select authentication method:");
    println!();

    let method_id = match select_option_no_header(&options_refs, true) {
        NavResult::Forward(selected) => selected,
        NavResult::Back | NavResult::Cancel => {
            println!("Cancelled.");
            return Ok(());
        }
    };

    let method = methods
        .iter()
        .find(|m| m.id == method_id)
        .ok_or_else(|| format!("Unknown method: {}", method_id))?;

    match method.method_type {
        AuthMethodType::OAuth => {
            handle_oauth_login(config_dir, provider, &method_id, profile).await
        }
        AuthMethodType::ApiKey => {
            handle_api_key_login(config_dir, provider, profile).await
        }
    }
}

/// Handle OAuth login flow
async fn handle_oauth_login(
    config_dir: &Path,
    provider: &dyn OAuthProvider,
    method_id: &str,
    profile: &str,
) -> Result<(), String> {
    let oauth_config = provider
        .oauth_config(method_id)
        .ok_or("OAuth not supported for this method")?;

    let mut flow = OAuthFlow::new(oauth_config);
    let auth_url = flow.generate_auth_url();

    println!();
    println!("Opening browser for {} authentication...", provider.name());
    println!();
    println!("If browser doesn't open, visit:");
    println!("{}", auth_url);
    println!();

    // Try to open browser
    let _ = open::that(&auth_url);

    // Prompt for authorization code
    print!("Paste the authorization code: ");
    io::stdout().flush().map_err(|e| e.to_string())?;

    let mut code = String::new();
    io::stdin()
        .read_line(&mut code)
        .map_err(|e| format!("Failed to read input: {}", e))?;
    let code = code.trim();

    if code.is_empty() {
        println!("Cancelled.");
        return Ok(());
    }

    println!();
    println!("Exchanging code for tokens...");

    let tokens = flow
        .exchange_code(code)
        .await
        .map_err(|e| format!("Token exchange failed: {}", e))?;

    let auth = provider
        .post_authorize(method_id, &tokens)
        .await
        .map_err(|e| format!("Post-authorization failed: {}", e))?;

    // Save credentials
    let mut auth_manager = AuthManager::new(config_dir)
        .map_err(|e| format!("Failed to load auth manager: {}", e))?;

    auth_manager
        .set(profile, provider.id(), auth)
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    println!();
    println!("Successfully logged in to {}!", provider.name());

    if profile == "all" {
        println!("Credentials saved as shared default (all profiles).");
    } else {
        println!("Credentials saved for profile '{}'.", profile);
    }

    Ok(())
}

/// Handle API key login
async fn handle_api_key_login(
    config_dir: &Path,
    provider: &dyn OAuthProvider,
    profile: &str,
) -> Result<(), String> {
    println!();

    let key = match prompt_password("Enter API key", true) {
        NavResult::Forward(Some(key)) => key,
        NavResult::Forward(None) => {
            println!("API key is required.");
            return Ok(());
        }
        NavResult::Back | NavResult::Cancel => {
            println!("Cancelled.");
            return Ok(());
        }
    };

    let auth = ProviderAuth::api_key(key);

    let mut auth_manager = AuthManager::new(config_dir)
        .map_err(|e| format!("Failed to load auth manager: {}", e))?;

    auth_manager
        .set(profile, provider.id(), auth)
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    println!();
    println!("Successfully saved {} API key!", provider.name());

    if profile == "all" {
        println!("Credentials saved as shared default (all profiles).");
    } else {
        println!("Credentials saved for profile '{}'.", profile);
    }

    Ok(())
}
