//! OpenAI OAuth provider for ChatGPT Plus/Pro Codex access

use crate::models::auth::ProviderAuth;
use crate::oauth::config::{AuthorizationRequestMode, OAuthConfig, TokenRequestMode};
use crate::oauth::error::{OAuthError, OAuthResult};
use crate::oauth::flow::TokenResponse;
use crate::oauth::provider::{AuthMethod, OAuthProvider};
use async_trait::async_trait;
use reqwest::header::HeaderMap;

/// OpenAI provider with ChatGPT Plus/Pro OAuth support.
pub struct OpenAICodexProvider;

impl OpenAICodexProvider {
    pub const CLIENT_ID: &'static str = "app_EMoamEEZ73f0CkXaXp7hrann";
    const AUTH_URL: &'static str = "https://auth.openai.com/oauth/authorize";
    const TOKEN_URL: &'static str = "https://auth.openai.com/oauth/token";
    const REDIRECT_URL: &'static str = "http://localhost:1455/auth/callback";
    const SCOPES: &'static [&'static str] = &["openid", "profile", "email", "offline_access"];
    const CODEX_METHOD_ID: &'static str = "chatgpt-plus-pro";
    const CODEX_METHOD_LABEL: &'static str = "ChatGPT Plus/Pro";
    const CLIENT_ID_OVERRIDE_ENV: &'static str = "STAKPAK_OPENAI_OAUTH_CLIENT_ID";

    pub fn new() -> Self {
        Self
    }

    fn client_id() -> String {
        std::env::var(Self::CLIENT_ID_OVERRIDE_ENV).unwrap_or_else(|_| Self::CLIENT_ID.to_string())
    }
}

impl Default for OpenAICodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OAuthProvider for OpenAICodexProvider {
    fn id(&self) -> &'static str {
        "openai"
    }

    fn name(&self) -> &'static str {
        "OpenAI"
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![
            AuthMethod::oauth(
                Self::CODEX_METHOD_ID,
                Self::CODEX_METHOD_LABEL,
                Some("Use your ChatGPT Plus/Pro subscription".to_string()),
            ),
            AuthMethod::api_key(
                "api-key",
                "API Key",
                Some("Enter an existing OpenAI API key".to_string()),
            ),
        ]
    }

    fn oauth_config(&self, method_id: &str) -> Option<OAuthConfig> {
        if method_id != Self::CODEX_METHOD_ID {
            return None;
        }

        Some(
            OAuthConfig::new(
                Self::client_id(),
                Self::AUTH_URL,
                Self::TOKEN_URL,
                Self::REDIRECT_URL,
                Self::SCOPES.iter().map(|scope| scope.to_string()).collect(),
            )
            .with_authorization_request_mode(AuthorizationRequestMode::StandardPkce)
            .with_authorization_params(vec![
                ("id_token_add_organizations", "true"),
                ("codex_cli_simplified_flow", "true"),
                ("originator", "stakpak"),
            ])
            .with_token_request_mode(TokenRequestMode::FormUrlEncoded),
        )
    }

    async fn post_authorize(
        &self,
        method_id: &str,
        tokens: &TokenResponse,
    ) -> OAuthResult<ProviderAuth> {
        if method_id != Self::CODEX_METHOD_ID {
            return Err(OAuthError::unknown_method(method_id));
        }

        let expires = chrono::Utc::now().timestamp_millis() + (tokens.expires_in * 1000);
        Ok(ProviderAuth::oauth_with_name(
            &tokens.access_token,
            &tokens.refresh_token,
            expires,
            Self::CODEX_METHOD_LABEL,
        ))
    }

    fn apply_auth_headers(&self, auth: &ProviderAuth, headers: &mut HeaderMap) -> OAuthResult<()> {
        let bearer_token = match auth {
            ProviderAuth::Api { key } => key,
            ProviderAuth::OAuth { access, .. } => access,
        };

        headers.insert(
            "authorization",
            format!("Bearer {}", bearer_token)
                .parse()
                .map_err(|_| OAuthError::InvalidHeader)?,
        );
        Ok(())
    }

    fn api_key_env_var(&self) -> Option<&'static str> {
        Some("OPENAI_API_KEY")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::config::AuthorizationRequestMode;
    use std::sync::Mutex;

    static OPENAI_CLIENT_ID_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_provider_id_and_name() {
        let provider = OpenAICodexProvider::new();
        assert_eq!(provider.id(), "openai");
        assert_eq!(provider.name(), "OpenAI");
    }

    #[test]
    fn test_auth_methods() {
        let provider = OpenAICodexProvider::new();
        let methods = provider.auth_methods();

        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].id, "chatgpt-plus-pro");
        assert_eq!(methods[0].label, "ChatGPT Plus/Pro");
        assert_eq!(methods[1].id, "api-key");
    }

    #[test]
    fn test_oauth_config_for_codex() {
        let _guard = OPENAI_CLIENT_ID_ENV_LOCK
            .lock()
            .expect("lock client id env");
        unsafe {
            std::env::remove_var("STAKPAK_OPENAI_OAUTH_CLIENT_ID");
        }

        let provider = OpenAICodexProvider::new();
        let config = provider
            .oauth_config("chatgpt-plus-pro")
            .expect("oauth config");

        assert_eq!(config.client_id, OpenAICodexProvider::CLIENT_ID);
        assert_eq!(config.auth_url, "https://auth.openai.com/oauth/authorize");
        assert_eq!(config.token_url, "https://auth.openai.com/oauth/token");
        assert_eq!(config.redirect_url, "http://localhost:1455/auth/callback");
        assert_eq!(
            config.scopes.join(" "),
            "openid profile email offline_access"
        );
        assert_eq!(
            config.authorization_params,
            vec![
                ("id_token_add_organizations".to_string(), "true".to_string()),
                ("codex_cli_simplified_flow".to_string(), "true".to_string()),
                ("originator".to_string(), "stakpak".to_string()),
            ]
        );
        assert_eq!(
            config.authorization_request_mode,
            AuthorizationRequestMode::StandardPkce
        );
        assert_eq!(config.token_request_mode, TokenRequestMode::FormUrlEncoded);
    }

    #[test]
    fn test_oauth_config_uses_env_override_for_client_id() {
        let _guard = OPENAI_CLIENT_ID_ENV_LOCK
            .lock()
            .expect("lock client id env");
        let provider = OpenAICodexProvider::new();
        unsafe {
            std::env::set_var("STAKPAK_OPENAI_OAUTH_CLIENT_ID", "app_override_test");
        }

        let config = provider
            .oauth_config("chatgpt-plus-pro")
            .expect("oauth config");

        assert_eq!(config.client_id, "app_override_test");

        unsafe {
            std::env::remove_var("STAKPAK_OPENAI_OAUTH_CLIENT_ID");
        }
    }

    #[tokio::test]
    async fn test_post_authorize_returns_named_oauth_auth() {
        let provider = OpenAICodexProvider::new();
        let tokens = TokenResponse {
            access_token: "access-token".to_string(),
            refresh_token: "refresh-token".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
        };

        let auth = provider
            .post_authorize("chatgpt-plus-pro", &tokens)
            .await
            .expect("oauth auth");

        match auth {
            ProviderAuth::OAuth {
                access,
                refresh,
                name,
                ..
            } => {
                assert_eq!(access, "access-token");
                assert_eq!(refresh, "refresh-token");
                assert_eq!(name.as_deref(), Some("ChatGPT Plus/Pro"));
            }
            _ => panic!("expected oauth auth"),
        }
    }
}
