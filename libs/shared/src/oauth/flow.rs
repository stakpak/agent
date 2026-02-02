//! OAuth 2.0 authorization code flow implementation

use super::config::OAuthConfig;
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

/// OAuth 2.0 authorization code flow handler
pub struct OAuthFlow {
    config: OAuthConfig,
    pkce: Option<PkceChallenge>,
}

impl OAuthFlow {
    /// Create a new OAuth flow with the given configuration
    pub fn new(config: OAuthConfig) -> Self {
        Self { config, pkce: None }
    }

    /// Generate the authorization URL for the user to visit
    ///
    /// This generates a new PKCE challenge and returns the full authorization URL
    /// that should be opened in the user's browser.
    pub fn generate_auth_url(&mut self) -> String {
        let pkce = PkceChallenge::generate();

        let url = format!(
            "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method={}&state={}",
            self.config.auth_url,
            urlencoding::encode(&self.config.client_id),
            urlencoding::encode(&self.config.redirect_url),
            urlencoding::encode(&self.config.scopes_string()),
            urlencoding::encode(&pkce.challenge),
            PkceChallenge::challenge_method(),
            urlencoding::encode(&pkce.verifier), // State contains verifier for validation
        );

        self.pkce = Some(pkce);
        url
    }

    /// Exchange authorization code for tokens
    ///
    /// The code should be in the format "authorization_code#state" as returned by Anthropic.
    pub async fn exchange_code(&self, code: &str) -> OAuthResult<TokenResponse> {
        let pkce = self.pkce.as_ref().ok_or(OAuthError::PkceNotInitialized)?;

        // Parse the authorization code - format: "authorization_code#state"
        let (auth_code, state) = parse_auth_code(code)?;

        // Validate state matches our verifier
        if state != pkce.verifier {
            return Err(OAuthError::invalid_code_format(
                "State mismatch - possible CSRF attack",
            ));
        }

        let client =
            crate::tls_client::create_tls_client(crate::tls_client::TlsClientConfig::default())
                .unwrap_or_else(|_| reqwest::Client::new());
        let response = client
            .post(&self.config.token_url)
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "code": auth_code,
                "state": state,
                "client_id": self.config.client_id,
                "redirect_uri": self.config.redirect_url,
                "code_verifier": pkce.verifier,
            }))
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
        let client =
            crate::tls_client::create_tls_client(crate::tls_client::TlsClientConfig::default())
                .unwrap_or_else(|_| reqwest::Client::new());
        let response = client
            .post(&self.config.token_url)
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": self.config.client_id,
            }))
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

/// Parse the authorization code from Anthropic's callback format
///
/// Anthropic returns codes in the format: "authorization_code#state"
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
    fn test_generate_auth_url() {
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

        // PKCE should be initialized
        assert!(flow.pkce.is_some());
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
