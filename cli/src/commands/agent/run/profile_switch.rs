use crate::config::{AppConfig, ProviderType};
use stakpak_api::{
    AgentProvider,
    local::{LocalClient, LocalClientConfig},
    remote::RemoteClient,
};
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
    if new_config.api_key.is_none() {
        if let Some(default_key) = default_api_key {
            new_config.api_key = Some(default_key);
        } else {
            // Only error if provider is remote
            if matches!(new_config.provider, ProviderType::Remote) {
                return Err(format!(
                    "Profile '{}' has no API key and no default key available",
                    new_profile
                ));
            }
        }
    }

    // 3. Test API key with retry logic
    let client: Box<dyn AgentProvider> = match new_config.provider {
        ProviderType::Remote => {
            let client = RemoteClient::new(&new_config.clone().into())
                .map_err(|e| format!("Failed to create API client: {}", e))?;
            Box::new(client)
        }
        ProviderType::Local => {
            let client = LocalClient::new(LocalClientConfig { store_path: None })
                .await
                .map_err(|e| format!("Failed to create local client: {}", e))?;
            Box::new(client)
        }
    };

    let mut last_error = String::new();
    for attempt in 1..=MAX_RETRIES {
        match client.get_my_account().await {
            Ok(_) => {
                // Success!
                return Ok(new_config);
            }
            Err(e) => {
                // If local provider returns "Not Implemented", we consider it a success for now as it doesn't support account check
                if matches!(new_config.provider, ProviderType::Local) {
                    return Ok(new_config);
                }

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
