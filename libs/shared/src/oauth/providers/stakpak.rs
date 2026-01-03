//! Stakpak provider implementation (API key only)

use crate::models::auth::ProviderAuth;
use crate::oauth::config::OAuthConfig;
use crate::oauth::error::{OAuthError, OAuthResult};
use crate::oauth::flow::TokenResponse;
use crate::oauth::provider::{AuthMethod, OAuthProvider};
use async_trait::async_trait;
use reqwest::header::HeaderMap;

/// Stakpak provider for remote API authentication
pub struct StakpakProvider;

impl StakpakProvider {
    /// Create a new Stakpak provider
    pub fn new() -> Self {
        Self
    }
}

impl Default for StakpakProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OAuthProvider for StakpakProvider {
    fn id(&self) -> &'static str {
        "stakpak"
    }

    fn name(&self) -> &'static str {
        "Stakpak"
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::api_key(
            "api-key",
            "API Key",
            Some("Enter your Stakpak API key".to_string()),
        )]
    }

    fn oauth_config(&self, _method_id: &str) -> Option<OAuthConfig> {
        // Stakpak only supports API key authentication
        None
    }

    async fn post_authorize(
        &self,
        _method_id: &str,
        _tokens: &TokenResponse,
    ) -> OAuthResult<ProviderAuth> {
        // Stakpak doesn't use OAuth, this should not be called
        Err(OAuthError::unknown_method("oauth"))
    }

    fn apply_auth_headers(&self, auth: &ProviderAuth, headers: &mut HeaderMap) -> OAuthResult<()> {
        match auth {
            ProviderAuth::Api { key } => {
                headers.insert(
                    "authorization",
                    format!("Bearer {}", key)
                        .parse()
                        .map_err(|_| OAuthError::InvalidHeader)?,
                );
                Ok(())
            }
            ProviderAuth::OAuth { .. } => {
                // Stakpak doesn't support OAuth
                Err(OAuthError::unknown_method("oauth"))
            }
        }
    }

    fn api_key_env_var(&self) -> Option<&'static str> {
        Some("STAKPAK_API_KEY")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_and_name() {
        let provider = StakpakProvider::new();
        assert_eq!(provider.id(), "stakpak");
        assert_eq!(provider.name(), "Stakpak");
    }

    #[test]
    fn test_auth_methods() {
        let provider = StakpakProvider::new();
        let methods = provider.auth_methods();

        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].id, "api-key");
        assert_eq!(methods[0].label, "API Key");
    }

    #[test]
    fn test_oauth_config_returns_none() {
        let provider = StakpakProvider::new();
        assert!(provider.oauth_config("api-key").is_none());
        assert!(provider.oauth_config("oauth").is_none());
    }

    #[test]
    fn test_apply_auth_headers_api_key() {
        let provider = StakpakProvider::new();
        let auth = ProviderAuth::api_key("test-api-key");
        let mut headers = HeaderMap::new();

        provider.apply_auth_headers(&auth, &mut headers).unwrap();

        assert_eq!(headers.get("authorization").unwrap(), "Bearer test-api-key");
    }

    #[test]
    fn test_api_key_env_var() {
        let provider = StakpakProvider::new();
        assert_eq!(provider.api_key_env_var(), Some("STAKPAK_API_KEY"));
    }
}
