//! OAuth configuration types

/// Provider-specific authorization request shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthorizationRequestMode {
    /// Standard OAuth 2.0 Authorization Code + PKCE request.
    #[default]
    StandardPkce,
    /// Legacy request shape that includes `code=true`.
    LegacyCode,
}

/// Provider-specific token request encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenRequestMode {
    /// JSON request bodies used by legacy providers.
    #[default]
    Json,
    /// `application/x-www-form-urlencoded` request bodies.
    FormUrlEncoded,
}

/// Configuration for an OAuth 2.0 provider
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    /// OAuth client ID
    pub client_id: String,
    /// Authorization endpoint URL
    pub auth_url: String,
    /// Token exchange endpoint URL
    pub token_url: String,
    /// Redirect URI for authorization callback
    pub redirect_url: String,
    /// Scopes to request
    pub scopes: Vec<String>,
    /// Provider-specific authorization request mode.
    pub authorization_request_mode: AuthorizationRequestMode,
    /// Additional provider-specific authorization query parameters.
    pub authorization_params: Vec<(String, String)>,
    /// Provider-specific token request encoding.
    pub token_request_mode: TokenRequestMode,
}

impl OAuthConfig {
    /// Create a new OAuth configuration
    pub fn new(
        client_id: impl Into<String>,
        auth_url: impl Into<String>,
        token_url: impl Into<String>,
        redirect_url: impl Into<String>,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            auth_url: auth_url.into(),
            token_url: token_url.into(),
            redirect_url: redirect_url.into(),
            scopes,
            authorization_request_mode: AuthorizationRequestMode::StandardPkce,
            authorization_params: Vec::new(),
            token_request_mode: TokenRequestMode::Json,
        }
    }

    /// Override the authorization request mode for providers with non-standard requirements.
    pub fn with_authorization_request_mode(mut self, mode: AuthorizationRequestMode) -> Self {
        self.authorization_request_mode = mode;
        self
    }

    /// Add provider-specific authorization query parameters.
    pub fn with_authorization_params<K, V, I>(mut self, params: I) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        I: IntoIterator<Item = (K, V)>,
    {
        self.authorization_params = params
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self
    }

    /// Override the token request encoding for providers with non-standard requirements.
    pub fn with_token_request_mode(mut self, mode: TokenRequestMode) -> Self {
        self.token_request_mode = mode;
        self
    }

    /// Get the scopes as a space-separated string
    pub fn scopes_string(&self) -> String {
        self.scopes.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_config_creation() {
        let config = OAuthConfig::new(
            "client-id",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/callback",
            vec!["scope1".to_string(), "scope2".to_string()],
        );

        assert_eq!(config.client_id, "client-id");
        assert_eq!(config.auth_url, "https://example.com/auth");
        assert_eq!(config.token_url, "https://example.com/token");
        assert_eq!(config.redirect_url, "https://example.com/callback");
        assert_eq!(config.scopes, vec!["scope1", "scope2"]);
        assert_eq!(
            config.authorization_request_mode,
            AuthorizationRequestMode::StandardPkce
        );
        assert!(config.authorization_params.is_empty());
        assert_eq!(config.token_request_mode, TokenRequestMode::Json);
    }

    #[test]
    fn test_authorization_request_mode_builder() {
        let config = OAuthConfig::new(
            "client-id",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/callback",
            vec!["scope".to_string()],
        )
        .with_authorization_request_mode(AuthorizationRequestMode::LegacyCode);

        assert_eq!(
            config.authorization_request_mode,
            AuthorizationRequestMode::LegacyCode
        );
    }

    #[test]
    fn test_authorization_params_builder() {
        let config = OAuthConfig::new(
            "client-id",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/callback",
            vec!["scope".to_string()],
        )
        .with_authorization_params(vec![("originator", "stakpak"), ("mode", "codex")]);

        assert_eq!(
            config.authorization_params,
            vec![
                ("originator".to_string(), "stakpak".to_string()),
                ("mode".to_string(), "codex".to_string()),
            ]
        );
    }

    #[test]
    fn test_token_request_mode_builder() {
        let config = OAuthConfig::new(
            "client-id",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/callback",
            vec!["scope".to_string()],
        )
        .with_token_request_mode(TokenRequestMode::FormUrlEncoded);

        assert_eq!(config.token_request_mode, TokenRequestMode::FormUrlEncoded);
    }

    #[test]
    fn test_scopes_string() {
        let config = OAuthConfig::new(
            "client-id",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/callback",
            vec!["read".to_string(), "write".to_string(), "admin".to_string()],
        );

        assert_eq!(config.scopes_string(), "read write admin");
    }

    #[test]
    fn test_empty_scopes() {
        let config = OAuthConfig::new(
            "client-id",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/callback",
            vec![],
        );

        assert_eq!(config.scopes_string(), "");
    }
}
