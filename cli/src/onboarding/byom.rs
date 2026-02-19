//! Bring Your Own Model (BYOM) / Custom Provider configuration
//!
//! This module provides configuration for custom OpenAI-compatible providers
//! such as Ollama, vLLM, or any other provider that implements the OpenAI API.
//!
//! # Configuration Format
//!
//! Custom providers are configured in the `providers` HashMap. The provider key
//! becomes the model prefix used to route requests.
//!
//! ```toml
//! [profiles.default]
//! provider = "local"
//! model = "offline/llama3"
//!
//! [profiles.default.providers.offline]
//! type = "custom"
//! api_endpoint = "http://localhost:11434/v1"
//! # api_key is optional for local providers like Ollama
//! ```
//!
//! # Model Prefix Routing
//!
//! The provider key (e.g., `offline`) becomes the model prefix:
//! - `offline/llama3` → provider: `offline`, model sent to API: `llama3`
//! - `custom/anthropic/claude-opus` → provider: `custom`, model sent to API: `anthropic/claude-opus`
//!
//! This allows nested prefixes for providers that route to upstream providers.
//!
//! # API Endpoint
//!
//! The `api_endpoint` should be the base URL of your provider. Do NOT include
//! `/chat/completions` - this is appended automatically at runtime.

use crate::config::ProfileConfig;
use crate::onboarding::config_templates::{self, generate_custom_provider_profile};
use crate::onboarding::menu::{prompt_password, prompt_text, prompt_yes_no};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::styled_output::{self, StepStatus};

/// Configure a custom OpenAI-compatible provider (BYOM)
///
/// This function guides the user through setting up a custom provider by collecting:
/// - Provider name (e.g., "litellm", "ollama") - becomes the model prefix
/// - API endpoint as required by the provider (e.g., "http://localhost:4000" or "http://localhost:11434/v1")
/// - API key (optional)
/// - Model name (without provider prefix)
///
/// Model names are automatically prefixed with the provider name.
/// For example, with provider "litellm" and model "anthropic/claude-opus":
/// - Full model string: "litellm/anthropic/claude-opus"
/// - Provider lookup: "litellm" (matches config key)
/// - Model sent to API: "anthropic/claude-opus"
pub fn configure_byom(
    current_step: usize,
    total_steps: usize,
    profile_name: &str,
) -> Option<ProfileConfig> {
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

    // Step 2: API endpoint (user provides the full base URL as required by their provider)
    let api_endpoint = match prompt_text(
        "Enter API endpoint (e.g., http://localhost:4000 or http://localhost:11434/v1)",
        true,
    ) {
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

    // Step 4: Model name
    println!();
    styled_output::render_info(&format!(
        "Model name will be prefixed with '{}/' automatically.",
        provider_name
    ));
    let model_name = match prompt_text("Enter model name (e.g., claude-opus-4-5, llama3)", true) {
        NavResult::Forward(Some(model)) => model.trim().to_string(),
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

    if model_name.is_empty() {
        styled_output::render_error("Model name cannot be empty.");
        return None;
    }

    // Generate profile
    let profile = generate_custom_provider_profile(
        provider_name.clone(),
        api_endpoint,
        api_key,
        model_name.clone(),
    );

    // Show preview
    println!();
    styled_output::render_info(&format!("Model: {}/{}", provider_name, model_name));
    println!();

    styled_output::render_config_preview(&config_templates::config_to_toml_preview(
        &profile,
        profile_name,
    ));

    // Confirm
    match prompt_yes_no("Save this configuration?", true) {
        NavResult::Forward(Some(true)) | NavResult::Forward(None) => Some(profile),
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => None,
    }
}
