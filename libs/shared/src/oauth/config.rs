//! OAuth configuration types

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
        }
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
