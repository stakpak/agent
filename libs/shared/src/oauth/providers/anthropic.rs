//! Anthropic OAuth provider implementation

use crate::models::auth::ProviderAuth;
use crate::oauth::config::OAuthConfig;
use crate::oauth::error::{OAuthError, OAuthResult};
use crate::oauth::flow::TokenResponse;
use crate::oauth::provider::{AuthMethod, OAuthProvider};
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde::Deserialize;

/// Anthropic OAuth provider
pub struct AnthropicProvider;

impl AnthropicProvider {
    /// Anthropic's public OAuth client ID
    const CLIENT_ID: &'static str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

    /// Token exchange endpoint
    const TOKEN_URL: &'static str = "https://console.anthropic.com/v1/oauth/token";

    /// OAuth redirect URL
    const REDIRECT_URL: &'static str = "https://console.anthropic.com/oauth/code/callback";

    /// OAuth scopes
    const SCOPES: &'static [&'static str] =
        &["org:create_api_key", "user:profile", "user:inference"];

    /// Claude.ai authorization URL (for Pro/Max subscriptions)
    const CLAUDE_AI_AUTH_URL: &'static str = "https://claude.ai/oauth/authorize";

    /// Console authorization URL (for API console)
    const CONSOLE_AUTH_URL: &'static str = "https://console.anthropic.com/oauth/authorize";

    /// Beta header for OAuth authentication
    const OAUTH_BETA_HEADER: &'static str =
        "oauth-2025-04-20,claude-code-20250219,interleaved-thinking-2025-05-14";

    /// Create a new Anthropic provider
    pub fn new() -> Self {
        Self
    }

    /// Create an API key from OAuth tokens (for "console" method)
    async fn create_api_key(&self, access_token: &str) -> OAuthResult<String> {
        let client =
            crate::tls_client::create_tls_client(crate::tls_client::TlsClientConfig::default())
                .unwrap_or_else(|_| reqwest::Client::new());
        let response = client
            .post("https://api.anthropic.com/api/oauth/claude_cli/create_api_key")
            .header("authorization", format!("Bearer {}", access_token))
            .header("content-type", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!(
                "Failed to create API key from OAuth tokens: {} - {}",
                status,
                error_text
            );
            return Err(OAuthError::ApiKeyCreationFailed);
        }

        #[derive(Deserialize)]
        struct ApiKeyResponse {
            raw_key: String,
        }

        let result: ApiKeyResponse = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse API key response: {}", e);
            OAuthError::ApiKeyCreationFailed
        })?;

        Ok(result.raw_key)
    }

    /// Helper to decode JWT payload (without verification)
    fn decode_jwt_payload(token: &str) -> Option<serde_json::Value> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        use base64::Engine;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let payload_part = parts[1];
        let decoded = match engine.decode(payload_part) {
            Ok(d) => d,
            Err(_) => {
                let rem = payload_part.len() % 4;
                if rem > 0 {
                    let padded = format!("{}{}", payload_part, "=".repeat(4 - rem));
                    engine.decode(&padded).ok()?
                } else {
                    return None;
                }
            }
        };

        serde_json::from_slice(&decoded).ok()
    }
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OAuthProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn name(&self) -> &'static str {
        "Anthropic (Claude)"
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![
            AuthMethod::oauth(
                "claude-max",
                "Claude Pro/Max",
                Some("Use your existing Claude subscription".to_string()),
            ),
            AuthMethod::oauth(
                "console",
                "Create API Key",
                Some("Generate a new API key from console.anthropic.com".to_string()),
            ),
            AuthMethod::api_key(
                "api-key",
                "Manual API Key",
                Some("Enter an existing API key".to_string()),
            ),
        ]
    }

    fn oauth_config(&self, method_id: &str) -> Option<OAuthConfig> {
        let auth_url = match method_id {
            "claude-max" => Self::CLAUDE_AI_AUTH_URL,
            "console" => Self::CONSOLE_AUTH_URL,
            _ => return None,
        };

        Some(OAuthConfig::new(
            Self::CLIENT_ID,
            auth_url,
            Self::TOKEN_URL,
            Self::REDIRECT_URL,
            Self::SCOPES.iter().map(|s| s.to_string()).collect(),
        ))
    }

    async fn post_authorize(
        &self,
        method_id: &str,
        tokens: &TokenResponse,
    ) -> OAuthResult<ProviderAuth> {
        match method_id {
            "claude-max" => {
                // Return OAuth tokens for direct API use
                let expires = chrono::Utc::now().timestamp_millis() + (tokens.expires_in * 1000);

                // Try to determine subscription tier from JWT claims
                let mut name = "Claude Pro/Max".to_string();
                if let Some(claims) = Self::decode_jwt_payload(&tokens.access_token)
                    && let Some(tier) = claims.get("tier").and_then(|v| v.as_str())
                {
                    match tier {
                        "pro" => name = "Claude Pro".to_string(),
                        "max" => name = "Claude Max".to_string(),
                        _ => {} // Keep default
                    }
                }

                Ok(ProviderAuth::oauth_with_name(
                    &tokens.access_token,
                    &tokens.refresh_token,
                    expires,
                    name,
                ))
            }
            "console" => {
                // Exchange OAuth tokens for permanent API key
                let api_key = self.create_api_key(&tokens.access_token).await?;
                Ok(ProviderAuth::api_key(api_key))
            }
            _ => Err(OAuthError::unknown_method(method_id)),
        }
    }

    fn apply_auth_headers(&self, auth: &ProviderAuth, headers: &mut HeaderMap) -> OAuthResult<()> {
        match auth {
            ProviderAuth::OAuth { access, .. } => {
                // OAuth: Use Bearer token
                headers.insert(
                    "authorization",
                    format!("Bearer {}", access)
                        .parse()
                        .map_err(|_| OAuthError::InvalidHeader)?,
                );
                // Required beta header for OAuth authentication
                headers.insert(
                    "anthropic-beta",
                    Self::OAUTH_BETA_HEADER
                        .parse()
                        .map_err(|_| OAuthError::InvalidHeader)?,
                );
                // Remove API key header if present (OAuth takes precedence)
                headers.remove("x-api-key");
            }
            ProviderAuth::Api { key } => {
                // API key: Use x-api-key header
                headers.insert(
                    "x-api-key",
                    key.parse().map_err(|_| OAuthError::InvalidHeader)?,
                );
                // Remove Authorization header if present
                headers.remove("authorization");
            }
        }
        Ok(())
    }

    fn api_key_env_var(&self) -> Option<&'static str> {
        Some("ANTHROPIC_API_KEY")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_and_name() {
        let provider = AnthropicProvider::new();
        assert_eq!(provider.id(), "anthropic");
        assert_eq!(provider.name(), "Anthropic (Claude)");
    }

    #[test]
    fn test_auth_methods() {
        let provider = AnthropicProvider::new();
        let methods = provider.auth_methods();

        assert_eq!(methods.len(), 3);

        assert_eq!(methods[0].id, "claude-max");
        assert_eq!(methods[0].label, "Claude Pro/Max");

        assert_eq!(methods[1].id, "console");
        assert_eq!(methods[1].label, "Create API Key");

        assert_eq!(methods[2].id, "api-key");
        assert_eq!(methods[2].label, "Manual API Key");
    }

    #[test]
    fn test_oauth_config_claude_max() {
        let provider = AnthropicProvider::new();
        let config = provider.oauth_config("claude-max").unwrap();

        assert_eq!(config.client_id, AnthropicProvider::CLIENT_ID);
        assert_eq!(config.auth_url, "https://claude.ai/oauth/authorize");
        assert_eq!(
            config.token_url,
            "https://console.anthropic.com/v1/oauth/token"
        );
    }

    #[test]
    fn test_oauth_config_console() {
        let provider = AnthropicProvider::new();
        let config = provider.oauth_config("console").unwrap();

        assert_eq!(
            config.auth_url,
            "https://console.anthropic.com/oauth/authorize"
        );
    }

    #[test]
    fn test_oauth_config_api_key_returns_none() {
        let provider = AnthropicProvider::new();
        assert!(provider.oauth_config("api-key").is_none());
    }

    #[test]
    fn test_oauth_config_unknown_method() {
        let provider = AnthropicProvider::new();
        assert!(provider.oauth_config("unknown").is_none());
    }

    #[test]
    fn test_apply_auth_headers_oauth() {
        let provider = AnthropicProvider::new();
        let auth = ProviderAuth::oauth("access-token", "refresh-token", 0);
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "old-key".parse().unwrap());

        provider.apply_auth_headers(&auth, &mut headers).unwrap();

        assert_eq!(headers.get("authorization").unwrap(), "Bearer access-token");
        assert!(headers.get("anthropic-beta").is_some());
        assert!(headers.get("x-api-key").is_none()); // Should be removed
    }

    #[test]
    fn test_apply_auth_headers_api_key() {
        let provider = AnthropicProvider::new();
        let auth = ProviderAuth::api_key("sk-ant-test-key");
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer old-token".parse().unwrap());

        provider.apply_auth_headers(&auth, &mut headers).unwrap();

        assert_eq!(headers.get("x-api-key").unwrap(), "sk-ant-test-key");
        assert!(headers.get("authorization").is_none()); // Should be removed
    }

    #[test]
    fn test_api_key_env_var() {
        let provider = AnthropicProvider::new();
        assert_eq!(provider.api_key_env_var(), Some("ANTHROPIC_API_KEY"));
    }

    #[tokio::test]
    async fn test_post_authorize_claude_pro() {
        let provider = AnthropicProvider::new();

        // Create a dummy JWT with "tier": "pro"
        let payload = r#"{"sub":"123","tier":"pro"}"#;
        use base64::Engine;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let encoded_payload = engine.encode(payload);
        let token = format!("header.{}.signature", encoded_payload);

        let tokens = TokenResponse {
            access_token: token,
            refresh_token: "refresh".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
        };

        let auth = provider
            .post_authorize("claude-max", &tokens)
            .await
            .unwrap();

        match auth {
            ProviderAuth::OAuth { name, .. } => {
                assert_eq!(name, Some("Claude Pro".to_string()));
            }
            _ => panic!("Expected OAuth auth"),
        }
    }

    #[tokio::test]
    async fn test_post_authorize_claude_max() {
        let provider = AnthropicProvider::new();

        // Create a dummy JWT with "tier": "max"
        let payload = r#"{"sub":"123","tier":"max"}"#;
        use base64::Engine;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let encoded_payload = engine.encode(payload);
        let token = format!("header.{}.signature", encoded_payload);

        let tokens = TokenResponse {
            access_token: token,
            refresh_token: "refresh".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
        };

        let auth = provider
            .post_authorize("claude-max", &tokens)
            .await
            .unwrap();

        match auth {
            ProviderAuth::OAuth { name, .. } => {
                assert_eq!(name, Some("Claude Max".to_string()));
            }
            _ => panic!("Expected OAuth auth"),
        }
    }
}
