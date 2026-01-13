//! Bring Your Own Model (BYOM) / Custom Provider configuration
//!
//! This module provides configuration for custom OpenAI-compatible providers
//! such as LiteLLM, Ollama, or any other provider that implements the OpenAI API.

use crate::config::ProfileConfig;
use crate::onboarding::config_templates::{self, generate_custom_provider_profile};
use crate::onboarding::menu::{prompt_password, prompt_text, prompt_yes_no};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::styled_output::{self, StepStatus};

/// Configure a custom OpenAI-compatible provider (BYOM)
///
/// This function guides the user through setting up a custom provider by collecting:
/// - Provider name (e.g., "litellm", "ollama")
/// - API endpoint (e.g., "http://localhost:4000")
/// - API key (optional)
/// - Smart model name (without provider prefix)
/// - Eco model name (without provider prefix, defaults to smart model)
///
/// Model names are automatically prefixed with the provider name (e.g., "litellm/claude-opus").
pub fn configure_byom(current_step: usize, total_steps: usize) -> Option<ProfileConfig> {
    // Render step indicators
    let steps: Vec<_> = (0..total_steps)
        .map(|i| {
            let status = if i < current_step {
                StepStatus::Completed
            } else if i == current_step {
                StepStatus::Active
            } else {
                StepStatus::Pending
            };
            (format!("Step {}", i + 1), status)
        })
        .collect();
    styled_output::render_steps(&steps);
    println!();
    println!();

    // Step 1: Provider name
    styled_output::render_info(
        "Configure your custom OpenAI-compatible provider (e.g., LiteLLM, Ollama).",
    );
    println!();

    let provider_name = match prompt_text("Enter provider name (e.g., litellm, ollama)", true) {
        NavResult::Forward(Some(name)) => name.trim().to_string(),
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

    if provider_name.is_empty() {
        styled_output::render_error("Provider name cannot be empty.");
        return None;
    }

    // Step 2: API endpoint
    let api_endpoint = match prompt_text("Enter API endpoint (e.g., http://localhost:4000)", true) {
        NavResult::Forward(Some(ep)) => ep.trim().to_string(),
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

    if !api_endpoint.starts_with("http://") && !api_endpoint.starts_with("https://") {
        styled_output::render_error("Invalid endpoint URL. Must start with http:// or https://");
        return None;
    }

    // Step 3: API key (optional)
    let api_key = match prompt_password("Enter API key (optional, press Enter to skip)", false) {
        NavResult::Forward(key) => key.filter(|k| !k.is_empty()),
        NavResult::Back | NavResult::Cancel => return None,
    };

    // Step 4: Smart model name
    println!();
    styled_output::render_info(&format!(
        "Model names will be prefixed with '{}/' automatically.",
        provider_name
    ));
    let smart_model = match prompt_text("Enter smart model name (e.g., claude-opus-4-5)", true) {
        NavResult::Forward(Some(model)) => model.trim().to_string(),
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

    if smart_model.is_empty() {
        styled_output::render_error("Smart model name cannot be empty.");
        return None;
    }

    // Step 5: Eco model name (optional, defaults to smart model)
    println!();
    styled_output::render_info("Eco model is used for smaller tasks and cost saving.");
    let eco_model_input = match prompt_text(
        &format!(
            "Enter eco model name (press Enter to use '{}')",
            smart_model
        ),
        false,
    ) {
        NavResult::Forward(input) => input,
        NavResult::Back | NavResult::Cancel => return None,
    };

    let eco_model = match eco_model_input {
        Some(input) if !input.trim().is_empty() => input.trim().to_string(),
        _ => smart_model.clone(),
    };

    // Generate profile
    let profile = generate_custom_provider_profile(
        provider_name.clone(),
        api_endpoint,
        api_key,
        smart_model.clone(),
        eco_model.clone(),
    );

    // Show preview
    println!();
    styled_output::render_info(&format!("Smart model: {}/{}", provider_name, smart_model));
    styled_output::render_info(&format!("Eco model: {}/{}", provider_name, eco_model));
    println!();

    styled_output::render_config_preview(&config_templates::config_to_toml_preview(&profile));

    // Confirm
    match prompt_yes_no("Save this configuration?", true) {
        NavResult::Forward(Some(true)) | NavResult::Forward(None) => Some(profile),
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => None,
    }
}
