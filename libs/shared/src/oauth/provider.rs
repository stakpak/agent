//! OAuth provider trait and authentication method types

use super::config::OAuthConfig;
use super::error::OAuthResult;
use super::flow::TokenResponse;
use crate::models::auth::ProviderAuth;
use async_trait::async_trait;
use reqwest::header::HeaderMap;

/// Authentication method offered by a provider
#[derive(Debug, Clone)]
pub struct AuthMethod {
    /// Unique identifier for this method
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Description/hint for the user
    pub description: Option<String>,
    /// Type of authentication
    pub method_type: AuthMethodType,
}

impl AuthMethod {
    /// Create a new OAuth authentication method
    pub fn oauth(
        id: impl Into<String>,
        label: impl Into<String>,
        description: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description,
            method_type: AuthMethodType::OAuth,
        }
    }

    /// Create a new API key authentication method
    pub fn api_key(
        id: impl Into<String>,
        label: impl Into<String>,
        description: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description,
            method_type: AuthMethodType::ApiKey,
        }
    }

    /// Get a display string combining label and description
    pub fn display(&self) -> String {
        match &self.description {
            Some(desc) => format!("{} - {}", self.label, desc),
            None => self.label.clone(),
        }
    }
}

/// Type of authentication method
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMethodType {
    /// OAuth 2.0 browser-based flow
    OAuth,
    /// Manual API key entry
    ApiKey,
}

/// Trait for providers that support authentication
#[async_trait]
pub trait OAuthProvider: Send + Sync {
    /// Provider identifier (e.g., "anthropic")
    fn id(&self) -> &'static str;

    /// Human-readable provider name
    fn name(&self) -> &'static str;

    /// List available authentication methods
    fn auth_methods(&self) -> Vec<AuthMethod>;

    /// Get OAuth configuration for a specific method
    fn oauth_config(&self, method_id: &str) -> Option<OAuthConfig>;

    /// Post-authorization processing (e.g., exchange OAuth tokens for API key)
    ///
    /// This is called after the OAuth flow completes to convert the tokens
    /// into the appropriate `ProviderAuth` type.
    async fn post_authorize(
        &self,
        method_id: &str,
        tokens: &TokenResponse,
    ) -> OAuthResult<ProviderAuth>;

    /// Apply authentication to HTTP request headers
    ///
    /// This method modifies the provided headers to include the appropriate
    /// authentication headers for API requests.
    fn apply_auth_headers(&self, auth: &ProviderAuth, headers: &mut HeaderMap) -> OAuthResult<()>;

    /// Get the environment variable name for API key (if supported)
    fn api_key_env_var(&self) -> Option<&'static str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_method_oauth() {
        let method = AuthMethod::oauth("claude-max", "Claude Pro/Max", Some("Use your subscription".to_string()));
        
        assert_eq!(method.id, "claude-max");
        assert_eq!(method.label, "Claude Pro/Max");
        assert_eq!(method.description, Some("Use your subscription".to_string()));
        assert_eq!(method.method_type, AuthMethodType::OAuth);
    }

    #[test]
    fn test_auth_method_api_key() {
        let method = AuthMethod::api_key("api-key", "Manual API Key", None);
        
        assert_eq!(method.id, "api-key");
        assert_eq!(method.label, "Manual API Key");
        assert_eq!(method.description, None);
        assert_eq!(method.method_type, AuthMethodType::ApiKey);
    }

    #[test]
    fn test_auth_method_display() {
        let with_desc = AuthMethod::oauth(
            "test",
            "Test Method",
            Some("Description here".to_string()),
        );
        assert_eq!(with_desc.display(), "Test Method - Description here");

        let without_desc = AuthMethod::oauth("test", "Test Method", None);
        assert_eq!(without_desc.display(), "Test Method");
    }
}
