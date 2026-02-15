//! Login command - authenticate with LLM providers

use crate::onboarding::menu::{prompt_password, select_option_no_header};
use crate::onboarding::navigation::NavResult;
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::oauth::{AuthMethodType, OAuthFlow, OAuthProvider, ProviderRegistry};
use std::io::{self, Write};
use std::path::Path;

/// Handle the login command
pub async fn handle_login(
    config_dir: &Path,
    provider: &str,
    profile: Option<&str>,
    api_key: Option<String>,
    region: Option<String>,
    aws_profile_name: Option<String>,
) -> Result<(), String> {
    // Bedrock has its own non-interactive flow (no API key needed)
    if provider == "bedrock" || provider == "amazon-bedrock" {
        return handle_bedrock_setup(config_dir, profile, region, aws_profile_name).await;
    }

    // Non-interactive mode when --api-key is provided
    if let Some(key) = api_key {
        return handle_non_interactive_setup(config_dir, provider, profile, key).await;
    }

    // Interactive mode (existing behavior)
    // Select profile if not specified
    let profile = match profile {
        Some(p) => p.to_string(),
        None => select_profile_for_auth(config_dir).await?,
    };

    let registry = ProviderRegistry::new();

    // Always prompt for provider selection in interactive mode
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

    let provider_id = match select_option_no_header(&options_refs, false) {
        NavResult::Forward(selected) => selected,
        NavResult::Back | NavResult::Cancel => {
            println!("Cancelled.");
            return Ok(());
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
            handle_oauth_login(config_dir, provider, &method_id, &profile).await
        }
        AuthMethodType::ApiKey => handle_api_key_login(config_dir, provider, &profile).await,
    }
}

/// Select profile interactively for auth commands
/// Shows: "All profiles (shared)" and existing profiles
async fn select_profile_for_auth(config_dir: &Path) -> Result<String, String> {
    use crate::config::AppConfig;

    // Get available profiles from config
    let config_path = config_dir.join("config.toml");
    let available_profiles = AppConfig::list_available_profiles(Some(&config_path))
        .unwrap_or_else(|_| vec!["default".to_string()]);

    // Build options: "all" (shared) + existing profiles
    let mut options: Vec<(String, String, bool)> = vec![(
        "all".to_string(),
        "All profiles (shared credentials)".to_string(),
        true, // recommended
    )];

    for profile in &available_profiles {
        options.push((profile.clone(), format!("Profile: {}", profile), false));
    }

    let options_refs: Vec<(String, &str, bool)> = options
        .iter()
        .map(|(id, display, recommended)| (id.clone(), display.as_str(), *recommended))
        .collect();

    println!();
    println!("Save credentials to:");
    println!();

    match select_option_no_header(&options_refs, true) {
        NavResult::Forward(selected) => Ok(selected),
        NavResult::Back | NavResult::Cancel => Err("Cancelled.".to_string()),
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
    // Use OSC 8 escape sequence to make the URL clickable in supported terminals
    println!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", auth_url, auth_url);
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
    let mut auth_manager =
        AuthManager::new(config_dir).map_err(|e| format!("Failed to load auth manager: {}", e))?;

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

/// Handle non-interactive setup with --api-key and --provider flags
/// This initializes config and saves credentials in one step, mirroring interactive setup
async fn handle_non_interactive_setup(
    config_dir: &Path,
    provider_id: &str,
    profile: Option<&str>,
    api_key: String,
) -> Result<(), String> {
    use crate::config::{ProfileConfig, ProviderType};
    use crate::onboarding::config_templates::{
        generate_anthropic_profile, generate_gemini_profile, generate_openai_profile,
    };
    use crate::onboarding::save_config::save_to_profile;

    // Default to "default" profile for non-interactive setup
    let profile_name = profile.unwrap_or("default");

    // Ensure config directory exists
    std::fs::create_dir_all(config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    // Determine profile config based on provider
    let profile_config = match provider_id {
        "stakpak" => {
            // Stakpak API key -> Remote provider (key stored in config.toml)
            ProfileConfig {
                provider: Some(ProviderType::Remote),
                api_key: Some(api_key.clone()),
                ..ProfileConfig::default()
            }
        }
        "anthropic" => generate_anthropic_profile(),
        "openai" => generate_openai_profile(),
        "gemini" => generate_gemini_profile(),
        _ => {
            return Err(format!(
                "Unsupported provider '{}'. Supported: anthropic, openai, gemini, stakpak, amazon-bedrock\n\
                 For bedrock, use: stakpak auth login --provider amazon-bedrock --region <region>",
                provider_id
            ));
        }
    };

    // Save API key to auth.toml for local providers (not stakpak)
    if provider_id != "stakpak" {
        let mut auth_manager = AuthManager::new(config_dir)
            .map_err(|e| format!("Failed to load auth manager: {}", e))?;

        let auth = ProviderAuth::api_key(api_key);
        auth_manager
            .set(profile_name, provider_id, auth)
            .map_err(|e| format!("Failed to save credentials: {}", e))?;
    }

    // Save profile config to config.toml (this also creates readonly profile)
    let config_path = config_dir.join("config.toml");
    let config_path_str = config_path
        .to_str()
        .ok_or_else(|| "Invalid config path".to_string())?;

    save_to_profile(config_path_str, profile_name, profile_config)
        .map_err(|e| format!("Failed to save config: {}", e))?;

    println!(
        "Successfully configured {} for profile '{}'.",
        provider_id, profile_name
    );
    println!("Config saved to: {}", config_path.display());
    if provider_id != "stakpak" {
        println!(
            "Credentials saved to: {}",
            config_dir.join("auth.toml").display()
        );
    }

    Ok(())
}

/// Handle Bedrock provider setup
///
/// Unlike other providers, Bedrock does NOT need an API key.
/// Authentication is handled by the AWS credential chain.
/// We only need the region and optionally an AWS named profile.
async fn handle_bedrock_setup(
    config_dir: &Path,
    profile: Option<&str>,
    region: Option<String>,
    aws_profile_name: Option<String>,
) -> Result<(), String> {
    use crate::config::{ProfileConfig, ProviderType};
    use crate::onboarding::save_config::save_to_profile;
    use stakpak_shared::models::llm::ProviderConfig;

    let region = region.unwrap_or_else(|| {
        println!("No --region specified, defaulting to us-east-1");
        "us-east-1".to_string()
    });

    let profile_name = profile.unwrap_or("default");

    // Ensure config directory exists
    std::fs::create_dir_all(config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    // Bedrock uses the same Anthropic models — use friendly aliases
    // that resolve_bedrock_model_id() will map to full Bedrock IDs
    let smart_model = "amazon-bedrock/claude-sonnet-4-5".to_string();
    let eco_model = "amazon-bedrock/claude-haiku-4-5".to_string();

    let mut profile_config = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some(smart_model.clone()),
        smart_model: Some(smart_model),
        eco_model: Some(eco_model),
        ..ProfileConfig::default()
    };

    profile_config.providers.insert(
        "amazon-bedrock".to_string(),
        ProviderConfig::Bedrock {
            region: region.clone(),
            profile_name: aws_profile_name.clone(),
        },
    );

    // Save profile config to config.toml (this also creates readonly profile)
    // NO credentials are saved to auth.toml — AWS credential chain handles auth
    let config_path = config_dir.join("config.toml");
    let config_path_str = config_path
        .to_str()
        .ok_or_else(|| "Invalid config path".to_string())?;

    save_to_profile(config_path_str, profile_name, profile_config)
        .map_err(|e| format!("Failed to save config: {}", e))?;

    println!(
        "Successfully configured Bedrock provider for profile '{}'.",
        profile_name
    );
    println!("Region: {}", region);
    if let Some(ref aws_profile) = aws_profile_name {
        println!("AWS Profile: {}", aws_profile);
    }
    println!("Config saved to: {}", config_path.display());
    println!();
    println!("Authentication uses the AWS credential chain:");
    println!("  1. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)");
    println!("  2. Shared credentials file (~/.aws/credentials)");
    println!("  3. AWS SSO / IAM Identity Center");
    println!("  4. EC2/ECS instance roles");
    println!();
    println!("No AWS credentials are stored by stakpak.");

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

    let mut auth_manager =
        AuthManager::new(config_dir).map_err(|e| format!("Failed to load auth manager: {}", e))?;

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
