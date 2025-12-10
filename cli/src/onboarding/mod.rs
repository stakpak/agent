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
use std::io::{self, Write};

/// Helper function to get config path string from AppConfig
/// Returns the config_path if not empty, otherwise gets the default path
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
            // For default mode, use the current profile name
            let profile = config.profile_name.clone();

            print!("\r\n");
            crate::onboarding::styled_output::render_title("Welcome to Stakpak");
            print!("\r\n");
            // Show profile name with empty line before it
            render_profile_name(&profile);
            print!("\r\n");
            crate::onboarding::styled_output::render_info(
                "Let's set up your configuration. You can connect to Stakpak API or use your own model/API keys.",
            );
            print!("\r\n");

            profile
        }
        OnboardingMode::New => {
            // For new profile mode, prompt for profile name first
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

            // Clear the title and prompt lines, then show profile name with empty line before it
            print!("\x1b[2A"); // Move up 2 lines (to title line)
            print!("\x1b[0J"); // Clear from cursor to end of screen
            print!("\r\n"); // Empty line before profile name
            render_profile_name(&profile_name);
            print!("\r\n");
            crate::onboarding::styled_output::render_info(
                "Let's set up your configuration. You can connect to Stakpak API or use your own model/API keys.",
            );
            print!("\r\n");

            profile_name
        }
    };

    // Save position after welcome message - this is where ALL step content starts
    // We'll always clear from here before rendering a new step
    // This position will be used by all subsequent steps
    print!("\x1b[s");

    // Initial decision: Stakpak API or Own Keys
    loop {
        // CRITICAL: Restore to step content start and clear everything from there
        // This ensures we only see the current step, not previous steps
        // We restore to the position saved after welcome message
        print!("\x1b[u"); // Restore to position after welcome
        // Clear from cursor to end of screen AND clear the current line
        print!("\x1b[0J"); // Clear from cursor to end of screen
        print!("\x1b[K"); // Clear from cursor to end of current line
        let _ = io::stdout().flush();
        // Re-save position to ensure it's correct
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
                // Clear step content before OAuth flow
                print!("\x1b[u");
                print!("\x1b[0J");
                let _ = io::stdout().flush();

                // Handle Stakpak API based on mode
                match mode {
                    OnboardingMode::Default => {
                        // Use existing OAuth flow - saves to default profile
                        prompt_for_api_key(config).await;
                    }
                    OnboardingMode::New => {
                        // For new profile, save API key to new profile, not default
                        handle_stakpak_api_for_new_profile(config, &profile_name).await;
                    }
                }
                break;
            }
            NavResult::Forward(InitialChoice::OwnKeys) => {
                // select_option already cleared everything including title and steps
                // Cursor is now at the position right after welcome message
                // Just re-save this position for step 2 (no need to restore/clear again)
                print!("\x1b[s");
                if handle_own_keys_flow(config, &profile_name).await {
                    break;
                }
                // If we return false (user went back), loop to re-render step 1
                continue;
            }
            NavResult::Back => {
                // Shouldn't happen on first step, but handle gracefully
                break;
            }
            NavResult::Cancel => {
                // Clear step content
                print!("\x1b[u");
                print!("\x1b[0J");
                // User cancelled (ESC on first step = exit)
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

/// Handle the "Own Keys" flow
/// Returns true if configuration was completed, false if user went back
async fn handle_own_keys_flow(config: &mut AppConfig, profile_name: &str) -> bool {
    // Provider selection with back navigation support
    loop {
        // CRITICAL: Clear previous step content WITHOUT touching welcome message
        // On first iteration, cursor is already at correct position (select_option cleared everything)
        // On subsequent iterations (after back navigation), restore and clear
        print!("\x1b[u");
        print!("\x1b[0J"); // Clear from cursor to end of screen
        print!("\x1b[K"); // Clear current line
        let _ = io::stdout().flush();
        // Re-save position after clearing for next step or back navigation
        print!("\x1b[s");

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
                // User wants to go back to step 1
                // Clear step 2 content WITHOUT touching welcome message
                print!("\x1b[u");
                print!("\x1b[0J"); // Clear from cursor to end of screen
                print!("\x1b[K"); // Clear current line
                let _ = io::stdout().flush();
                // Re-save position after welcome for step 1
                print!("\x1b[s");
                return false;
            }
            NavResult::Cancel => {
                // Shouldn't happen, but handle gracefully
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

    // Show default models
    crate::onboarding::styled_output::render_default_models(
        &OpenAIModel::default_smart_model(),
        &OpenAIModel::default_eco_model(),
        Some(&OpenAIModel::default_recovery_model()),
    );

    match prompt_password("Enter your OpenAI API key", true) {
        NavResult::Forward(Some(api_key)) => {
            let profile = generate_openai_config(api_key);

            // Show confirmation
            crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(
                &profile,
            ));

            match crate::onboarding::menu::prompt_yes_no("Proceed with this configuration?", true) {
                NavResult::Forward(Some(true)) | NavResult::Forward(None) => {
                    let config_path = get_config_path_string(config);

                    if let Err(e) = save_to_profile(&config_path, profile_name, profile.clone()) {
                        crate::onboarding::styled_output::render_error(&format!(
                            "Failed to save configuration: {}",
                            e
                        ));
                        std::process::exit(1);
                    }

                    println!();
                    crate::onboarding::styled_output::render_success(
                        "✓ Configuration saved successfully",
                    );
                    println!();

                    // Update AppConfig with saved values so we can use them immediately
                    config.provider = profile
                        .provider
                        .unwrap_or(crate::config::ProviderType::Local);
                    config.openai = profile.openai.clone();
                    config.smart_model = profile.smart_model.clone();
                    config.eco_model = profile.eco_model.clone();
                    config.recovery_model = profile.recovery_model.clone();
                    if let Some(key) = &profile.api_key {
                        config.api_key = Some(key.clone());
                    }

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

    // Show default models
    crate::onboarding::styled_output::render_default_models(
        &GeminiModel::default_smart_model(),
        &GeminiModel::default_eco_model(),
        Some(&GeminiModel::default_recovery_model()),
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

                    if let Err(e) = save_to_profile(&config_path, profile_name, profile.clone()) {
                        crate::onboarding::styled_output::render_error(&format!(
                            "Failed to save configuration: {}",
                            e
                        ));
                        std::process::exit(1);
                    }

                    println!();
                    crate::onboarding::styled_output::render_success(
                        "✓ Configuration saved successfully",
                    );
                    println!();

                    // Update AppConfig with saved values so we can use them immediately
                    config.provider = profile
                        .provider
                        .unwrap_or(crate::config::ProviderType::Local);
                    config.gemini = profile.gemini.clone();
                    config.smart_model = profile.smart_model.clone();
                    config.eco_model = profile.eco_model.clone();
                    config.recovery_model = profile.recovery_model.clone();
                    if let Some(key) = &profile.api_key {
                        config.api_key = Some(key.clone());
                    }

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

    // Show default models
    crate::onboarding::styled_output::render_default_models(
        &AnthropicModel::default_smart_model(),
        &AnthropicModel::default_eco_model(),
        Some(&AnthropicModel::default_recovery_model()),
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

                    if let Err(e) = save_to_profile(&config_path, profile_name, profile.clone()) {
                        crate::onboarding::styled_output::render_error(&format!(
                            "Failed to save configuration: {}",
                            e
                        ));
                        std::process::exit(1);
                    }

                    println!();
                    crate::onboarding::styled_output::render_success(
                        "✓ Configuration saved successfully",
                    );
                    println!();

                    // Update AppConfig with saved values so we can use them immediately
                    config.provider = profile
                        .provider
                        .unwrap_or(crate::config::ProviderType::Local);
                    config.anthropic = profile.anthropic.clone();
                    config.smart_model = profile.smart_model.clone();
                    config.eco_model = profile.eco_model.clone();
                    config.recovery_model = profile.recovery_model.clone();
                    if let Some(key) = &profile.api_key {
                        config.api_key = Some(key.clone());
                    }

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
    let smart = match configure_hybrid_model() {
        Some(model) => model,
        None => return false,
    };

    print!("\r\n");

    // Configure eco model
    crate::onboarding::styled_output::render_subtitle("Configure Eco Model");
    let eco = match configure_hybrid_model() {
        Some(model) => model,
        None => return false,
    };

    let profile = generate_hybrid_config(smart, eco);

    // Show confirmation
    crate::onboarding::styled_output::render_config_preview(&config_to_toml_preview(&profile));

    match crate::onboarding::menu::prompt_yes_no("Proceed with this configuration?", true) {
        NavResult::Forward(Some(true)) | NavResult::Forward(None) => {
            if let Err(e) = save_to_profile(&config_path, profile_name, profile.clone()) {
                crate::onboarding::styled_output::render_error(&format!(
                    "Failed to save configuration: {}",
                    e
                ));
                std::process::exit(1);
            }

            println!();
            crate::onboarding::styled_output::render_success("✓ Configuration saved successfully");
            println!();

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

            true
        }
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => false,
    }
}

/// Configure a single model for hybrid setup
fn configure_hybrid_model() -> Option<HybridModelConfig> {
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

    // Get API key
    let api_key = match crate::onboarding::menu::prompt_password(
        &format!("Enter {} API key", provider.as_str()),
        true,
    ) {
        NavResult::Forward(Some(key)) => key,
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
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
    let models: Vec<(String, &str, bool)> = match provider {
        crate::onboarding::config_templates::HybridProvider::OpenAI => {
            vec![
                (OpenAIModel::GPT5.to_string(), "GPT-5", true),
                (OpenAIModel::GPT5Mini.to_string(), "GPT-5 Mini", false),
                (OpenAIModel::GPT5Nano.to_string(), "GPT-5 Nano", false),
            ]
        }
        crate::onboarding::config_templates::HybridProvider::Gemini => {
            vec![
                (
                    GeminiModel::Gemini3Pro.to_string(),
                    "Gemini 3 Pro Preview",
                    true,
                ),
                (
                    GeminiModel::Gemini25Pro.to_string(),
                    "Gemini 2.5 Pro",
                    false,
                ),
                (
                    GeminiModel::Gemini25Flash.to_string(),
                    "Gemini 2.5 Flash",
                    false,
                ),
                (
                    GeminiModel::Gemini25FlashLite.to_string(),
                    "Gemini 2.5 Flash Lite",
                    false,
                ),
            ]
        }
        crate::onboarding::config_templates::HybridProvider::Anthropic => {
            vec![
                (
                    AnthropicModel::Claude45Opus.to_string(),
                    "Claude Opus 4.5",
                    true,
                ),
                (
                    AnthropicModel::Claude45Sonnet.to_string(),
                    "Claude Sonnet 4.5",
                    false,
                ),
                (
                    AnthropicModel::Claude45Haiku.to_string(),
                    "Claude Haiku 4.5",
                    false,
                ),
            ]
        }
    };

    match select_option_no_header(&models, true) {
        NavResult::Forward(model) => Some(model),
        NavResult::Back | NavResult::Cancel => None,
    }
}

/// Handle BYOM setup
/// Returns true if completed, false if cancelled/back
async fn handle_byom_setup(config: &mut AppConfig, profile_name: &str) -> bool {
    let config_path = get_config_path_string(config);

    if let Some(profile) = configure_byom(2, 4) {
        if let Err(e) = preview_and_save_to_profile(&config_path, profile_name, profile.clone()) {
            crate::onboarding::styled_output::render_error(&format!(
                "Failed to save configuration: {}",
                e
            ));
            std::process::exit(1);
        }

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

    println!();
    crate::onboarding::styled_output::render_success("✓ Configuration saved successfully");
    println!();
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
