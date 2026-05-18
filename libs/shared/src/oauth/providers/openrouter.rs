//! OpenRouter API key auth provider

use crate::models::auth::ProviderAuth;
use crate::oauth::config::OAuthConfig;
use crate::oauth::error::{OAuthError, OAuthResult};
use crate::oauth::flow::TokenResponse;
use crate::oauth::provider::{AuthMethod, OAuthProvider};
use async_trait::async_trait;
use reqwest::header::HeaderMap;

/// OpenRouter provider.
pub struct OpenRouterProvider;

impl OpenRouterProvider {
    /// Create a new OpenRouter provider.
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenRouterProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OAuthProvider for OpenRouterProvider {
    fn id(&self) -> &'static str {
        "openrouter"
    }

    fn name(&self) -> &'static str {
        "OpenRouter"
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::api_key(
            "api-key",
            "API Key",
            Some("Enter an existing OpenRouter API key".to_string()),
        )]
    }

    fn oauth_config(&self, _method_id: &str) -> Option<OAuthConfig> {
        None
    }

    async fn post_authorize(
        &self,
        method_id: &str,
        _tokens: &TokenResponse,
    ) -> OAuthResult<ProviderAuth> {
        Err(OAuthError::unknown_method(method_id))
    }

    fn apply_auth_headers(&self, auth: &ProviderAuth, headers: &mut HeaderMap) -> OAuthResult<()> {
        let api_key = match auth {
            ProviderAuth::Api { key } => key,
            ProviderAuth::OAuth { access, .. } => access,
        };

        headers.insert(
            "authorization",
            format!("Bearer {}", api_key)
                .parse()
                .map_err(|_| OAuthError::InvalidHeader)?,
        );
        Ok(())
    }

    fn api_key_env_var(&self) -> Option<&'static str> {
        Some("OPENROUTER_API_KEY")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_name_and_methods() {
        let provider = OpenRouterProvider::new();
        let methods = provider.auth_methods();

        assert_eq!(provider.id(), "openrouter");
        assert_eq!(provider.name(), "OpenRouter");
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].id, "api-key");
    }

    #[test]
    fn test_oauth_config_is_not_supported() {
        let provider = OpenRouterProvider::new();
        assert!(provider.oauth_config("api-key").is_none());
    }

    #[test]
    fn test_apply_auth_headers_api_key() {
        let provider = OpenRouterProvider::new();
        let auth = ProviderAuth::api_key("sk-or-v1-test-key");
        let mut headers = HeaderMap::new();

        let result = provider.apply_auth_headers(&auth, &mut headers);
        assert!(result.is_ok());
        assert_eq!(
            headers.get("authorization"),
            Some(&"Bearer sk-or-v1-test-key".parse().expect("valid header"))
        );
    }
}
