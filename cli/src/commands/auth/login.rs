//! Login command - authenticate with LLM providers

use crate::config::AppConfig;
use crate::config::{ProfileConfig, ProviderType};
use crate::onboarding::config_templates::{
    generate_anthropic_profile, generate_gemini_profile, generate_github_copilot_profile,
    generate_openai_profile,
};
use crate::onboarding::menu::{prompt_password, select_option_no_header};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::save_config::save_to_profile;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::models::llm::ProviderConfig;
use stakpak_shared::oauth::{AuthMethodType, OAuthFlow, OAuthProvider, ProviderRegistry};
use std::io::{self, Write};
use std::path::Path;

/// AWS/Bedrock-specific login parameters
pub struct AwsLoginParams {
    pub region: Option<String>,
    pub aws_profile_name: Option<String>,
}

/// OAuth token parameters for non-interactive login
pub struct OAuthTokenParams {
    pub access: Option<String>,
    pub refresh: String,
    pub expiry: i64,
}

/// Handle the login command
pub async fn handle_login(
    config_dir: &Path,
    provider: &str,
    profile: Option<&str>,
    api_key: Option<String>,
    endpoint: Option<String>,
    aws: AwsLoginParams,
    oauth: OAuthTokenParams,
) -> Result<(), String> {
    // Bedrock has its own non-interactive flow (no API key needed)
    if provider == "bedrock" || provider == "amazon-bedrock" {
        return handle_bedrock_setup(
            config_dir,
            profile,
            aws.region,
            aws.aws_profile_name,
            endpoint,
        )
        .await;
    }

    // Non-interactive OAuth setup when --access is provided
    if let Some(access_token) = oauth.access {
        return handle_non_interactive_oauth_setup(
            config_dir,
            provider,
            profile,
            access_token,
            oauth.refresh,
            oauth.expiry,
        )
        .await;
    }

    // Non-interactive mode when --api-key is provided
    if let Some(key) = api_key {
        return handle_non_interactive_api_setup(config_dir, provider, profile, key, endpoint)
            .await;
    }

    if endpoint.is_some() {
        let _validated = validate_login_endpoint(endpoint)?;
        eprintln!(
            "Warning: --endpoint is currently applied only in non-interactive mode (--api-key/--access). Ignoring in interactive flow."
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
        AuthMethodType::DeviceFlow => {
            handle_device_flow_login(config_dir, provider, &method_id, &profile).await
        }
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

/// Handle Device Authorization Grant (RFC 8628) login.
async fn handle_device_flow_login(
    config_dir: &Path,
    provider: &dyn OAuthProvider,
    method_id: &str,
    profile: &str,
) -> Result<(), String> {
    // Step 1: request device code and display instructions to the user.
    let (flow, device_code) = provider
        .request_device_code(method_id)
        .await
        .map_err(|e| format!("Device flow failed: {}", e))?;

    println!();
    println!("To authenticate with {}:", provider.name());
    println!();
    // Use OSC 8 escape sequence for a clickable hyperlink in supported terminals
    println!(
        "  1. Visit: \x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\",
        device_code.verification_uri, device_code.verification_uri
    );
    println!("  2. Enter code: {}", device_code.user_code);
    println!();

    // Try to open the browser automatically
    let _ = open::that(&device_code.verification_uri);

    println!("Waiting for authorisation...");

    // Step 2: poll using the same HTTP client that was built in step 1.
    let token = provider
        .wait_for_token(&flow, &device_code)
        .await
        .map_err(|e| format!("Device flow failed: {}", e))?;

    let auth = provider
        .post_device_authorize(method_id, &token)
        .await
        .map_err(|e| format!("Post-authorization failed: {}", e))?;

    save_auth_to_config(config_dir, provider, profile, auth)
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

    let mut flow = OAuthFlow::new(oauth_config.clone());
    let auth_url = flow.generate_auth_url();

    println!();
    println!("Opening browser for {} authentication...", provider.name());
    println!();
    println!("If browser doesn't open, visit:");
    // Use OSC 8 escape sequence to make the URL clickable in supported terminals
    println!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", auth_url, auth_url);
    println!();

    let callback = if should_wait_for_local_oauth_callback(&oauth_config.redirect_url) {
        println!(
            "Waiting for OAuth callback on {}...",
            oauth_config.redirect_url
        );

        let callback_listener =
            bind_local_oauth_callback_listener(oauth_config.redirect_url.clone()).await?;

        // Try to open browser after the listener is ready.
        let _ = open::that(&auth_url);

        OAuthCallback::FromRedirect(
            callback_listener
                .wait(std::time::Duration::from_secs(300))
                .await?,
        )
    } else {
        // Try to open browser
        let _ = open::that(&auth_url);

        // Prompt for authorization code
        print!("Paste the authorization code: ");
        io::stdout().flush().map_err(|e| e.to_string())?;

        let mut code = String::new();
        io::stdin()
            .read_line(&mut code)
            .map_err(|e| format!("Failed to read input: {}", e))?;
        let code = code.trim().to_string();

        if code.is_empty() {
            println!("Cancelled.");
            return Ok(());
        }

        OAuthCallback::Manual(code)
    };

    println!();
    println!("Exchanging code for tokens...");

    let tokens = match callback {
        OAuthCallback::Manual(code) => flow.exchange_code(&code).await,
        OAuthCallback::FromRedirect(callback) => {
            flow.exchange_code_with_state(&callback.code, &callback.state)
                .await
        }
    }
    .map_err(|e| format!("Token exchange failed: {}", e))?;

    let auth = provider
        .post_authorize(method_id, &tokens)
        .await
        .map_err(|e| format!("Post-authorization failed: {}", e))?;

    save_auth_to_config(config_dir, provider, profile, auth)
}

fn should_wait_for_local_oauth_callback(redirect_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(redirect_url) else {
        return false;
    };

    matches!(url.host_str(), Some("localhost") | Some("127.0.0.1"))
}

enum OAuthCallback {
    Manual(String),
    FromRedirect(LocalOAuthCallback),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalOAuthCallback {
    code: String,
    state: String,
}

struct LocalOAuthCallbackListener {
    callback_rx: tokio::sync::oneshot::Receiver<Result<LocalOAuthCallback, String>>,
    server_task: tokio::task::JoinHandle<()>,
}

impl LocalOAuthCallbackListener {
    async fn wait(
        self,
        timeout_duration: std::time::Duration,
    ) -> Result<LocalOAuthCallback, String> {
        let code = match tokio::time::timeout(timeout_duration, self.callback_rx).await {
            Ok(result) => result.map_err(|_| {
                "OAuth callback listener closed before receiving a response".to_string()
            })?,
            Err(_) => Err(format!(
                "OAuth callback timed out after {} seconds",
                timeout_duration.as_secs()
            )),
        };

        self.server_task.abort();
        let _ = self.server_task.await;

        code
    }
}

async fn bind_local_oauth_callback_listener(
    redirect_url: String,
) -> Result<LocalOAuthCallbackListener, String> {
    let parsed = reqwest::Url::parse(&redirect_url)
        .map_err(|e| format!("Invalid OAuth redirect URL '{}': {}", redirect_url, e))?;
    let host = parsed.host_str().unwrap_or("127.0.0.1");
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| format!("OAuth redirect URL is missing a port: {}", redirect_url))?;
    let bind_addr = format!("{}:{}", host, port);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| {
            format!(
                "Failed to bind OAuth callback listener on {}: {}",
                bind_addr, e
            )
        })?;

    build_local_oauth_callback_listener(redirect_url, listener)
}

fn build_local_oauth_callback_listener(
    redirect_url: String,
    listener: tokio::net::TcpListener,
) -> Result<LocalOAuthCallbackListener, String> {
    let parsed = reqwest::Url::parse(&redirect_url)
        .map_err(|e| format!("Invalid OAuth redirect URL '{}': {}", redirect_url, e))?;
    let path = parsed.path().to_string();

    let (code_tx, code_rx) = tokio::sync::oneshot::channel::<Result<LocalOAuthCallback, String>>();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let code_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(code_tx)));
    let shutdown_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(shutdown_tx)));

    let build_callback_route = || {
        let code_tx = code_tx.clone();
        let shutdown_tx = shutdown_tx.clone();
        axum::routing::get(
            move |axum::extract::Query(query): axum::extract::Query<
                std::collections::HashMap<String, String>,
            >| {
                let code_tx = code_tx.clone();
                let shutdown_tx = shutdown_tx.clone();
                async move {
                    let result = if let Some(error) = query.get("error") {
                        Err(format!(
                            "OAuth callback returned '{}'{}",
                            error,
                            query
                                .get("error_description")
                                .map(|description| format!(": {}", description))
                                .unwrap_or_default()
                        ))
                    } else {
                        match (query.get("code"), query.get("state")) {
                            (Some(code), Some(state)) => Ok(LocalOAuthCallback {
                                code: code.clone(),
                                state: state.clone(),
                            }),
                            _ => Err("OAuth callback missing code or state".to_string()),
                        }
                    };

                    if let Ok(mut sender) = code_tx.lock()
                        && let Some(sender) = sender.take()
                    {
                        let _ = sender.send(result.clone());
                    }

                    if let Ok(mut sender) = shutdown_tx.lock()
                        && let Some(sender) = sender.take()
                    {
                        let _ = sender.send(());
                    }

                    let (body, status) = match result {
                        Ok(_) => (
                            "Authentication complete. You can return to Stakpak.",
                            axum::http::StatusCode::OK,
                        ),
                        Err(_) => (
                            "Authentication failed. You can return to Stakpak for details.",
                            axum::http::StatusCode::BAD_REQUEST,
                        ),
                    };

                    (status, axum::response::Html(body.to_string()))
                }
            },
        )
    };

    let mut app = axum::Router::new().route(&path, build_callback_route());
    if path != "/" {
        app = app.route("/", build_callback_route());
    }
    if path != "/callback" {
        app = app.route("/callback", build_callback_route());
    }

    let server =
        axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });

    let server_task = tokio::spawn(async move {
        if let Err(error) = server.await {
            tracing::debug!("OAuth callback server exited: {}", error);
        }
    });

    Ok(LocalOAuthCallbackListener {
        callback_rx: code_rx,
        server_task,
    })
}

/// Persist a `ProviderAuth` into the config file for the given profile.
///
/// Shared by all login flows (OAuth, device flow, API key).  Handles
/// creating the provider config entry if it doesn't exist yet, syncing the
/// readonly profile, saving to disk, and printing the success message.
fn save_auth_to_config(
    config_dir: &Path,
    provider: &dyn OAuthProvider,
    profile: &str,
    auth: ProviderAuth,
) -> Result<(), String> {
    let config_path = config_dir.join("config.toml");
    let mut config_file = AppConfig::load_config_file(&config_path)
        .map_err(|e| format!("Failed to load config file: {}", e))?;

    let profile_config = config_file.profiles.entry(profile.to_string()).or_default();

    let provider_config = profile_config
        .providers
        .entry(provider.id().to_string())
        .or_insert_with(|| {
            ProviderConfig::empty_for_provider(provider.id()).unwrap_or(ProviderConfig::Anthropic {
                api_key: None,
                api_endpoint: None,
                access_token: None,
                auth: None,
            })
        });

    provider_config.set_auth(auth);

    // Keep readonly profile in sync when modifying the default profile
    if profile == "default" {
        config_file.update_readonly();
    }

    config_file
        .save_to(&config_path)
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    println!();
    println!("Successfully logged in to {}!", provider.name());

    if profile == "all" {
        println!("Credentials saved as shared default (all profiles).");
    } else {
        println!("Credentials saved for profile '{}'.", profile);
    }
    println!("Config saved to: {}", config_path.display());

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
async fn handle_non_interactive_api_setup(
    config_dir: &Path,
    provider_id: &str,
    profile: Option<&str>,
    api_key: String,
    endpoint: Option<String>,
) -> Result<(), String> {
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
        "github-copilot" => {
            return Err("GitHub Copilot does not support API key authentication.\n\
                It requires OAuth via your GitHub account.\n\n\
                Authenticate using one of the following:\n\
                • Interactive login:\n\
                stakpak auth login --provider github-copilot\n\n\
                • Provide an OAuth access token:\n\
                stakpak auth login --access <ACESS_TOKEN>"
                .to_string());
        }
        _ => {
            return Err(format!(
                "Unsupported provider '{}'. Supported: anthropic, openai, gemini, stakpak, amazon-bedrock, github-copilot\n\
                 For bedrock, use: stakpak auth login --provider amazon-bedrock --region <region>\n\
                 For github-copilot, run without --api-key to use the device flow.",
                provider_id
            ));
        }
    };

    // Set endpoint if provided
    if provider_id != "stakpak"
        && let Some(ref endpoint) = validated_endpoint
    {
        let provider = profile_config
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| format!("Provider '{}' not found in generated profile", provider_id))?;
        provider.set_api_endpoint(Some(endpoint.clone()));
    }

    // Save API key to provider config in config.toml (not auth.toml)
    if provider_id != "stakpak" {
        let auth = ProviderAuth::api_key(api_key);
        let provider = profile_config
            .providers
            .get_mut(provider_id)
            .ok_or_else(|| format!("Provider '{}' not found in generated profile", provider_id))?;
        provider.set_auth(auth);
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

    Ok(())
}

/// Handle non-interactive OAuth setup with --access, --refresh, --expiry.
///
/// Works for any provider that supports OAuth (`ProviderAuth::OAuth`).
async fn handle_non_interactive_oauth_setup(
    config_dir: &Path,
    provider_id: &str,
    profile: Option<&str>,
    access_token: String,
    refresh_token: String,
    expiry: i64,
) -> Result<(), String> {
    let profile_name = profile.unwrap_or("default");

    std::fs::create_dir_all(config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    let auth = ProviderAuth::OAuth {
        access: access_token,
        refresh: refresh_token,
        expires: expiry,
        name: None,
    };

    let mut profile_config = match provider_id {
        "anthropic" => generate_anthropic_profile(),
        "openai" => generate_openai_profile(),
        "gemini" => generate_gemini_profile(),
        "github-copilot" => generate_github_copilot_profile(),
        _ => {
            return Err(format!(
                "Unsupported provider '{}' for OAuth non-interactive setup. \
                 Supported: anthropic, openai, gemini, github-copilot",
                provider_id
            ));
        }
    };

    let provider = profile_config
        .providers
        .get_mut(provider_id)
        .ok_or_else(|| format!("Provider '{}' not found in generated profile", provider_id))?;
    provider.set_auth(auth);

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
    let default_model = "amazon-bedrock/claude-sonnet-4-5".to_string();

    let mut profile_config = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some(default_model),
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
    use crate::config::AppConfig;
    use stakpak_shared::models::llm::ProviderConfig;

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

    // Load config using the standard pipeline (handles migrations, old formats, etc.)
    let config_path = config_dir.join("config.toml");
    let mut config_file = AppConfig::load_config_file(&config_path)
        .map_err(|e| format!("Failed to load config file: {}", e))?;

    // Get or create profile
    let profile_config = config_file.profiles.entry(profile.to_string()).or_default();

    // Get or create provider config
    let provider_config = profile_config
        .providers
        .entry(provider.id().to_string())
        .or_insert_with(|| {
            ProviderConfig::empty_for_provider(provider.id()).unwrap_or(ProviderConfig::OpenAI {
                api_key: None,
                api_endpoint: None,
                auth: None,
            })
        });

    // Set auth on provider config
    provider_config.set_auth(auth);

    // Keep readonly profile in sync when modifying the default profile
    if profile == "default" {
        config_file.update_readonly();
    }

    // Save config file
    config_file
        .save_to(&config_path)
        .map_err(|e| format!("Failed to save credentials: {}", e))?;

    println!();
    println!("Successfully saved {} API key!", provider.name());

    if profile == "all" {
        println!("Credentials saved as shared default (all profiles).");
    } else {
        println!("Credentials saved for profile '{}'.", profile);
    }
    println!("Config saved to: {}", config_path.display());

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
        let result = handle_non_interactive_api_setup(
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
        let result = handle_non_interactive_api_setup(
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

    #[test]
    fn should_wait_for_local_oauth_callback_detects_localhost_redirects() {
        assert!(should_wait_for_local_oauth_callback(
            "http://localhost:1455/callback"
        ));
        assert!(should_wait_for_local_oauth_callback(
            "http://127.0.0.1:1455/callback"
        ));
        assert!(!should_wait_for_local_oauth_callback(
            "https://console.anthropic.com/oauth/code/callback"
        ));
    }

    fn reserve_local_callback_listener(
        redirect_url: &str,
    ) -> Result<(String, LocalOAuthCallbackListener), String> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("bind temp listener: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("local addr: {error}"))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("set nonblocking: {error}"))?;
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|error| format!("tokio listener: {error}"))?;
        let redirect_url = redirect_url.replace("{port}", &port.to_string());
        let callback_listener =
            build_local_oauth_callback_listener(redirect_url.clone(), listener)?;
        Ok((redirect_url, callback_listener))
    }

    #[tokio::test]
    async fn local_oauth_callback_listener_is_ready_before_request() {
        let (redirect_url, callback_listener) =
            reserve_local_callback_listener("http://127.0.0.1:{port}/callback")
                .expect("reserve callback listener");
        let callback_url = format!("{redirect_url}?code=test-code&state=test-state");

        let response = reqwest::get(callback_url)
            .await
            .expect("send callback request");
        assert!(response.status().is_success());

        let callback = callback_listener
            .wait(std::time::Duration::from_secs(1))
            .await
            .expect("oauth callback");
        assert_eq!(callback.code, "test-code");
        assert_eq!(callback.state, "test-state");
    }

    #[tokio::test]
    async fn local_oauth_callback_listener_accepts_auth_callback_path() {
        let (redirect_url, callback_listener) =
            reserve_local_callback_listener("http://127.0.0.1:{port}/auth/callback")
                .expect("reserve callback listener");
        let callback_url = format!("{redirect_url}?code=test-code&state=test-state");

        let response = reqwest::get(callback_url)
            .await
            .expect("send callback request");
        assert!(response.status().is_success());

        let callback = callback_listener
            .wait(std::time::Duration::from_secs(1))
            .await
            .expect("oauth callback");
        assert_eq!(callback.code, "test-code");
        assert_eq!(callback.state, "test-state");
    }

    #[tokio::test]
    async fn local_oauth_callback_listener_accepts_callback_path_when_redirect_uses_root() {
        let (redirect_url, callback_listener) =
            reserve_local_callback_listener("http://127.0.0.1:{port}")
                .expect("reserve callback listener");
        let callback_url = format!("{redirect_url}/callback?code=test-code&state=test-state");

        let response = reqwest::get(callback_url)
            .await
            .expect("send callback request");
        assert!(response.status().is_success());

        let callback = callback_listener
            .wait(std::time::Duration::from_secs(1))
            .await
            .expect("oauth callback");
        assert_eq!(callback.code, "test-code");
        assert_eq!(callback.state, "test-state");
    }

    #[tokio::test]
    async fn local_oauth_callback_listener_times_out() {
        let (_redirect_url, callback_listener) =
            reserve_local_callback_listener("http://127.0.0.1:{port}/callback")
                .expect("reserve callback listener");

        let error = callback_listener
            .wait(std::time::Duration::from_millis(10))
            .await
            .expect_err("listener should time out");
        assert!(error.contains("timed out"));
    }
}
