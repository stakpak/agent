//! GitHub Copilot OAuth provider
//!
//! Uses the GitHub Device Authorization Grant (RFC 8628) to obtain an OAuth
//! token that can be passed directly as a `Bearer` token to the Copilot API.

use crate::models::auth::ProviderAuth;
use crate::oauth::config::OAuthConfig;
use crate::oauth::device_flow::{DeviceFlow, DeviceTokenResponse};
use crate::oauth::error::{OAuthError, OAuthResult};
use crate::oauth::flow::TokenResponse;
use crate::oauth::provider::{AuthMethod, AuthMethodType, OAuthProvider};
use async_trait::async_trait;
use reqwest::header::HeaderMap;

/// GitHub's device-flow response contains no expiry for the OAuth token
/// if the GitHub OAuth token is revoked, the user must re-authenticate manually.
const GITHUB_TOKEN_NO_EXPIRY: i64 = i64::MAX;

/// GitHub Copilot OAuth provider
///
/// Registers a single auth method: `"device-flow"`, which triggers the
/// GitHub Device Authorization Grant instead of the PKCE browser flow.
pub struct GitHubCopilotProvider;

impl GitHubCopilotProvider {
    /// GitHub OAuth App client ID used by GitHub Copilot integrations.
    ///
    /// OAuth App client IDs are **not secret** — they identify the application
    /// and are safe to embed in source code.  Only the `client_secret` (unused
    /// in the device flow) must be kept confidential.
    pub const CLIENT_ID: &'static str = "Ov23li6Jke610XFUzJf5";

    /// OAuth scope required for Copilot API access.
    /// `read:user` is sufficient — Copilot authorization is bound to the
    /// GitHub account's Copilot subscription, not to a specific scope.
    pub const SCOPE: &'static str = "read:user";

    /// GitHub's device-code request endpoint.
    const DEVICE_CODE_URL: &'static str = "https://github.com/login/device/code";

    /// GitHub's token-polling endpoint.
    const TOKEN_URL: &'static str = "https://github.com/login/oauth/access_token";

    /// Create a new GitHub Copilot provider
    pub fn new() -> Self {
        Self
    }
}

impl Default for GitHubCopilotProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OAuthProvider for GitHubCopilotProvider {
    fn id(&self) -> &'static str {
        "github-copilot"
    }

    fn name(&self) -> &'static str {
        "GitHub Copilot"
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod {
            id: "device-flow".to_string(),
            label: "GitHub Device Flow".to_string(),
            description: Some(
                "Authenticate with your GitHub account (GitHub Copilot subscription required)"
                    .to_string(),
            ),
            method_type: AuthMethodType::DeviceFlow,
        }]
    }

    fn oauth_config(&self, _method_id: &str) -> Option<OAuthConfig> {
        // The device flow does not use the PKCE-based OAuthConfig; return None
        // so the caller knows to use the DeviceFlow path instead.
        None
    }

    fn device_flow(&self, method_id: &str) -> OAuthResult<DeviceFlow> {
        if method_id != "device-flow" {
            return Err(OAuthError::unknown_method(method_id));
        }
        DeviceFlow::new(
            Self::CLIENT_ID,
            vec![Self::SCOPE.to_string()],
            Self::DEVICE_CODE_URL,
            Self::TOKEN_URL,
        )
    }

    async fn post_authorize(
        &self,
        method_id: &str,
        _tokens: &TokenResponse,
    ) -> OAuthResult<ProviderAuth> {
        // GitHub Copilot uses the Device Authorization Grant, not the PKCE
        // authorization-code flow.  post_authorize (PKCE path) is not
        // applicable; callers should use post_device_authorize instead.
        Err(OAuthError::unknown_method(format!(
            "GitHub Copilot method '{}' uses the device flow — call post_device_authorize instead",
            method_id,
        )))
    }

    async fn post_device_authorize(
        &self,
        method_id: &str,
        token: &DeviceTokenResponse,
    ) -> OAuthResult<ProviderAuth> {
        if method_id != "device-flow" {
            return Err(OAuthError::unknown_method(method_id));
        }
        Ok(ProviderAuth::oauth_with_name(
            &token.access_token,
            "", // no refresh token in GitHub device-flow responses
            GITHUB_TOKEN_NO_EXPIRY,
            "GitHub Copilot".to_string(),
        ))
    }

    fn apply_auth_headers(&self, auth: &ProviderAuth, headers: &mut HeaderMap) -> OAuthResult<()> {
        match auth {
            ProviderAuth::OAuth { access, .. } => {
                headers.insert(
                    "authorization",
                    format!("Bearer {}", access)
                        .parse()
                        .map_err(|_| OAuthError::InvalidHeader)?,
                );
                Ok(())
            }
            ProviderAuth::Api { key } => {
                // Also accept plain API key (future-proofing)
                headers.insert(
                    "authorization",
                    format!("Bearer {}", key)
                        .parse()
                        .map_err(|_| OAuthError::InvalidHeader)?,
                );
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_and_name() {
        let provider = GitHubCopilotProvider::new();
        assert_eq!(provider.id(), "github-copilot");
        assert_eq!(provider.name(), "GitHub Copilot");
    }

    #[test]
    fn test_auth_methods() {
        let provider = GitHubCopilotProvider::new();
        let methods = provider.auth_methods();

        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].id, "device-flow");
        assert_eq!(methods[0].method_type, AuthMethodType::DeviceFlow);
    }

    #[test]
    fn test_oauth_config_returns_none() {
        let provider = GitHubCopilotProvider::new();
        assert!(provider.oauth_config("device-flow").is_none());
    }

    #[test]
    fn test_apply_auth_headers_oauth() {
        let provider = GitHubCopilotProvider::new();
        let auth = ProviderAuth::oauth("ghu_testtoken", "", 0);
        let mut headers = HeaderMap::new();

        provider.apply_auth_headers(&auth, &mut headers).unwrap();

        assert_eq!(
            headers.get("authorization").unwrap(),
            "Bearer ghu_testtoken"
        );
    }

    #[tokio::test]
    async fn test_post_authorize_returns_error() {
        // post_authorize (PKCE path) must not be called for a device-flow-only
        // provider; it should return an error so callers are directed to
        // post_device_authorize instead.
        let provider = GitHubCopilotProvider::new();
        let tokens = TokenResponse {
            access_token: "ghu_testtoken123".to_string(),
            refresh_token: String::new(),
            expires_in: 0,
            token_type: "bearer".to_string(),
        };
        let result = provider.post_authorize("device-flow", &tokens).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_post_device_authorize_device_flow() {
        use crate::oauth::device_flow::DeviceTokenResponse;

        let provider = GitHubCopilotProvider::new();
        let token = DeviceTokenResponse {
            access_token: "ghu_testtoken123".to_string(),
            token_type: "bearer".to_string(),
            scope: "read:user".to_string(),
        };

        let auth = provider
            .post_device_authorize("device-flow", &token)
            .await
            .unwrap();

        match auth {
            ProviderAuth::OAuth { access, name, .. } => {
                assert_eq!(access, "ghu_testtoken123");
                assert_eq!(name, Some("GitHub Copilot".to_string()));
            }
            _ => panic!("Expected OAuth auth"),
        }
    }

    #[tokio::test]
    async fn test_post_device_authorize_unknown_method() {
        use crate::oauth::device_flow::DeviceTokenResponse;

        let provider = GitHubCopilotProvider::new();
        let token = DeviceTokenResponse {
            access_token: "token".to_string(),
            token_type: "bearer".to_string(),
            scope: String::new(),
        };

        let result = provider.post_device_authorize("unknown", &token).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_no_expiry_constant_is_i64_max() {
        // Ensure the sentinel is never accidentally changed to a real timestamp.
        assert_eq!(GITHUB_TOKEN_NO_EXPIRY, i64::MAX);
    }
}
