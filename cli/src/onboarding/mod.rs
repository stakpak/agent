//! Onboarding flow for first-time users
//!
//! This module provides a styled, interactive onboarding experience that guides users
//! through setting up their Stakpak configuration, including:
//! - Stakpak API authentication (OAuth flow)
//! - Provider selection (OpenAI, Anthropic, Google, Hybrid Providers, BYOM)
//! - Bring Your Own Model (BYOM) configuration
//! - Hybrid provider configurations (mixing providers)

pub mod auth_flow;
mod byom;
pub mod config_templates;
pub mod menu;
pub mod navigation;
pub mod save_config;
mod styled_output;

use crate::apikey_auth::prompt_for_api_key;
use crate::config::AppConfig;
use crate::onboarding::auth_flow::{
    AuthFlowConfig, apply_auth_flow_result, run_provider_auth_flow,
};
use crate::onboarding::menu::{prompt_profile_name, select_option};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::styled_output::render_profile_name;
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
                    "Use my own Model/API Key (or ChatGPT Plus/Pro / Claude Pro/Max / GitHub Copilot Subscription)",
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
    let config_path = get_config_path_string(config);
    let result = run_provider_auth_flow(AuthFlowConfig {
        profile_name: profile_name.to_string(),
        config_path,
        step_offset: 2,
        total_steps: 5,
        show_preview: true,
        include_special_options: true,
        preserve_existing_profile: false,
    })
    .await;

    if let Some(result) = result {
        apply_auth_flow_result(config, &result);
        true
    } else {
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

#[cfg(test)]
mod tests {
    #[test]
    fn own_keys_choice_mentions_chatgpt_plus_pro_subscription() {
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/onboarding/mod.rs"
        ))
        .expect("read onboarding module source");
        let own_keys_line = source
            .lines()
            .find(|line| line.contains("Use my own Model/API Key"))
            .expect("own-keys onboarding label");

        assert!(
            own_keys_line.contains("ChatGPT Plus/Pro"),
            "config new own-keys choice should mention ChatGPT Plus/Pro subscription"
        );
    }
}

/// Initial choice enum
#[derive(Clone, Copy, PartialEq)]
enum InitialChoice {
    StakpakAPI,
    OwnKeys,
}
