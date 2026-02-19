//! Onboarding flow for first-time users
//!
//! This module provides a styled, interactive onboarding experience that guides users
//! through setting up their Stakpak configuration, including:
//! - Stakpak API authentication (OAuth flow)
//! - Provider selection (OpenAI, Anthropic, Google, Hybrid Providers, BYOM)
//! - Bring Your Own Model (BYOM) configuration
//! - Hybrid provider configurations (mixing providers)

mod byom;
pub mod config_templates;
pub mod menu;
pub mod navigation;
pub mod save_config;
mod styled_output;

use crate::apikey_auth::prompt_for_api_key;
use crate::config::AppConfig;
use crate::onboarding::byom::configure_byom;
use crate::onboarding::config_templates::{
    BuiltinProvider, DEFAULT_MODEL, ProviderSetup, config_to_toml_preview,
    generate_anthropic_profile, generate_gemini_profile, generate_multi_provider_profile,
    generate_openai_profile,
};
use crate::onboarding::menu::{
    prompt_password, prompt_profile_name, select_option, select_option_no_header,
};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::save_config::{preview_and_save_to_profile, save_to_profile};
use crate::onboarding::styled_output::{StepStatus, render_profile_name};
use stakpak_shared::oauth::{OAuthFlow, ProviderRegistry};
use std::io::{self, Write};

fn get_config_path_string(config: &AppConfig) -> String {
    if config.config_path.is_empty() {
        AppConfig::get_config_path::<&str>(None)
            .display()
            .to_string()
    } else {
        config.config_path.clone()
    }
}

/// Onboarding mode
pub enum OnboardingMode {
    /// Default onboarding for existing/default profile
    Default,
    /// Creating a new profile
    New,
}

/// Main onboarding flow entry point
pub async fn run_onboarding(config: &mut AppConfig, mode: OnboardingMode) {
    let profile_name = match mode {
        OnboardingMode::Default => {
            let profile = config.profile_name.clone();

            print!("\r\n");
            crate::onboarding::styled_output::render_title("Welcome to Stakpak");
            print!("\r\n");
            render_profile_name(&profile);
            print!("\r\n");
            crate::onboarding::styled_output::render_info(
                "Configuring stakpak. You can connect to Stakpak API or use your own model/API keys.",
            );
            print!("\r\n");

            profile
        }
        OnboardingMode::New => {
            print!("\r\n");
            crate::onboarding::styled_output::render_title("Creating new profile");
            print!("\r\n");

            let config_path = get_config_path_string(config);
            let custom_path = if config_path.is_empty() {
                None
            } else {
                Some(config_path.as_str())
            };

            let profile_name_result = prompt_profile_name(custom_path);
            let profile_name = match profile_name_result {
                NavResult::Forward(Some(name)) => name,
                NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => {
                    crate::onboarding::styled_output::render_warning("Profile creation cancelled.");
                    return;
                }
            };

            print!("\x1b[2A");
            print!("\x1b[0J");
            print!("\r\n");
            render_profile_name(&profile_name);
            print!("\r\n");
            crate::onboarding::styled_output::render_info(
                "Configuring stakpak. You can connect to Stakpak API or use your own model/API keys.",
            );
            print!("\r\n");

            profile_name
        }
    };

    print!("\x1b[s");

    // Initial decision: Stakpak API or Own Keys
    loop {
        print!("\x1b[u");
        print!("\x1b[0J");
        print!("\x1b[K");
        let _ = io::stdout().flush();
        print!("\x1b[s");

        let initial_choice = select_option(
            "Choose authentication method",
            &[
                (
                    InitialChoice::StakpakAPI,
                    "Use Stakpak API (recommended)",
                    true,
                ),
                (
                    InitialChoice::OwnKeys,
                    "Use my own Model/API Key (or Claude Pro/Max Subscription)",
                    false,
                ),
            ],
            0,
            2,
            false, // Can't go back from first step
        );

        match initial_choice {
            NavResult::Forward(InitialChoice::StakpakAPI) => {
                print!("\x1b[u");
                print!("\x1b[0J");
                let _ = io::stdout().flush();

                match mode {
                    OnboardingMode::Default => {
                        prompt_for_api_key(config).await;
                    }
                    OnboardingMode::New => {
                        handle_stakpak_api_for_new_profile(config, &profile_name).await;
                    }
                }
                break;
            }
            NavResult::Forward(InitialChoice::OwnKeys) => {
                print!("\x1b[s");
                if handle_own_keys_flow(config, &profile_name).await {
                    break;
                }
                continue;
            }
            NavResult::Back => {
                break;
            }
            NavResult::Cancel => {
                print!("\x1b[u");
                print!("\x1b[0J");
                print!("\r\n");
                crate::onboarding::styled_output::render_warning(
                    "Onboarding cancelled. You can run this again later.",
                );
                print!("\r\n");
                break;
            }
        }
    }

    // Update config with the new profile name so callers can use it
    config.profile_name = profile_name;
}

async fn handle_own_keys_flow(config: &mut AppConfig, profile_name: &str) -> bool {
    let mut disclaimer_shown = false;

    loop {
        print!("\x1b[u");
        print!("\x1b[0J");
        print!("\x1b[K");
        let _ = io::stdout().flush();
        print!("\x1b[s");

        if !disclaimer_shown {
            crate::onboarding::styled_output::render_telemetry_disclaimer();
            disclaimer_shown = true;
        }

        let provider_choice = select_option(
            "Choose provider or bring your own model",
            &[
                (ProviderChoice::OpenAI, "OpenAI", false),
                (ProviderChoice::Anthropic, "Anthropic", false),
                (ProviderChoice::Google, "Google", false),
                (
                    ProviderChoice::Hybrid,
                    "Hybrid providers (e.g., Google and Anthropic)",
                    false,
                ),
                (ProviderChoice::Byom, "Bring your own model", false),
            ],
            1,
            3,
            true, // Can go back to previous step
        );

        let completed = match provider_choice {
            NavResult::Forward(ProviderChoice::Anthropic) => {
                handle_anthropic_provider_selection(config, profile_name).await
            }
            NavResult::Forward(ProviderChoice::OpenAI) => {
                handle_openai_setup(config, profile_name).await
            }
            NavResult::Forward(ProviderChoice::Google) => {
                handle_gemini_setup(config, profile_name).await
            }
            NavResult::Forward(ProviderChoice::Hybrid) => {
                handle_hybrid_setup(config, profile_name).await
            }
            NavResult::Forward(ProviderChoice::Byom) => {
                handle_byom_setup(config, profile_name).await
            }
            NavResult::Back => {
                print!("\x1b[u");
                print!("\x1b[0J");
                print!("\x1b[K");
                let _ = io::stdout().flush();
                print!("\x1b[s");
                return false;
            }
            NavResult::Cancel => {
                return false;
            }
        };

        if completed {
            return true;
        }
    }
}

/// Handle Anthropic provider selection (subscription vs API key)
/// Returns true if completed, false if cancelled/back
async fn handle_anthropic_provider_selection(config: &mut AppConfig, profile_name: &str) -> bool {
    // Clear previous step content
    print!("\x1b[u");
    print!("\x1b[0J");
    print!("\x1b[K");
    let _ = io::stdout().flush();
    print!("\x1b[s");

    // Show sub-menu for Anthropic auth method
    let auth_choice = select_option(
        "Choose Anthropic authentication method",
        &[
            (
                AnthropicAuthChoice::ClaudeSubscription,
                "Claude Pro/Max Subscription",
                false,
            ),
            (AnthropicAuthChoice::ApiKey, "API Key", false),
        ],
        2,
        4,
        true, // Can go back
    );

    match auth_choice {
        NavResult::Forward(AnthropicAuthChoice::ClaudeSubscription) => {
            handle_claude_subscription_setup(config, profile_name).await
        }
        NavResult::Forward(AnthropicAuthChoice::ApiKey) => {
            handle_anthropic_api_key_setup(config, profile_name).await
        }
        NavResult::Back => {
            // Go back to provider selection
            print!("\x1b[u");
            print!("\x1b[0J");
            print!("\x1b[K");
            let _ = io::stdout().flush();
            false
        }
        NavResult::Cancel => false,
    }
}

/// Handle Claude Pro/Max Subscription setup via OAuth
/// Returns true if completed, false if cancelled/back
async fn handle_claude_subscription_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    // Clear previous step content WITHOUT touching welcome message
    print!("\x1b[u");
    print!("\x1b[0J"); // Clear from cursor to end of screen
    print!("\x1b[K"); // Clear current line
    let _ = io::stdout().flush();

    // Render step 3 - start immediately after welcome message (no extra newline)
    crate::onboarding::styled_output::render_title("Claude Pro/Max Subscription");
    print!("\r\n");

    // Show step indicators on one line
    let steps = vec![
        ("Step 1".to_string(), StepStatus::Completed),
        ("Step 2".to_string(), StepStatus::Completed),
        ("Step 3".to_string(), StepStatus::Active),
    ];
    crate::onboarding::styled_output::render_steps(&steps);
    print!("\r\n");

    // Show info about Claude subscription
    crate::onboarding::styled_output::render_info(
        "Use your existing Claude Pro or Max subscription to access Claude models.",
    );
    print!("\r\n");

    // Show default model
    crate::onboarding::styled_output::render_default_model(DEFAULT_MODEL);

    // Get OAuth provider and config
    let registry = ProviderRegistry::new();
    let provider = match registry.get("anthropic") {
        Some(p) => p,
        None => {
            crate::onboarding::styled_output::render_error("Anthropic provider not found");
            return false;
        }
    };

    let oauth_config = match provider.oauth_config("claude-max") {
        Some(c) => c,
        None => {
            crate::onboarding::styled_output::render_error("OAuth not supported for this method");
            return false;
        }
    };

    // Start OAuth flow
    let mut flow = OAuthFlow::new(oauth_config);
    let auth_url = flow.generate_auth_url();

    print!("\r\n");
    crate::onboarding::styled_output::render_info("Opening browser for authentication...");
    print!("\r\n");
    println!("If browser doesn't open, visit:");
    // Use OSC 8 escape sequence to make the URL clickable in supported terminals
    println!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", auth_url, auth_url);
    print!("\r\n");

    // Try to open browser
    let _ = open::that(&auth_url);

    // Prompt for authorization code
    print!("Paste the authorization code: ");
    io::stdout().flush().ok();

    let mut code = String::new();
    if io::stdin().read_line(&mut code).is_err() {
        crate::onboarding::styled_output::render_error("Failed to read input");
        return false;
    }
    let code = code.trim();

    if code.is_empty() {
        crate::onboarding::styled_output::render_warning("Authentication cancelled.");
        return false;
    }

    print!("\r\n");
    crate::onboarding::styled_output::render_info("Exchanging code for tokens...");

    // Exchange code for tokens
    let tokens = match flow.exchange_code(code).await {
        Ok(t) => t,
        Err(e) => {
            print!("\r\n");
            crate::onboarding::styled_output::render_error(&format!(
                "Token exchange failed: {}",
                e
            ));
            return false;
        }
    };

    // Post-authorize to get final auth
    let auth = match provider.post_authorize("claude-max", &tokens).await {
        Ok(a) => a,
        Err(e) => {
            print!("\r\n");
            crate::onboarding::styled_output::render_error(&format!(
                "Post-authorization failed: {}",
                e
            ));
            return false;
        }
    };

    // Generate profile config with auth credentials embedded
    let mut profile = generate_anthropic_profile();

    // Set auth on the anthropic provider config
    if let Some(provider_config) = profile.providers.get_mut("anthropic") {
        provider_config.set_auth(auth);
    }

    // Save profile config
    let config_path = get_config_path_string(config);
    let telemetry = match save_to_profile(&config_path, profile_name, profile.clone()) {
        Ok(t) => t,
        Err(e) => {
            crate::onboarding::styled_output::render_error(&format!(
                "Failed to save configuration: {}",
                e
            ));
            std::process::exit(1);
        }
    };

    print!("\r\n");
    crate::onboarding::styled_output::render_success("Successfully logged in to Claude!");
    print!("\r\n");
    crate::onboarding::styled_output::render_success("Configuration saved successfully");
    print!("\r\n");

    // Update AppConfig with saved values so we can use them immediately
    config.provider = profile
        .provider
        .unwrap_or(crate::config::ProviderType::Local);
    config.providers = profile.providers.clone();
    config.model = profile.model.clone();
    config.anonymous_id = telemetry.anonymous_id;
    config.collect_telemetry = telemetry.collect_telemetry;

    true
}

/// Handle OpenAI setup
/// Returns true if completed, false if cancelled/back
async fn handle_openai_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    // Clear previous step content WITHOUT touching welcome message
    print!("\x1b[u");
    print!("\x1b[0J"); // Clear from cursor to end of screen
    print!("\x1b[K"); // Clear current line
    let _ = io::stdout().flush();

    // Render step 3 - start immediately after welcome message (no extra newline)
    crate::onboarding::styled_output::render_title("OpenAI Configuration");
    print!("\r\n");

    // Show step indicators on one line
    let steps = vec![
        ("Step 1".to_string(), StepStatus::Completed),
        ("Step 2".to_string(), StepStatus::Completed),
        ("Step 3".to_string(), StepStatus::Active),
    ];
    crate::onboarding::styled_output::render_steps(&steps);
    print!("\r\n");

    // Show default model
    crate::onboarding::styled_output::render_default_model("gpt-4.1");

    match prompt_password("Enter your OpenAI API key", true) {
        NavResult::Forward(Some(api_key)) => {
            // Generate profile config with auth credentials embedded
            let mut profile = generate_openai_profile();
            let auth = stakpak_shared::models::auth::ProviderAuth::api_key(&api_key);
            if let Some(provider_config) = profile.providers.get_mut("openai") {
                provider_config.set_auth(auth);
            }

            crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
                &profile,
                profile_name,
            ));

            match crate::onboarding::menu::prompt_yes_no("Proceed with this configuration?", true) {
                NavResult::Forward(Some(true)) | NavResult::Forward(None) => {
                    let config_path = get_config_path_string(config);

                    let telemetry =
                        match save_to_profile(&config_path, profile_name, profile.clone()) {
                            Ok(t) => t,
                            Err(e) => {
                                crate::onboarding::styled_output::render_error(&format!(
                                    "Failed to save configuration: {}",
                                    e
                                ));
                                std::process::exit(1);
                            }
                        };

                    print!("\r\n");
                    crate::onboarding::styled_output::render_success(
                        "✓ Configuration saved successfully",
                    );
                    print!("\r\n");

                    config.provider = profile
                        .provider
                        .unwrap_or(crate::config::ProviderType::Local);
                    config.providers = profile.providers.clone();
                    config.model = profile.model.clone();
                    config.anonymous_id = telemetry.anonymous_id;
                    config.collect_telemetry = telemetry.collect_telemetry;

                    true
                }
                NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => false,
            }
        }
        NavResult::Back => false,
        NavResult::Forward(None) | NavResult::Cancel => false,
    }
}

/// Handle Gemini setup
/// Returns true if completed, false if cancelled/back
async fn handle_gemini_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    // Clear previous step content WITHOUT touching welcome message
    print!("\x1b[u");
    print!("\x1b[0J"); // Clear from cursor to end of screen
    print!("\x1b[K"); // Clear current line
    let _ = io::stdout().flush();

    // Render step 3 - start immediately after welcome message (no extra newline)
    crate::onboarding::styled_output::render_title("Gemini Configuration");
    print!("\r\n");

    // Show step indicators on one line
    let steps = vec![
        ("Step 1".to_string(), StepStatus::Completed),
        ("Step 2".to_string(), StepStatus::Completed),
        ("Step 3".to_string(), StepStatus::Active),
    ];
    crate::onboarding::styled_output::render_steps(&steps);
    print!("\r\n");

    // Show default model
    crate::onboarding::styled_output::render_default_model("gemini-2.5-pro");

    match prompt_password("Enter your Gemini API key", true) {
        NavResult::Forward(Some(api_key)) => {
            // Generate profile config with auth credentials embedded
            let mut profile = generate_gemini_profile();
            let auth = stakpak_shared::models::auth::ProviderAuth::api_key(&api_key);
            if let Some(provider_config) = profile.providers.get_mut("gemini") {
                provider_config.set_auth(auth);
            }

            // Show confirmation
            crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
                &profile,
                profile_name,
            ));

            match crate::onboarding::menu::prompt_yes_no("Proceed with this configuration?", true) {
                NavResult::Forward(Some(true)) | NavResult::Forward(None) => {
                    let config_path = get_config_path_string(config);

                    let telemetry =
                        match save_to_profile(&config_path, profile_name, profile.clone()) {
                            Ok(t) => t,
                            Err(e) => {
                                crate::onboarding::styled_output::render_error(&format!(
                                    "Failed to save configuration: {}",
                                    e
                                ));
                                std::process::exit(1);
                            }
                        };

                    print!("\r\n");
                    crate::onboarding::styled_output::render_success(
                        "✓ Configuration saved successfully",
                    );
                    print!("\r\n");

                    // Update AppConfig with saved values so we can use them immediately
                    config.provider = profile
                        .provider
                        .unwrap_or(crate::config::ProviderType::Local);
                    config.providers = profile.providers.clone();
                    config.model = profile.model.clone();
                    config.anonymous_id = telemetry.anonymous_id;
                    config.collect_telemetry = telemetry.collect_telemetry;

                    true
                }
                NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => false,
            }
        }
        NavResult::Back => false,
        NavResult::Forward(None) | NavResult::Cancel => false,
    }
}

/// Handle Anthropic API key setup
/// Returns true if completed, false if cancelled/back
async fn handle_anthropic_api_key_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    // Clear previous step content WITHOUT touching welcome message
    print!("\x1b[u");
    print!("\x1b[0J"); // Clear from cursor to end of screen
    print!("\x1b[K"); // Clear current line
    let _ = io::stdout().flush();

    // Render step 4 - API key configuration
    crate::onboarding::styled_output::render_title("Anthropic API Key Configuration");
    print!("\r\n");

    // Show step indicators on one line
    let steps = vec![
        ("Step 1".to_string(), StepStatus::Completed),
        ("Step 2".to_string(), StepStatus::Completed),
        ("Step 3".to_string(), StepStatus::Completed),
        ("Step 4".to_string(), StepStatus::Active),
    ];
    crate::onboarding::styled_output::render_steps(&steps);
    print!("\r\n");

    // Show default model
    crate::onboarding::styled_output::render_default_model(DEFAULT_MODEL);

    match prompt_password("Enter your Anthropic API key", true) {
        NavResult::Forward(Some(api_key)) => {
            // Generate profile config with auth credentials embedded
            let mut profile = generate_anthropic_profile();
            let auth = stakpak_shared::models::auth::ProviderAuth::api_key(&api_key);
            if let Some(provider_config) = profile.providers.get_mut("anthropic") {
                provider_config.set_auth(auth);
            }

            // Show confirmation
            crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
                &profile,
                profile_name,
            ));

            match crate::onboarding::menu::prompt_yes_no("Proceed with this configuration?", true) {
                NavResult::Forward(Some(true)) | NavResult::Forward(None) => {
                    let config_path = get_config_path_string(config);

                    let telemetry =
                        match save_to_profile(&config_path, profile_name, profile.clone()) {
                            Ok(t) => t,
                            Err(e) => {
                                crate::onboarding::styled_output::render_error(&format!(
                                    "Failed to save configuration: {}",
                                    e
                                ));
                                std::process::exit(1);
                            }
                        };

                    print!("\r\n");
                    crate::onboarding::styled_output::render_success(
                        "✓ Configuration saved successfully",
                    );
                    print!("\r\n");

                    // Update AppConfig with saved values so we can use them immediately
                    config.provider = profile
                        .provider
                        .unwrap_or(crate::config::ProviderType::Local);
                    config.providers = profile.providers.clone();
                    config.model = profile.model.clone();
                    config.anonymous_id = telemetry.anonymous_id;
                    config.collect_telemetry = telemetry.collect_telemetry;

                    true
                }
                NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => false,
            }
        }
        NavResult::Back => false,
        NavResult::Forward(None) | NavResult::Cancel => false,
    }
}

/// Handle Multi-Provider setup (formerly Hybrid Providers)
/// Returns true if completed, false if cancelled/back
async fn handle_hybrid_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    let config_path = get_config_path_string(config);

    // Clear previous step content WITHOUT touching welcome message
    print!("\x1b[u");
    print!("\x1b[0J"); // Clear from cursor to end of screen
    print!("\x1b[K"); // Clear current line
    let _ = io::stdout().flush();

    // Render step 3 - start immediately after welcome message (no extra newline)
    crate::onboarding::styled_output::render_title("Multi-Provider Configuration");
    print!("\r\n");

    // Show step indicators on one line
    let steps = vec![
        ("Step 1".to_string(), StepStatus::Completed),
        ("Step 2".to_string(), StepStatus::Completed),
        ("Step 3".to_string(), StepStatus::Active),
    ];
    crate::onboarding::styled_output::render_steps(&steps);
    print!("\r\n");

    crate::onboarding::styled_output::render_info(
        "Configure multiple providers so you can switch between models at runtime using /model.",
    );
    print!("\r\n");

    // Collect providers
    let mut providers: Vec<ProviderSetup> = Vec::new();
    let available_providers = [
        (BuiltinProvider::Anthropic, "Anthropic (recommended)", true),
        (BuiltinProvider::OpenAI, "OpenAI", false),
        (BuiltinProvider::Gemini, "Gemini", false),
    ];

    loop {
        crate::onboarding::styled_output::render_subtitle("Add a provider");

        // Filter out already configured providers
        let remaining: Vec<_> = available_providers
            .iter()
            .filter(|(p, _, _)| !providers.iter().any(|setup| setup.provider == *p))
            .cloned()
            .collect();

        if remaining.is_empty() {
            crate::onboarding::styled_output::render_info("All providers configured!");
            break;
        }

        let provider = match select_option_no_header(&remaining, true) {
            NavResult::Forward(p) => p,
            NavResult::Back | NavResult::Cancel => {
                if providers.is_empty() {
                    return false;
                }
                break;
            }
        };

        // Ask for API key
        let api_key = match crate::onboarding::menu::prompt_password(
            &format!("Enter {} API key", provider.display_name()),
            true,
        ) {
            NavResult::Forward(Some(key)) => key,
            NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => continue,
        };

        providers.push(ProviderSetup { provider, api_key });

        // Ask if they want to add more
        if remaining.len() > 1 {
            match crate::onboarding::menu::prompt_yes_no("Add another provider?", false) {
                NavResult::Forward(Some(true)) => continue,
                _ => break,
            }
        } else {
            break;
        }
    }

    if providers.is_empty() {
        crate::onboarding::styled_output::render_warning("No providers configured.");
        return false;
    }

    // Ask for default model
    print!("\r\n");
    crate::onboarding::styled_output::render_subtitle("Select default model");
    crate::onboarding::styled_output::render_info(
        "You can switch models at runtime using /model command.",
    );

    let default_model = providers
        .first()
        .map(|p| p.provider.default_model().to_string())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let profile = generate_multi_provider_profile(providers, default_model);

    // Show confirmation
    crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
        &profile,
        profile_name,
    ));

    match crate::onboarding::menu::prompt_yes_no("Proceed with this configuration?", true) {
        NavResult::Forward(Some(true)) | NavResult::Forward(None) => {
            let telemetry = match save_to_profile(&config_path, profile_name, profile.clone()) {
                Ok(t) => t,
                Err(e) => {
                    crate::onboarding::styled_output::render_error(&format!(
                        "Failed to save configuration: {}",
                        e
                    ));
                    std::process::exit(1);
                }
            };

            print!("\r\n");
            crate::onboarding::styled_output::render_success("✓ Configuration saved successfully");
            print!("\r\n");

            // Update AppConfig with saved values so we can use them immediately
            config.provider = profile
                .provider
                .unwrap_or(crate::config::ProviderType::Local);
            config.providers = profile.providers.clone();
            config.model = profile.model.clone();
            if let Some(key) = &profile.api_key {
                config.api_key = Some(key.clone());
            }
            config.anonymous_id = telemetry.anonymous_id;
            config.collect_telemetry = telemetry.collect_telemetry;

            true
        }
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => false,
    }
}

/// Handle BYOM / Custom Provider setup
/// Returns true if completed, false if cancelled/back
async fn handle_byom_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    let config_path = get_config_path_string(config);

    if let Some(profile) = configure_byom(2, 4, profile_name) {
        let telemetry =
            match preview_and_save_to_profile(&config_path, profile_name, profile.clone()) {
                Ok(t) => t,
                Err(e) => {
                    crate::onboarding::styled_output::render_error(&format!(
                        "Failed to save configuration: {}",
                        e
                    ));
                    std::process::exit(1);
                }
            };

        // Update AppConfig with saved values so we can use them immediately
        config.provider = profile
            .provider
            .unwrap_or(crate::config::ProviderType::Local);
        config.providers = profile.providers.clone();
        config.model = profile.model.clone();
        if let Some(key) = &profile.api_key {
            config.api_key = Some(key.clone());
        }
        config.anonymous_id = telemetry.anonymous_id;
        config.collect_telemetry = telemetry.collect_telemetry;

        true
    } else {
        crate::onboarding::styled_output::render_warning(
            "Custom provider configuration cancelled.",
        );
        false
    }
}

/// Handle Stakpak API setup for a new profile
/// Saves API key to the new profile, copying endpoint from default but using new API key
async fn handle_stakpak_api_for_new_profile(config: &AppConfig, profile_name: &str) {
    use crate::apikey_auth::prompt_for_api_key;

    // Create a temporary config with the new profile name for OAuth flow
    let mut temp_config = config.clone();
    temp_config.profile_name = profile_name.to_string();

    // Get the API key via OAuth flow (this will update temp_config)
    prompt_for_api_key(&mut temp_config).await;

    // Now save the new profile with the API key and endpoint
    let config_path = get_config_path_string(config);

    // Create profile config with Remote provider, new API key, and same endpoint
    use crate::config::ProfileConfig;
    use crate::config::ProviderType;
    let new_profile = ProfileConfig {
        provider: Some(ProviderType::Remote),
        api_key: temp_config.api_key.clone(),
        api_endpoint: Some(config.api_endpoint.clone()), // Copy endpoint from default
        ..ProfileConfig::default()
    };

    // Save to the new profile
    if let Err(e) =
        crate::onboarding::save_config::save_to_profile(&config_path, profile_name, new_profile)
    {
        crate::onboarding::styled_output::render_error(&format!(
            "Failed to save configuration: {}",
            e
        ));
        std::process::exit(1);
    }

    print!("\r\n");
    crate::onboarding::styled_output::render_success("✓ Configuration saved successfully");
    print!("\r\n");
}

/// Initial choice enum
#[derive(Clone, Copy, PartialEq)]
enum InitialChoice {
    StakpakAPI,
    OwnKeys,
}

/// Provider choice enum
#[derive(Clone, Copy, PartialEq)]
enum ProviderChoice {
    OpenAI,
    Anthropic,
    Google,
    Hybrid,
    Byom,
}

/// Anthropic auth method choice
#[derive(Clone, Copy, PartialEq)]
enum AnthropicAuthChoice {
    ClaudeSubscription,
    ApiKey,
}
