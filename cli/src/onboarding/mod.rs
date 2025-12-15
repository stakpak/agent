//! Onboarding flow for first-time users
//!
//! This module provides a styled, interactive onboarding experience that guides users
//! through setting up their Stakpak configuration, including:
//! - Stakpak API authentication (OAuth flow)
//! - Provider selection (OpenAI, Anthropic, Google, Hybrid Providers, BYOM)
//! - Bring Your Own Model (BYOM) configuration
//! - Hybrid provider configurations (mixing providers)

mod byom;
mod config_templates;
pub mod menu;
pub mod navigation;
mod save_config;
mod styled_output;

use crate::apikey_auth::prompt_for_api_key;
use crate::config::AppConfig;
use crate::onboarding::byom::configure_byom;
use crate::onboarding::config_templates::{
    HybridModelConfig, config_to_toml_preview, generate_anthropic_config, generate_gemini_config,
    generate_hybrid_config, generate_openai_config,
};
use crate::onboarding::menu::{
    prompt_password, prompt_profile_name, select_option, select_option_no_header,
};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::save_config::{preview_and_save_to_profile, save_to_profile};
use crate::onboarding::styled_output::{StepStatus, render_profile_name};
use stakpak_shared::models::integrations::anthropic::AnthropicModel;
use stakpak_shared::models::integrations::gemini::GeminiModel;
use stakpak_shared::models::integrations::openai::OpenAIModel;
use stakpak_shared::models::model_pricing::ContextAware;
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
                "Let's set up your configuration. You can connect to Stakpak API or use your own model/API keys.",
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
                "Let's set up your configuration. You can connect to Stakpak API or use your own model/API keys.",
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
                (InitialChoice::OwnKeys, "Use my own Model/API Key", false),
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
            NavResult::Forward(ProviderChoice::OpenAI) => {
                handle_openai_setup(config, profile_name).await
            }
            NavResult::Forward(ProviderChoice::Anthropic) => {
                handle_anthropic_setup(config, profile_name).await
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

    // Show default models (using same names as config templates)
    crate::onboarding::styled_output::render_default_models(
        &OpenAIModel::DEFAULT_SMART_MODEL.model_name(),
        &OpenAIModel::DEFAULT_ECO_MODEL.model_name(),
    );

    match prompt_password("Enter your OpenAI API key", true) {
        NavResult::Forward(Some(api_key)) => {
            let profile = generate_openai_config(api_key);

            crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
                &profile,
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
                    config.openai = profile.openai.clone();
                    config.smart_model = profile.smart_model.clone();
                    config.eco_model = profile.eco_model.clone();
                    if let Some(key) = &profile.api_key {
                        config.api_key = Some(key.clone());
                    }
                    config.user_id = telemetry.user_id;
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

    // Show default models (using same names as config templates)
    crate::onboarding::styled_output::render_default_models(
        &GeminiModel::DEFAULT_SMART_MODEL.model_name(),
        &GeminiModel::DEFAULT_ECO_MODEL.model_name(),
    );

    match prompt_password("Enter your Gemini API key", true) {
        NavResult::Forward(Some(api_key)) => {
            let profile = generate_gemini_config(api_key);

            // Show confirmation
            crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
                &profile,
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
                    config.gemini = profile.gemini.clone();
                    config.smart_model = profile.smart_model.clone();
                    config.eco_model = profile.eco_model.clone();
                    if let Some(key) = &profile.api_key {
                        config.api_key = Some(key.clone());
                    }
                    config.user_id = telemetry.user_id;
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

/// Handle Anthropic setup
/// Returns true if completed, false if cancelled/back
async fn handle_anthropic_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    // Clear previous step content WITHOUT touching welcome message
    print!("\x1b[u");
    print!("\x1b[0J"); // Clear from cursor to end of screen
    print!("\x1b[K"); // Clear current line
    let _ = io::stdout().flush();

    // Render step 3 - start immediately after welcome message (no extra newline)
    crate::onboarding::styled_output::render_title("Anthropic Configuration");
    print!("\r\n");

    // Show step indicators on one line
    let steps = vec![
        ("Step 1".to_string(), StepStatus::Completed),
        ("Step 2".to_string(), StepStatus::Completed),
        ("Step 3".to_string(), StepStatus::Active),
    ];
    crate::onboarding::styled_output::render_steps(&steps);
    print!("\r\n");

    // Show default models (using same names as config templates)
    crate::onboarding::styled_output::render_default_models(
        &AnthropicModel::DEFAULT_SMART_MODEL.model_name(),
        &AnthropicModel::DEFAULT_ECO_MODEL.model_name(),
    );

    match prompt_password("Enter your Anthropic API key", true) {
        NavResult::Forward(Some(api_key)) => {
            let profile = generate_anthropic_config(api_key);

            // Show confirmation
            crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
                &profile,
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
                    config.anthropic = profile.anthropic.clone();
                    config.smart_model = profile.smart_model.clone();
                    config.eco_model = profile.eco_model.clone();
                    if let Some(key) = &profile.api_key {
                        config.api_key = Some(key.clone());
                    }
                    config.user_id = telemetry.user_id;
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

/// Handle Hybrid Providers setup
/// Returns true if completed, false if cancelled/back
async fn handle_hybrid_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    let config_path = get_config_path_string(config);

    // Clear previous step content WITHOUT touching welcome message
    print!("\x1b[u");
    print!("\x1b[0J"); // Clear from cursor to end of screen
    print!("\x1b[K"); // Clear current line
    let _ = io::stdout().flush();

    // Render step 3 - start immediately after welcome message (no extra newline)
    crate::onboarding::styled_output::render_title("Hybrid Providers Configuration");
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
        "You'll configure smart_model and eco_model separately, each from different providers if desired.",
    );
    print!("\r\n");

    // Configure smart model
    crate::onboarding::styled_output::render_subtitle("Configure Smart Model");
    let smart = match configure_hybrid_model(None) {
        Some(model) => model,
        None => return false,
    };

    print!("\r\n");

    // Configure eco model - pass smart provider to reuse API key if same
    crate::onboarding::styled_output::render_subtitle("Configure Eco Model");
    let eco = match configure_hybrid_model(Some(&smart)) {
        Some(model) => model,
        None => return false,
    };

    let profile = generate_hybrid_config(smart, eco);

    // Show confirmation
    crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(&profile));

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
            config.openai = profile.openai.clone();
            config.anthropic = profile.anthropic.clone();
            config.gemini = profile.gemini.clone();
            config.smart_model = profile.smart_model.clone();
            config.eco_model = profile.eco_model.clone();
            config.recovery_model = profile.recovery_model.clone();
            if let Some(key) = &profile.api_key {
                config.api_key = Some(key.clone());
            }
            config.user_id = telemetry.user_id;
            config.collect_telemetry = telemetry.collect_telemetry;

            true
        }
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => false,
    }
}

/// Configure a single model for hybrid setup
/// If `previous_config` is provided and uses the same provider, reuse the API key
fn configure_hybrid_model(
    previous_config: Option<&HybridModelConfig>,
) -> Option<HybridModelConfig> {
    use crate::onboarding::config_templates::HybridProvider;

    // Select provider
    let providers = [
        (HybridProvider::OpenAI, "OpenAI", false),
        (HybridProvider::Gemini, "Gemini", false),
        (HybridProvider::Anthropic, "Anthropic", false),
    ];

    let provider = match select_option_no_header(&providers, true) {
        NavResult::Forward(p) => p,
        NavResult::Back | NavResult::Cancel => return None,
    };

    // Select model based on provider
    let model = select_model_for_provider(&provider)?;

    // Check if we can reuse API key from previous config
    let api_key = if let Some(prev) = previous_config {
        if prev.provider == provider {
            // Same provider, reuse API key
            prev.api_key.clone()
        } else {
            // Different provider, ask for API key
            match crate::onboarding::menu::prompt_password(
                &format!("Enter {} API key", provider.as_str()),
                true,
            ) {
                NavResult::Forward(Some(key)) => key,
                NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
            }
        }
    } else {
        // No previous config, ask for API key
        match crate::onboarding::menu::prompt_password(
            &format!("Enter {} API key", provider.as_str()),
            true,
        ) {
            NavResult::Forward(Some(key)) => key,
            NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
        }
    };

    Some(HybridModelConfig {
        provider,
        model,
        api_key,
    })
}

/// Select model for a provider
fn select_model_for_provider(
    provider: &crate::onboarding::config_templates::HybridProvider,
) -> Option<String> {
    use std::collections::HashSet;

    let (candidates, smart_id) = match provider {
        crate::onboarding::config_templates::HybridProvider::OpenAI => {
            let smart = OpenAIModel::DEFAULT_SMART_MODEL;
            let eco = OpenAIModel::DEFAULT_ECO_MODEL;
            (
                vec![
                    (smart.to_string(), smart.model_name()),
                    (eco.to_string(), eco.model_name()),
                ],
                smart.to_string(),
            )
        }
        crate::onboarding::config_templates::HybridProvider::Gemini => {
            let smart = GeminiModel::DEFAULT_SMART_MODEL;
            let eco = GeminiModel::DEFAULT_ECO_MODEL;
            (
                vec![
                    (smart.to_string(), smart.model_name()),
                    (eco.to_string(), eco.model_name()),
                ],
                smart.to_string(),
            )
        }
        crate::onboarding::config_templates::HybridProvider::Anthropic => {
            let smart = AnthropicModel::DEFAULT_SMART_MODEL;
            let eco = AnthropicModel::DEFAULT_ECO_MODEL;
            (
                vec![
                    (smart.to_string(), smart.model_name()),
                    (eco.to_string(), eco.model_name()),
                ],
                smart.to_string(),
            )
        }
    };

    let mut options = Vec::new();
    let mut seen = HashSet::new();

    for (id, name) in candidates {
        if !seen.insert(id.clone()) {
            continue;
        }
        let is_recommended = id == smart_id;
        options.push((id, name, is_recommended));
    }

    let options_refs: Vec<(String, &str, bool)> = options
        .iter()
        .map(|(id, name, rec)| (id.clone(), name.as_str(), *rec))
        .collect();

    match select_option_no_header(&options_refs, true) {
        NavResult::Forward(model) => Some(model),
        NavResult::Back | NavResult::Cancel => None,
    }
}

/// Handle BYOM setup
/// Returns true if completed, false if cancelled/back
async fn handle_byom_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    let config_path = get_config_path_string(config);

    if let Some(profile) = configure_byom(2, 4) {
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
        config.openai = profile.openai.clone();
        config.anthropic = profile.anthropic.clone();
        config.gemini = profile.gemini.clone();
        config.smart_model = profile.smart_model.clone();
        config.eco_model = profile.eco_model.clone();
        config.recovery_model = profile.recovery_model.clone();
        if let Some(key) = &profile.api_key {
            config.api_key = Some(key.clone());
        }
        config.user_id = telemetry.user_id;
        config.collect_telemetry = telemetry.collect_telemetry;

        true
    } else {
        crate::onboarding::styled_output::render_warning("BYOM configuration cancelled.");
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
