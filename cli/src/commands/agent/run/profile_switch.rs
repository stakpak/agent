use crate::config::AppConfig;
use stakpak_api::{AgentClient, AgentClientConfig, AgentProvider, StakpakConfig};
use tokio::time::Duration;

const MAX_RETRIES: u32 = 2;

/// Validate profile switch before committing to it
/// - Loads the new profile configuration
/// - Inherits API key from default if new profile doesn't have one
/// - Validates API key with retry logic
pub async fn validate_profile_switch(
    new_profile: &str,
    config_path: Option<&str>,
    default_api_key: Option<String>,
) -> Result<AppConfig, String> {
    // 1. Try to load the new profile config
    let mut new_config = AppConfig::load(new_profile, config_path)
        .map_err(|e| format!("Failed to load profile '{}': {}", new_profile, e))?;

    // 2. Handle API key - inherit from default if not present
    // Note: With AgentClient, no API key is fine - it will use local providers
    if new_config.api_key.is_none()
        && let Some(default_key) = default_api_key
    {
        new_config.api_key = Some(default_key);
    }

    // 3. Create AgentClient and test API connection with retry logic
    let client: Box<dyn AgentProvider> = {
        // Use credential resolution with auth.toml fallback chain
        let stakpak = new_config
            .get_stakpak_api_key()
            .map(|api_key| StakpakConfig {
                api_key,
                api_endpoint: new_config.api_endpoint.clone(),
            });

        let client = AgentClient::new(AgentClientConfig {
            stakpak,
            providers: new_config.get_llm_provider_config(),
            eco_model: new_config.eco_model.clone(),
            recovery_model: new_config.recovery_model.clone(),
            smart_model: new_config.smart_model.clone(),
            store_path: None,
            hook_registry: None,
        })
        .await
        .map_err(|e| format!("Failed to create agent client: {}", e))?;
        Box::new(client)
    };

    let mut last_error = String::new();
    for attempt in 1..=MAX_RETRIES {
        match client.get_my_account().await {
            Ok(_) => {
                // Success!
                return Ok(new_config);
            }
            Err(e) => {
                // If no Stakpak key, the local stub account is returned, so this should succeed
                // If it fails, it's a real error
                last_error = e;
                if attempt < MAX_RETRIES {
                    // Wait before retry (exponential backoff)
                    tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
                }
            }
        }
    }

    Err(format!(
        "API validation failed after {} attempts: {}",
        MAX_RETRIES, last_error
    ))
}
