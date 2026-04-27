use crate::config::{AppConfig, ProfileConfig, ProviderType};
use crate::onboarding::byom::configure_byom;
use crate::onboarding::config_templates::{
    BuiltinProvider, DEFAULT_MODEL, ProviderSetup, config_to_toml_preview,
    generate_anthropic_profile, generate_gemini_profile, generate_github_copilot_profile,
    generate_multi_provider_profile, generate_openai_profile, generate_openrouter_profile,
};
use crate::onboarding::menu::{
    prompt_password, prompt_yes_no, select_option, select_option_no_header,
};
use crate::onboarding::navigation::NavResult;
use crate::onboarding::save_config::{TelemetrySettings, save_to_profile};
use crate::onboarding::styled_output::{self, StepStatus};
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::oauth::{
    AuthMethod, AuthMethodType, OAuthFlow, OAuthProvider, ProviderRegistry,
};
use std::io::{self, Write};

#[derive(Clone, Debug)]
pub struct AuthFlowConfig {
    pub profile_name: String,
    pub config_path: String,
    pub step_offset: usize,
    pub total_steps: usize,
    pub show_preview: bool,
    pub include_special_options: bool,
    pub preserve_existing_profile: bool,
}

#[derive(Clone, Debug)]
pub struct AuthFlowResult {
    pub profile: ProfileConfig,
    pub telemetry: TelemetrySettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProviderSelection {
    Registry(String),
    Hybrid,
    Byom,
}

#[derive(Clone, Debug)]
enum FlowOutcome {
    Complete(Box<AuthFlowResult>),
    RetryProviderSelection,
    Cancelled,
}

pub async fn run_provider_auth_flow(config: AuthFlowConfig) -> Option<AuthFlowResult> {
    styled_output::render_telemetry_disclaimer();

    loop {
        let provider_selection = match select_provider(&config) {
            Some(selection) => selection,
            None => return None,
        };

        let outcome = match provider_selection {
            ProviderSelection::Registry(provider_id) => {
                let registry = ProviderRegistry::new();
                let provider = match registry.get(&provider_id) {
                    Some(provider) => provider,
                    None => {
                        styled_output::render_error(&format!("Unknown provider: {}", provider_id));
                        return None;
                    }
                };

                let auth_method = match select_auth_method(provider, &config) {
                    Some(method) => method,
                    None => continue,
                };

                match auth_method.method_type {
                    AuthMethodType::OAuth => {
                        execute_oauth_flow(provider, &auth_method.id, &auth_method.label, &config)
                            .await
                    }
                    AuthMethodType::DeviceFlow => {
                        execute_device_flow(provider, &auth_method.id, &auth_method.label, &config)
                            .await
                    }
                    AuthMethodType::ApiKey => {
                        execute_api_key_flow(provider, &auth_method.label, &config).await
                    }
                }
            }
            ProviderSelection::Hybrid => execute_hybrid_flow(&config).await,
            ProviderSelection::Byom => execute_byom_flow(&config).await,
        };

        match outcome {
            FlowOutcome::Complete(result) => return Some(*result),
            FlowOutcome::RetryProviderSelection => continue,
            FlowOutcome::Cancelled => return None,
        }
    }
}

pub fn apply_auth_flow_result(config: &mut AppConfig, result: &AuthFlowResult) {
    config.provider = result.profile.provider.unwrap_or(ProviderType::Local);
    config.providers = result.profile.providers.clone();
    config.model = result.profile.model.clone();
    if let Some(api_key) = &result.profile.api_key {
        config.api_key = Some(api_key.clone());
    }
    config.anonymous_id = result.telemetry.anonymous_id.clone();
    config.collect_telemetry = result.telemetry.collect_telemetry;
}

fn select_provider(config: &AuthFlowConfig) -> Option<ProviderSelection> {
    let options = build_provider_menu_options(config.include_special_options);
    let option_refs: Vec<_> = options
        .iter()
        .map(|(value, label, recommended)| (value.clone(), label.as_str(), *recommended))
        .collect();

    match select_option(
        "Choose provider or authentication source",
        &option_refs,
        config.step_offset,
        config.total_steps,
        true,
    ) {
        NavResult::Forward(selection) => Some(selection),
        NavResult::Back | NavResult::Cancel => None,
    }
}

fn select_auth_method(provider: &dyn OAuthProvider, config: &AuthFlowConfig) -> Option<AuthMethod> {
    let methods = provider.auth_methods();
    if methods.len() == 1 {
        return methods.first().cloned();
    }

    let options: Vec<(String, String, bool)> = methods
        .iter()
        .enumerate()
        .map(|(index, method)| (method.id.clone(), method.display(), index == 0))
        .collect();
    let option_refs: Vec<_> = options
        .iter()
        .map(|(id, label, recommended)| (id.clone(), label.as_str(), *recommended))
        .collect();

    match select_option(
        &format!("Choose {} authentication method", provider.name()),
        &option_refs,
        config.step_offset + 1,
        config.total_steps,
        true,
    ) {
        NavResult::Forward(method_id) => methods.into_iter().find(|method| method.id == method_id),
        NavResult::Back | NavResult::Cancel => None,
    }
}

async fn execute_oauth_flow(
    provider: &dyn OAuthProvider,
    method_id: &str,
    method_label: &str,
    config: &AuthFlowConfig,
) -> FlowOutcome {
    render_auth_step_header(method_label, config);
    render_default_model_for_provider(provider.id());

    let oauth_config = match provider.oauth_config(method_id) {
        Some(config) => config,
        None => {
            styled_output::render_error("OAuth not supported for this method");
            return FlowOutcome::RetryProviderSelection;
        }
    };

    let mut flow = OAuthFlow::new(oauth_config.clone());
    let auth_url = flow.generate_auth_url();

    println!();
    styled_output::render_info("Opening browser for authentication...");
    println!();
    println!("If browser doesn't open, visit:");
    println!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", auth_url, auth_url);
    println!();

    let callback = if crate::commands::auth::login::should_wait_for_local_oauth_callback(
        &oauth_config.redirect_url,
    ) {
        styled_output::render_info(&format!(
            "Waiting for OAuth callback on {}...",
            oauth_config.redirect_url
        ));

        let callback_listener =
            match crate::commands::auth::login::bind_local_oauth_callback_listener(
                oauth_config.redirect_url.clone(),
            )
            .await
            {
                Ok(listener) => listener,
                Err(error) => {
                    styled_output::render_error(&error);
                    return FlowOutcome::RetryProviderSelection;
                }
            };

        let _ = open::that(&auth_url);

        match callback_listener
            .wait(std::time::Duration::from_secs(300))
            .await
        {
            Ok(callback) => crate::commands::auth::login::OAuthCallback::FromRedirect(callback),
            Err(error) => {
                styled_output::render_error(&error);
                return FlowOutcome::RetryProviderSelection;
            }
        }
    } else {
        let _ = open::that(&auth_url);

        print!("Paste the authorization code: ");
        if io::stdout().flush().is_err() {
            styled_output::render_error("Failed to flush stdout for OAuth prompt");
            return FlowOutcome::RetryProviderSelection;
        }

        let mut code = String::new();
        if io::stdin().read_line(&mut code).is_err() {
            styled_output::render_error("Failed to read OAuth authorization code");
            return FlowOutcome::RetryProviderSelection;
        }

        let code = code.trim().to_string();
        if code.is_empty() {
            styled_output::render_warning("Authentication cancelled.");
            return FlowOutcome::RetryProviderSelection;
        }

        crate::commands::auth::login::OAuthCallback::Manual(code)
    };

    println!();
    styled_output::render_info("Exchanging code for tokens...");

    let token_result = match callback {
        crate::commands::auth::login::OAuthCallback::Manual(code) => {
            flow.exchange_code(&code).await
        }
        crate::commands::auth::login::OAuthCallback::FromRedirect(callback) => {
            flow.exchange_code_with_state(&callback.code, &callback.state)
                .await
        }
    };
    let tokens = match token_result {
        Ok(tokens) => tokens,
        Err(error) => {
            styled_output::render_error(&format!("Token exchange failed: {}", error));
            return FlowOutcome::RetryProviderSelection;
        }
    };

    let auth = match provider.post_authorize(method_id, &tokens).await {
        Ok(auth) => auth,
        Err(error) => {
            styled_output::render_error(&format!("Post-authorization failed: {}", error));
            return FlowOutcome::RetryProviderSelection;
        }
    };

    save_provider_auth(provider, auth, config)
}

async fn execute_device_flow(
    provider: &dyn OAuthProvider,
    method_id: &str,
    method_label: &str,
    config: &AuthFlowConfig,
) -> FlowOutcome {
    render_auth_step_header(method_label, config);
    render_default_model_for_provider(provider.id());

    let (flow, device_code) = match provider.request_device_code(method_id).await {
        Ok(pair) => pair,
        Err(error) => {
            styled_output::render_error(&format!("Failed to start device flow: {}", error));
            return FlowOutcome::RetryProviderSelection;
        }
    };

    println!();
    styled_output::render_info(&format!("Authenticate with {}:", provider.name()));
    println!();
    println!("  1. Visit: {}", device_code.verification_uri);
    println!("  2. Enter code: {}", device_code.user_code);
    println!();

    let _ = open::that(&device_code.verification_uri);
    styled_output::render_info("Waiting for authorization...");

    let token = match provider.wait_for_token(&flow, &device_code).await {
        Ok(token) => token,
        Err(error) => {
            styled_output::render_error(&format!("Authorization failed: {}", error));
            return FlowOutcome::RetryProviderSelection;
        }
    };

    let auth = match provider.post_device_authorize(method_id, &token).await {
        Ok(auth) => auth,
        Err(error) => {
            styled_output::render_error(&format!("Post-authorization failed: {}", error));
            return FlowOutcome::RetryProviderSelection;
        }
    };

    save_provider_auth(provider, auth, config)
}

async fn execute_api_key_flow(
    provider: &dyn OAuthProvider,
    method_label: &str,
    config: &AuthFlowConfig,
) -> FlowOutcome {
    render_auth_step_header(method_label, config);
    render_default_model_for_provider(provider.id());

    let prompt = match provider.id() {
        "anthropic" => "Enter your Anthropic API key",
        "openai" => "Enter your OpenAI API key",
        "gemini" => "Enter your Gemini API key",
        _ => "Enter API key",
    };

    let api_key = match prompt_password(prompt, true) {
        NavResult::Forward(Some(api_key)) => api_key,
        NavResult::Forward(None) | NavResult::Back => return FlowOutcome::RetryProviderSelection,
        NavResult::Cancel => return FlowOutcome::Cancelled,
    };

    save_provider_auth(provider, ProviderAuth::api_key(api_key), config)
}

async fn execute_hybrid_flow(config: &AuthFlowConfig) -> FlowOutcome {
    render_auth_step_header("Hybrid providers", config);
    styled_output::render_info(
        "Configure multiple providers so you can switch models at runtime using /model.",
    );
    println!();

    let mut providers: Vec<ProviderSetup> = Vec::new();
    let available_providers = [
        (BuiltinProvider::Anthropic, "Anthropic (recommended)", true),
        (BuiltinProvider::OpenAI, "OpenAI", false),
        (BuiltinProvider::Gemini, "Gemini", false),
    ];

    loop {
        styled_output::render_subtitle("Add a provider");

        let remaining: Vec<_> = available_providers
            .iter()
            .filter(|(provider, _, _)| !providers.iter().any(|setup| setup.provider == *provider))
            .cloned()
            .collect();

        if remaining.is_empty() {
            styled_output::render_info("All providers configured!");
            break;
        }

        let provider = match select_option_no_header(&remaining, true) {
            NavResult::Forward(provider) => provider,
            NavResult::Back => return FlowOutcome::RetryProviderSelection,
            NavResult::Cancel => return FlowOutcome::Cancelled,
        };

        let api_key =
            match prompt_password(&format!("Enter {} API key", provider.display_name()), true) {
                NavResult::Forward(Some(key)) => key,
                NavResult::Forward(None) => continue,
                NavResult::Back => continue,
                NavResult::Cancel => return FlowOutcome::Cancelled,
            };

        providers.push(ProviderSetup { provider, api_key });

        if remaining.len() > 1 {
            match prompt_yes_no("Add another provider?", false) {
                NavResult::Forward(Some(true)) => continue,
                NavResult::Forward(Some(false)) | NavResult::Forward(None) | NavResult::Back => {
                    break;
                }
                NavResult::Cancel => return FlowOutcome::Cancelled,
            }
        } else {
            break;
        }
    }

    if providers.is_empty() {
        styled_output::render_warning("No providers configured.");
        return FlowOutcome::RetryProviderSelection;
    }

    let default_model = providers
        .first()
        .map(|provider| provider.provider.default_model().to_string())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let profile = generate_multi_provider_profile(providers, default_model);

    save_profile(profile, "Hybrid providers".to_string(), config)
}

async fn execute_byom_flow(config: &AuthFlowConfig) -> FlowOutcome {
    let byom_profile = configure_byom(
        config.step_offset + 1,
        config.total_steps.max(config.step_offset + 2),
        &config.profile_name,
    );

    match byom_profile {
        Some(profile) => save_profile(profile, "Custom provider".to_string(), config),
        None => FlowOutcome::RetryProviderSelection,
    }
}

fn save_provider_auth(
    provider: &dyn OAuthProvider,
    auth: ProviderAuth,
    config: &AuthFlowConfig,
) -> FlowOutcome {
    let mut profile = match build_provider_profile(provider.id()) {
        Ok(profile) => profile,
        Err(error) => {
            styled_output::render_error(&error);
            return FlowOutcome::RetryProviderSelection;
        }
    };

    if let Some(provider_config) = profile.providers.get_mut(provider.id()) {
        provider_config.set_auth(auth);
    }

    save_profile(profile, provider.name().to_string(), config)
}

fn save_profile(
    profile: ProfileConfig,
    provider_name: String,
    config: &AuthFlowConfig,
) -> FlowOutcome {
    let final_profile = if config.preserve_existing_profile {
        match merge_with_existing_profile(&config.config_path, &config.profile_name, &profile) {
            Ok(merged) => merged,
            Err(error) => {
                styled_output::render_error(&error);
                return FlowOutcome::RetryProviderSelection;
            }
        }
    } else {
        profile
    };

    if config.show_preview {
        styled_output::render_config_preview(&config_to_toml_preview(
            &final_profile,
            &config.profile_name,
        ));

        match prompt_yes_no("Proceed with this configuration?", true) {
            NavResult::Forward(Some(false)) | NavResult::Back => {
                return FlowOutcome::RetryProviderSelection;
            }
            NavResult::Cancel => return FlowOutcome::Cancelled,
            NavResult::Forward(Some(true)) | NavResult::Forward(None) => {}
        }
    }

    let telemetry = match save_to_profile(
        &config.config_path,
        &config.profile_name,
        final_profile.clone(),
    ) {
        Ok(telemetry) => telemetry,
        Err(error) => {
            styled_output::render_error(&format!("Failed to save configuration: {}", error));
            return FlowOutcome::RetryProviderSelection;
        }
    };

    println!();
    styled_output::render_success(&format!(
        "Successfully authenticated with {}!",
        provider_name
    ));
    styled_output::render_success("Configuration saved successfully");
    println!();

    FlowOutcome::Complete(Box::new(AuthFlowResult {
        profile: final_profile,
        telemetry,
    }))
}

fn render_auth_step_header(title: &str, config: &AuthFlowConfig) {
    styled_output::render_title(title);
    let steps = build_step_states(config.step_offset + 2, config.total_steps);
    styled_output::render_steps(&steps);
    println!();
}

fn render_default_model_for_provider(provider_id: &str) {
    let model = match provider_id {
        "anthropic" => DEFAULT_MODEL,
        "openai" => "gpt-4.1",
        "gemini" => "gemini-2.5-pro",
        "openrouter" => "openrouter/anthropic/claude-sonnet-4",
        "github-copilot" => "github-copilot/gpt-4o",
        _ => return,
    };
    styled_output::render_default_model(model);
}

fn build_step_states(active_step: usize, total_steps: usize) -> Vec<(String, StepStatus)> {
    (0..total_steps)
        .map(|index| {
            let status = if index < active_step {
                StepStatus::Completed
            } else if index == active_step {
                StepStatus::Active
            } else {
                StepStatus::Pending
            };
            (format!("Step {}", index + 1), status)
        })
        .collect()
}

fn build_provider_menu_options(
    include_special_options: bool,
) -> Vec<(ProviderSelection, String, bool)> {
    let registry = ProviderRegistry::new();
    let mut providers: Vec<_> = registry
        .list()
        .into_iter()
        .filter(|provider| provider.id() != "stakpak")
        .map(|provider| {
            (
                ProviderSelection::Registry(provider.id().to_string()),
                provider.name().to_string(),
                provider.id() == "anthropic",
            )
        })
        .collect();

    providers.sort_by_key(|(selection, label, _)| match selection {
        ProviderSelection::Registry(provider_id) => provider_order_key(provider_id, label),
        ProviderSelection::Hybrid => (200, label.clone()),
        ProviderSelection::Byom => (201, label.clone()),
    });

    if include_special_options {
        providers.push((
            ProviderSelection::Hybrid,
            "Hybrid providers (e.g., Google and Anthropic)".to_string(),
            false,
        ));
        providers.push((
            ProviderSelection::Byom,
            "Bring your own model".to_string(),
            false,
        ));
    }

    providers
}

fn provider_order_key(provider_id: &str, label: &str) -> (u8, String) {
    let priority = match provider_id {
        "anthropic" => 0,
        "openai" => 1,
        "gemini" => 2,
        "openrouter" => 3,
        "github-copilot" => 4,
        _ => 100,
    };
    (priority, label.to_string())
}

fn build_provider_profile(provider_id: &str) -> Result<ProfileConfig, String> {
    match provider_id {
        "anthropic" => Ok(generate_anthropic_profile()),
        "openai" => Ok(generate_openai_profile()),
        "gemini" => Ok(generate_gemini_profile()),
        "openrouter" => Ok(generate_openrouter_profile()),
        "github-copilot" => Ok(generate_github_copilot_profile()),
        other => Err(format!("Unsupported provider for auth flow: {}", other)),
    }
}

fn merge_with_existing_profile(
    config_path: &str,
    profile_name: &str,
    template: &ProfileConfig,
) -> Result<ProfileConfig, String> {
    let config_file = AppConfig::load_config_file(config_path)
        .map_err(|error| format!("Failed to load config file: {}", error))?;

    let Some(existing_profile) = config_file.profiles.get(profile_name) else {
        return Ok(template.clone());
    };

    let mut merged = existing_profile.clone();
    if merged.provider.is_none() {
        merged.provider = template.provider;
    }
    if merged.model.is_none() {
        merged.model = template.model.clone();
    }
    if merged.api_endpoint.is_none() {
        merged.api_endpoint = template.api_endpoint.clone();
    }
    if merged.api_key.is_none() {
        merged.api_key = template.api_key.clone();
    }

    for (provider_name, provider_config) in &template.providers {
        merged
            .providers
            .insert(provider_name.clone(), provider_config.clone());
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_shared::models::llm::ProviderConfig;

    #[test]
    fn provider_menu_includes_registry_entries_and_special_options() {
        let options = build_provider_menu_options(true);
        let labels: Vec<_> = options.iter().map(|(_, label, _)| label.as_str()).collect();

        assert_eq!(
            labels,
            vec![
                "Anthropic (Claude)",
                "OpenAI",
                "Google (Gemini)",
                "GitHub Copilot",
                "Hybrid providers (e.g., Google and Anthropic)",
                "Bring your own model",
            ]
        );
    }

    #[test]
    fn merge_with_existing_profile_preserves_existing_model_and_other_providers() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let config_path = temp_dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[settings]
editor = "nano"

[profiles.dev]
provider = "local"
model = "anthropic/claude-sonnet-4-5"

[profiles.dev.providers.openai]
type = "openai"
api_key = "existing-openai"
"#,
        )
        .expect("write config");

        let mut gemini_profile = generate_gemini_profile();
        if let Some(ProviderConfig::Gemini { auth, .. }) =
            gemini_profile.providers.get_mut("gemini")
        {
            *auth = Some(ProviderAuth::api_key("gemini-key"));
        }

        let merged = merge_with_existing_profile(
            config_path.to_str().expect("config path"),
            "dev",
            &gemini_profile,
        )
        .expect("merge profile");

        assert_eq!(merged.model.as_deref(), Some("anthropic/claude-sonnet-4-5"));
        assert!(merged.providers.contains_key("openai"));
        assert!(merged.providers.contains_key("gemini"));
        assert_eq!(
            merged
                .providers
                .get("openai")
                .and_then(|provider| provider.api_key()),
            Some("existing-openai")
        );
    }
}
