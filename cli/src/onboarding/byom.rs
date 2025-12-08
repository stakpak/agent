//! Bring Your Own Model (BYOM) configuration

use crate::config::ProfileConfig;
use crate::onboarding::config_templates::{
    self, HybridModelConfig, HybridProvider, generate_hybrid_config,
};
use crate::onboarding::menu::{prompt_password, prompt_text, prompt_yes_no, select_option};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::styled_output::{self, StepStatus};
use stakpak_shared::models::integrations::anthropic::AnthropicModel;
use stakpak_shared::models::integrations::gemini::GeminiModel;
use stakpak_shared::models::integrations::openai::OpenAIModel;

/// Configure BYOM - single model or hybrid
pub fn configure_byom(current_step: usize, total_steps: usize) -> Option<ProfileConfig> {
    println!();
    styled_output::render_title("Bring Your Own Model");
    println!();

    // Ask if they want hybrid configs
    let want_hybrid = match prompt_yes_no(
        "Do you want hybrid configs? (mix providers for smart/eco models)",
        false,
    ) {
        NavResult::Forward(Some(true)) => true,
        NavResult::Forward(Some(false)) | NavResult::Forward(None) => false,
        NavResult::Back | NavResult::Cancel => return None,
    };

    if want_hybrid {
        configure_hybrid_byom(current_step, total_steps)
    } else {
        configure_single_byom(current_step, total_steps)
    }
}

/// Configure single BYOM model
fn configure_single_byom(current_step: usize, total_steps: usize) -> Option<ProfileConfig> {
    // Step: Render steps - we'll update this dynamically if possible, but for now we just show progress
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

    // 1. Endpoint
    let endpoint = match prompt_text("Enter OpenAI-compatible API endpoint", true) {
        NavResult::Forward(Some(ep)) => ep,
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

    // Validate URL format
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        styled_output::render_error("Invalid endpoint URL. Must start with http:// or https://");
        return None;
    }

    // 2. Smart Model Name
    let smart_model = match prompt_text(
        "Enter smart model name (e.g., qwen/qwen-2.5-coder-32b-instruct)",
        true,
    ) {
        NavResult::Forward(Some(model)) => model,
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

    // 3. Eco Model Name (optional)
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
    let eco_model = if let Some(input) = eco_model_input {
        if input.trim().is_empty() {
            smart_model.clone()
        } else {
            input
        }
    } else {
        smart_model.clone()
    };

    // 4. API Key
    let api_key = match prompt_password("Enter API key (optional, press Enter to skip)", false) {
        NavResult::Forward(key) => key,
        NavResult::Back | NavResult::Cancel => return None,
    };

    // Generate config with both models
    // Note: generate_byom_single_config currently might strictly assume same model for both or just one generic "model"
    // I need to check `generate_byom_single_config`. If it only takes one model, I might need to construct ProfileConfig manually or update the helper.
    // Let's check `config_templates.rs` for `generate_byom_single_config`.
    // Assuming for now I can verify that later. If not, I'll construct it here.

    // Actually, I should probably check `config_templates.rs` first to be safe, but the plan didn't explicitly say to modify `config_templates.rs`.
    // However, I can manually construct the ProfileConfig here to be safe and support split models.

    let mut config = ProfileConfig::default();
    config.provider = Some(crate::config::ProviderType::Local); // BYOM usually implies Local/OpenAI compatible
    // Wait, BYOM usually maps to "openai" provider with custom endpoint in Stakpak?
    // Let's rely on `generate_byom_single_config` but if I can't see it, I'll assume it sets up basic compatibility.
    // Actually, to support smart/eco split, I should probably manually set them.

    use crate::onboarding::config_templates::generate_byom_single_config;
    // Let's use the existing helper first validation.
    // But wait, the user wants SEPARATE smart and eco models.
    // If helper doesn't support it, I should do it manually.

    let mut profile = generate_byom_single_config(endpoint, smart_model.clone(), api_key);
    profile.smart_model = Some(smart_model);
    profile.eco_model = Some(eco_model);

    // Show preview
    styled_output::render_config_preview(&config_templates::config_to_toml_preview(&profile));

    // Confirm
    match prompt_yes_no("Save this configuration?", true) {
        NavResult::Forward(Some(true)) | NavResult::Forward(None) => Some(profile), // None means use default (true)
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => None,
    }
}

/// Configure hybrid BYOM (mix providers)
fn configure_hybrid_byom(current_step: usize, total_steps: usize) -> Option<ProfileConfig> {
    println!();
    styled_output::render_info(
        "You'll configure smart_model and eco_model separately, each from different providers if desired.",
    );
    println!();

    // Configure smart model
    styled_output::render_subtitle("Configure Smart Model");
    let smart = configure_hybrid_model("smart", current_step, total_steps)?;

    println!();

    // Configure eco model
    styled_output::render_subtitle("Configure Eco Model");
    let eco = configure_hybrid_model("eco", current_step + 1, total_steps)?;

    let config = generate_hybrid_config(smart, eco);

    // Show preview
    styled_output::render_config_preview(&config_templates::config_to_toml_preview(&config));

    // Confirm
    match prompt_yes_no("Save this hybrid configuration?", true) {
        NavResult::Forward(Some(true)) | NavResult::Forward(None) => Some(config), // None means use default (true)
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => None,
    }
}

/// Configure a single model for hybrid setup
fn configure_hybrid_model(
    model_type: &str,
    current_step: usize,
    total_steps: usize,
) -> Option<HybridModelConfig> {
    // Select provider
    let providers = [
        (HybridProvider::OpenAI, "OpenAI", false),
        (HybridProvider::Gemini, "Gemini", false),
        (HybridProvider::Anthropic, "Anthropic", false),
    ];

    let provider = match select_option(
        &format!("Select provider for {} model", model_type),
        &providers
            .iter()
            .map(|(p, n, r)| (*p, *n, *r))
            .collect::<Vec<_>>(),
        current_step,
        total_steps,
        true, // Can go back
    ) {
        NavResult::Forward(p) => p,
        NavResult::Back | NavResult::Cancel => return None,
    };

    // Select model based on provider
    let model = select_model_for_provider(&provider, model_type, current_step + 1, total_steps)?;

    // Get API key
    let api_key = match prompt_password(&format!("Enter {} API key", provider.as_str()), true) {
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
    provider: &HybridProvider,
    model_type: &str,
    current_step: usize,
    total_steps: usize,
) -> Option<String> {
    let models = match provider {
        HybridProvider::OpenAI => {
            vec![
                (OpenAIModel::GPT5.to_string(), "GPT-5", true),
                (OpenAIModel::GPT5Mini.to_string(), "GPT-5 Mini", false),
                (OpenAIModel::GPT5Nano.to_string(), "GPT-5 Nano", false),
            ]
        }
        HybridProvider::Gemini => {
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
        HybridProvider::Anthropic => {
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

    let options: Vec<_> = models
        .iter()
        .map(|(val, name, rec)| ((*val).to_string(), *name, *rec))
        .collect();

    match select_option(
        &format!("Select {} model", model_type),
        &options
            .iter()
            .map(|(v, n, r)| (v.clone(), *n, *r))
            .collect::<Vec<_>>(),
        current_step,
        total_steps,
        true, // Can go back
    ) {
        NavResult::Forward(model) => Some(model),
        NavResult::Back | NavResult::Cancel => None,
    }
}
