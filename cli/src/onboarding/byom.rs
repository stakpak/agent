//! Bring Your Own Model (BYOM) configuration

use crate::config::ProfileConfig;
use crate::onboarding::config_templates::{self};
use crate::onboarding::menu::{prompt_password, prompt_text, prompt_yes_no};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::styled_output::{self, StepStatus};

/// Configure BYOM - single model configuration
pub fn configure_byom(current_step: usize, total_steps: usize) -> Option<ProfileConfig> {
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

    let endpoint = match prompt_text("Enter OpenAI-compatible API endpoint", true) {
        NavResult::Forward(Some(ep)) => ep,
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        styled_output::render_error("Invalid endpoint URL. Must start with http:// or https://");
        return None;
    }

    let smart_model = match prompt_text(
        "Enter smart model name (e.g., qwen/qwen-2.5-coder-32b-instruct)",
        true,
    ) {
        NavResult::Forward(Some(model)) => model,
        NavResult::Forward(None) | NavResult::Back | NavResult::Cancel => return None,
    };

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

    let api_key = match prompt_password("Enter API key (optional, press Enter to skip)", false) {
        NavResult::Forward(key) => key,
        NavResult::Back | NavResult::Cancel => return None,
    };

    use crate::onboarding::config_templates::generate_byom_single_config;

    let mut profile = generate_byom_single_config(endpoint, smart_model.clone(), api_key);
    profile.smart_model = Some(smart_model);
    profile.eco_model = Some(eco_model);

    styled_output::render_config_preview(&config_templates::config_to_toml_preview(&profile));

    match prompt_yes_no("Save this configuration?", true) {
        NavResult::Forward(Some(true)) | NavResult::Forward(None) => Some(profile), // None means use default (true)
        NavResult::Forward(Some(false)) | NavResult::Back | NavResult::Cancel => None,
    }
}
