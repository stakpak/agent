//! OAuth 2.0 authorization code flow implementation

use super::config::{AuthorizationRequestMode, OAuthConfig, TokenRequestMode};
use super::error::{OAuthError, OAuthResult};
use super::pkce::PkceChallenge;
use serde::{Deserialize, Serialize};

/// OAuth token response from the token endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    /// Access token for API requests
    pub access_token: String,
    /// Refresh token for obtaining new access tokens
    pub refresh_token: String,
    /// Token lifetime in seconds
    pub expires_in: i64,
    /// Token type (usually "Bearer")
    pub token_type: String,
}

enum TokenRequest {
    Json(serde_json::Value),
    Form(Vec<(String, String)>),
}

/// OAuth 2.0 authorization code flow handler
pub struct OAuthFlow {
    config: OAuthConfig,
    pkce: Option<PkceChallenge>,
    state: Option<String>,
}

impl OAuthFlow {
    /// Create a new OAuth flow with the given configuration
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            pkce: None,
            state: None,
        }
    }

    /// Generate the authorization URL for the user to visit
    ///
    /// This generates a new PKCE challenge and returns the full authorization URL
    /// that should be opened in the user's browser.
    pub fn generate_auth_url(&mut self) -> String {
        let pkce = PkceChallenge::generate();
        let state = uuid::Uuid::new_v4().simple().to_string();

        let mut query = vec![
            format!("client_id={}", urlencoding::encode(&self.config.client_id)),
            "response_type=code".to_string(),
            format!(
                "redirect_uri={}",
                urlencoding::encode(&self.config.redirect_url)
            ),
            format!(
                "scope={}",
                urlencoding::encode(&self.config.scopes_string())
            ),
            format!("code_challenge={}", urlencoding::encode(&pkce.challenge)),
            format!(
                "code_challenge_method={}",
                PkceChallenge::challenge_method()
            ),
            format!("state={}", urlencoding::encode(&state)),
        ];

        if self.config.authorization_request_mode == AuthorizationRequestMode::LegacyCode {
            query.insert(0, "code=true".to_string());
        }

        query.extend(self.config.authorization_params.iter().map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        }));

        let url = format!("{}?{}", self.config.auth_url, query.join("&"));

        self.pkce = Some(pkce);
        self.state = Some(state);
        url
    }

    fn build_token_exchange_request(
        &self,
        auth_code: String,
        state: String,
    ) -> OAuthResult<TokenRequest> {
        let pkce = self.pkce.as_ref().ok_or(OAuthError::PkceNotInitialized)?;

        Ok(match self.config.token_request_mode {
            TokenRequestMode::Json => TokenRequest::Json(serde_json::json!({
                "grant_type": "authorization_code",
                "code": auth_code,
                "state": state,
                "client_id": self.config.client_id,
                "redirect_uri": self.config.redirect_url,
                "code_verifier": pkce.verifier,
            })),
            TokenRequestMode::FormUrlEncoded => TokenRequest::Form(vec![
                // OpenAI's token endpoint rejects `state` in the form-encoded
                // exchange request (`Unknown parameter: 'state'.`). State is
                // still validated locally before building this request.
                ("grant_type".to_string(), "authorization_code".to_string()),
                ("code".to_string(), auth_code),
                ("client_id".to_string(), self.config.client_id.clone()),
                ("redirect_uri".to_string(), self.config.redirect_url.clone()),
                ("code_verifier".to_string(), pkce.verifier.clone()),
            ]),
        })
    }

    fn build_token_refresh_request(&self, refresh_token: String) -> TokenRequest {
        match self.config.token_request_mode {
            TokenRequestMode::Json => TokenRequest::Json(serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": self.config.client_id,
            })),
            TokenRequestMode::FormUrlEncoded => TokenRequest::Form(vec![
                ("grant_type".to_string(), "refresh_token".to_string()),
                ("refresh_token".to_string(), refresh_token),
                ("client_id".to_string(), self.config.client_id.clone()),
            ]),
        }
    }

    /// Exchange authorization code for tokens.
    ///
    /// The string form is kept for manual copy/paste flows that return
    /// `authorization_code#state`. Programmatic callers should prefer
    /// `exchange_code_with_state` when they already have separate values.
    pub async fn exchange_code(&self, code: &str) -> OAuthResult<TokenResponse> {
        let (auth_code, state) = parse_auth_code(code)?;
        self.exchange_code_with_state(&auth_code, &state).await
    }

    /// Exchange authorization code for tokens using separately supplied code and state.
    pub async fn exchange_code_with_state(
        &self,
        auth_code: &str,
        state: &str,
    ) -> OAuthResult<TokenResponse> {
        let _pkce = self.pkce.as_ref().ok_or(OAuthError::PkceNotInitialized)?;

        let expected_state = self
            .state
            .as_deref()
            .ok_or(OAuthError::PkceNotInitialized)?;

        // Validate state matches the authorization request state before the
        // token exchange request is built.
        if state != expected_state {
            return Err(OAuthError::invalid_code_format(
                "State mismatch - possible CSRF attack",
            ));
        }

        let token_request =
            self.build_token_exchange_request(auth_code.to_string(), state.to_string())?;

        let client =
            crate::tls_client::create_tls_client(crate::tls_client::TlsClientConfig::default())
                .expect("Failed to create TLS client for OAuth token exchange");
        let response = match token_request {
            TokenRequest::Json(body) => client.post(&self.config.token_url).json(&body),
            TokenRequest::Form(body) => client.post(&self.config.token_url).form(&body),
        }
        .send()
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(OAuthError::token_exchange_failed(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        response.json::<TokenResponse>().await.map_err(|e| {
            OAuthError::token_exchange_failed(format!("Failed to parse token response: {}", e))
        })
    }

    /// Refresh an expired access token
    pub async fn refresh_token(&self, refresh_token: &str) -> OAuthResult<TokenResponse> {
        let token_request = self.build_token_refresh_request(refresh_token.to_string());
        let client =
            crate::tls_client::create_tls_client(crate::tls_client::TlsClientConfig::default())
                .expect("Failed to create TLS client for OAuth token refresh");
        let response = match token_request {
            TokenRequest::Json(body) => client.post(&self.config.token_url).json(&body),
            TokenRequest::Form(body) => client.post(&self.config.token_url).form(&body),
        }
        .send()
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(OAuthError::token_refresh_failed(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        response.json::<TokenResponse>().await.map_err(|e| {
            OAuthError::token_refresh_failed(format!("Failed to parse token response: {}", e))
        })
    }

    /// Get the PKCE verifier (for validation purposes)
    pub fn pkce_verifier(&self) -> Option<&str> {
        self.pkce.as_ref().map(|p| p.verifier.as_str())
    }
}

/// Parse the authorization code from a provider callback format that embeds state.
///
/// Some providers return codes in the format: "authorization_code#state".
#[allow(clippy::string_slice)] // pos from find('#') on same string, '#' is ASCII
fn parse_auth_code(code: &str) -> OAuthResult<(String, String)> {
    // Handle both "#" and "%23" (URL-encoded #)
    let code = code.replace("%23", "#");

    if let Some(pos) = code.find('#') {
        let auth_code = code[..pos].to_string();
        let state = code[pos + 1..].to_string();

        if auth_code.is_empty() || state.is_empty() {
            return Err(OAuthError::invalid_code_format(
                "Authorization code or state is empty",
            ));
        }

        Ok((auth_code, state))
    } else {
        Err(OAuthError::invalid_code_format(
            "Expected format: authorization_code#state",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::config::AuthorizationRequestMode;

    fn test_config() -> OAuthConfig {
        OAuthConfig::new(
            "test-client-id",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/callback",
            vec!["scope1".to_string(), "scope2".to_string()],
        )
    }

    #[test]
    fn test_generate_auth_url_standard_pkce() {
        let mut flow = OAuthFlow::new(test_config());
        let url = flow.generate_auth_url();

        assert!(url.starts_with("https://example.com/auth?"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("scope=scope1%20scope2"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state="));
        assert!(!url.contains("code=true"));

        // PKCE should be initialized
        assert!(flow.pkce.is_some());
    }

    #[test]
    fn test_generate_auth_url_legacy_mode_includes_code_param() {
        let mut flow = OAuthFlow::new(
            test_config().with_authorization_request_mode(AuthorizationRequestMode::LegacyCode),
        );
        let url = flow.generate_auth_url();

        assert!(url.contains("code=true"));
        assert!(url.contains("response_type=code"));
    }

    #[test]
    fn test_generate_auth_url_includes_provider_specific_params() {
        let mut flow = OAuthFlow::new(test_config().with_authorization_params(vec![
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", "stakpak"),
        ]));
        let url = flow.generate_auth_url();

        assert!(url.contains("id_token_add_organizations=true"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("originator=stakpak"));
    }

    #[test]
    fn test_generate_auth_url_uses_separate_state_from_pkce_verifier() {
        let mut flow = OAuthFlow::new(test_config());
        let url = flow.generate_auth_url();
        let parsed = reqwest::Url::parse(&url).expect("parse auth url");
        let state = parsed
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.to_string())
            .expect("state param");

        assert_ne!(Some(state.as_str()), flow.pkce_verifier());
    }

    #[test]
    fn test_openai_token_exchange_request_uses_form_encoding_without_state() {
        let mut flow = OAuthFlow::new(
            test_config()
                .with_token_request_mode(crate::oauth::config::TokenRequestMode::FormUrlEncoded),
        );
        let _ = flow.generate_auth_url();
        let request = flow
            .build_token_exchange_request("auth-code".to_string(), "callback-state".to_string())
            .expect("token exchange request");

        match request {
            TokenRequest::Form(params) => {
                assert!(
                    params.contains(&("grant_type".to_string(), "authorization_code".to_string()))
                );
                assert!(params.contains(&("code".to_string(), "auth-code".to_string())));
                assert!(params.contains(&("client_id".to_string(), "test-client-id".to_string())));
                assert!(params.iter().all(|(key, _)| key != "state"));
            }
            TokenRequest::Json(_) => panic!("expected form request"),
        }
    }

    #[test]
    fn test_openai_token_refresh_request_uses_form_encoding() {
        let flow = OAuthFlow::new(
            test_config()
                .with_token_request_mode(crate::oauth::config::TokenRequestMode::FormUrlEncoded),
        );
        let request = flow.build_token_refresh_request("refresh-token".to_string());

        match request {
            TokenRequest::Form(params) => {
                assert!(params.contains(&("grant_type".to_string(), "refresh_token".to_string())));
                assert!(
                    params.contains(&("refresh_token".to_string(), "refresh-token".to_string()))
                );
                assert!(params.contains(&("client_id".to_string(), "test-client-id".to_string())));
            }
            TokenRequest::Json(_) => panic!("expected form request"),
        }
    }

    #[test]
    fn test_parse_auth_code_valid() {
        let result = parse_auth_code("abc123#verifier456");
        assert!(result.is_ok());
        let (code, state) = result.unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "verifier456");
    }

    #[test]
    fn test_parse_auth_code_url_encoded() {
        let result = parse_auth_code("abc123%23verifier456");
        assert!(result.is_ok());
        let (code, state) = result.unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "verifier456");
    }

    #[test]
    fn test_parse_auth_code_missing_separator() {
        let result = parse_auth_code("abc123verifier456");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_auth_code_empty_parts() {
        assert!(parse_auth_code("#state").is_err());
        assert!(parse_auth_code("code#").is_err());
        assert!(parse_auth_code("#").is_err());
    }

    #[test]
    fn test_exchange_code_without_pkce() {
        let flow = OAuthFlow::new(test_config());
        let result = tokio_test::block_on(flow.exchange_code("code#state"));
        assert!(matches!(result, Err(OAuthError::PkceNotInitialized)));
    }

    #[test]
    fn test_exchange_code_with_state_without_pkce() {
        let flow = OAuthFlow::new(test_config());
        let result = tokio_test::block_on(flow.exchange_code_with_state("code", "state"));
        assert!(matches!(result, Err(OAuthError::PkceNotInitialized)));
    }

    #[test]
    fn test_token_response_serde() {
        let json = r#"{
            "access_token": "access123",
            "refresh_token": "refresh456",
            "expires_in": 3600,
            "token_type": "Bearer"
        }"#;

        let response: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.access_token, "access123");
        assert_eq!(response.refresh_token, "refresh456");
        assert_eq!(response.expires_in, 3600);
        assert_eq!(response.token_type, "Bearer");
    }
}
