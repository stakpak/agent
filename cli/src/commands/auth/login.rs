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
    endpoint: Option<String>,
    region: Option<String>,
    aws_profile_name: Option<String>,
) -> Result<(), String> {
    // Bedrock has its own non-interactive flow (no API key needed)
    if provider == "bedrock" || provider == "amazon-bedrock" {
        return handle_bedrock_setup(config_dir, profile, region, aws_profile_name, endpoint).await;
    }

    // Non-interactive mode when --api-key is provided
    if let Some(key) = api_key {
        return handle_non_interactive_setup(config_dir, provider, profile, key, endpoint).await;
    }

    if endpoint.is_some() {
        let _validated = validate_login_endpoint(endpoint)?;
        eprintln!(
            "Warning: --endpoint is currently applied only in non-interactive mode (--api-key). Ignoring in interactive flow."
        );
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

fn validate_login_endpoint(endpoint: Option<String>) -> Result<Option<String>, String> {
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };

    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return Err("--endpoint cannot be empty".to_string());
    }

    let parsed =
        reqwest::Url::parse(trimmed).map_err(|e| format!("Invalid --endpoint format: {}", e))?;

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(
            "Invalid --endpoint scheme: only http:// or https:// endpoints are supported"
                .to_string(),
        );
    }

    Ok(Some(trimmed.to_string()))
}

/// Handle non-interactive setup with --api-key and --provider flags
/// This initializes config and saves credentials in one step, mirroring interactive setup
async fn handle_non_interactive_setup(
    config_dir: &Path,
    provider_id: &str,
    profile: Option<&str>,
    api_key: String,
    endpoint: Option<String>,
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

    let validated_endpoint = validate_login_endpoint(endpoint)?;

    // Determine profile config based on provider
    let mut profile_config = match provider_id {
        "stakpak" => {
            // Stakpak API key -> Remote provider (key stored in config.toml)
            ProfileConfig {
                provider: Some(ProviderType::Remote),
                api_key: Some(api_key.clone()),
                api_endpoint: validated_endpoint.clone(),
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

    if provider_id != "stakpak"
        && let Some(endpoint) = validated_endpoint
    {
        let provider = profile_config
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| format!("Provider '{}' not found in generated profile", provider_id))?;
        provider.set_api_endpoint(Some(endpoint));
    }

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
    endpoint: Option<String>,
) -> Result<(), String> {
    use crate::config::{ProfileConfig, ProviderType};
    use crate::onboarding::save_config::save_to_profile;
    use stakpak_shared::models::llm::ProviderConfig;

    if endpoint.is_some() {
        let _validated = validate_login_endpoint(endpoint)?;
        eprintln!(
            "Warning: --endpoint is ignored for amazon-bedrock provider (uses AWS regional endpoints)."
        );
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFile;

    fn load_config(config_dir: &Path) -> Result<ConfigFile, String> {
        let config_path = config_dir.join("config.toml");
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read {}: {}", config_path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", config_path.display(), e))
    }

    fn temp_dir() -> tempfile::TempDir {
        match tempfile::TempDir::new() {
            Ok(dir) => dir,
            Err(error) => panic!("failed to create temp dir: {error}"),
        }
    }

    async fn assert_non_interactive_provider_endpoint(provider_id: &str, endpoint: &str) {
        let temp_dir = temp_dir();
        let result = handle_non_interactive_setup(
            temp_dir.path(),
            provider_id,
            Some("default"),
            "test-key".to_string(),
            Some(endpoint.to_string()),
        )
        .await;
        assert!(result.is_ok());

        let config = load_config(temp_dir.path());
        assert!(config.is_ok());

        if let Ok(config) = config {
            let profile = config.profiles.get("default");
            assert!(profile.is_some());
            if let Some(profile) = profile {
                let endpoint_in_config = profile
                    .providers
                    .get(provider_id)
                    .and_then(|provider| provider.api_endpoint());
                assert_eq!(endpoint_in_config, Some(endpoint));
            }
        }
    }

    #[test]
    fn validate_login_endpoint_rejects_invalid_url() {
        let result = validate_login_endpoint(Some("not-a-url".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn validate_login_endpoint_rejects_empty_url() {
        let result = validate_login_endpoint(Some("   ".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn validate_login_endpoint_rejects_unsupported_scheme() {
        let result = validate_login_endpoint(Some("ftp://proxy.example.com".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn validate_login_endpoint_accepts_http_and_https() {
        let http = validate_login_endpoint(Some("http://localhost:4000".to_string()));
        assert!(http.is_ok());

        let https = validate_login_endpoint(Some("https://proxy.example.com/v1".to_string()));
        assert!(https.is_ok());
    }

    #[tokio::test]
    async fn non_interactive_stakpak_sets_profile_api_endpoint() {
        let temp_dir = temp_dir();

        let endpoint = "https://self-hosted.example.com";
        let result = handle_non_interactive_setup(
            temp_dir.path(),
            "stakpak",
            Some("default"),
            "spk-test".to_string(),
            Some(endpoint.to_string()),
        )
        .await;
        assert!(result.is_ok());

        let config = load_config(temp_dir.path());
        assert!(config.is_ok());
        if let Ok(config) = config {
            let profile = config.profiles.get("default");
            assert!(profile.is_some());
            if let Some(profile) = profile {
                assert_eq!(profile.api_endpoint.as_deref(), Some(endpoint));
            }
        }
    }

    #[tokio::test]
    async fn non_interactive_openai_sets_provider_api_endpoint() {
        assert_non_interactive_provider_endpoint("openai", "https://openai-proxy.example.com/v1")
            .await;
    }

    #[tokio::test]
    async fn non_interactive_anthropic_sets_provider_api_endpoint() {
        assert_non_interactive_provider_endpoint(
            "anthropic",
            "https://anthropic-proxy.example.com",
        )
        .await;
    }

    #[tokio::test]
    async fn non_interactive_gemini_sets_provider_api_endpoint() {
        assert_non_interactive_provider_endpoint("gemini", "https://gemini-proxy.example.com")
            .await;
    }

    #[tokio::test]
    async fn bedrock_ignores_valid_url_after_validation() {
        let temp_dir = temp_dir();

        let result = handle_bedrock_setup(
            temp_dir.path(),
            Some("default"),
            Some("us-east-1".to_string()),
            None,
            Some("https://ignored.example.com".to_string()),
        )
        .await;
        assert!(result.is_ok());

        let config = load_config(temp_dir.path());
        assert!(config.is_ok());
        if let Ok(config) = config {
            let profile = config.profiles.get("default");
            assert!(profile.is_some());
            if let Some(profile) = profile {
                let bedrock = profile
                    .providers
                    .get("amazon-bedrock")
                    .and_then(|provider| provider.api_endpoint());
                assert_eq!(bedrock, None);
            }
        }
    }

    #[tokio::test]
    async fn bedrock_rejects_invalid_url_when_provided() {
        let temp_dir = temp_dir();
        let result = handle_bedrock_setup(
            temp_dir.path(),
            Some("default"),
            Some("us-east-1".to_string()),
            None,
            Some("not-a-url".to_string()),
        )
        .await;
        assert!(result.is_err());
    }
}
