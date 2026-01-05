//! Authentication credentials for LLM providers
//!
//! This module defines the `ProviderAuth` enum which represents different
//! authentication methods for LLM providers (API key or OAuth tokens).

use serde::{Deserialize, Serialize};

/// Authentication credentials for an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderAuth {
    /// API key authentication
    Api {
        /// The API key
        key: String,
    },

    /// OAuth 2.0 authentication with refresh tokens
    #[serde(rename = "oauth")]
    OAuth {
        /// Access token for API requests
        access: String,
        /// Refresh token for obtaining new access tokens
        refresh: String,
        /// Expiration timestamp in milliseconds since Unix epoch
        expires: i64,
        /// Optional name for the subscription (e.g. "Claude Pro", "Claude Max")
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

impl ProviderAuth {
    /// Create a new API key authentication
    pub fn api_key(key: impl Into<String>) -> Self {
        Self::Api { key: key.into() }
    }

    /// Create a new OAuth authentication
    pub fn oauth(access: impl Into<String>, refresh: impl Into<String>, expires: i64) -> Self {
        Self::OAuth {
            access: access.into(),
            refresh: refresh.into(),
            expires,
            name: None,
        }
    }

    /// Create a new OAuth authentication with subscription name
    pub fn oauth_with_name(
        access: impl Into<String>,
        refresh: impl Into<String>,
        expires: i64,
        name: impl Into<String>,
    ) -> Self {
        Self::OAuth {
            access: access.into(),
            refresh: refresh.into(),
            expires,
            name: Some(name.into()),
        }
    }

    /// Check if OAuth token needs refresh (within 5 minutes of expiry)
    pub fn needs_refresh(&self) -> bool {
        match self {
            Self::OAuth { expires, .. } => {
                let now_ms = chrono::Utc::now().timestamp_millis();
                let buffer_ms = 5 * 60 * 1000; // 5 minutes
                *expires < (now_ms + buffer_ms)
            }
            Self::Api { .. } => false,
        }
    }

    /// Check if OAuth token is expired
    pub fn is_expired(&self) -> bool {
        match self {
            Self::OAuth { expires, .. } => *expires < chrono::Utc::now().timestamp_millis(),
            Self::Api { .. } => false,
        }
    }

    /// Get the API key if this is an API key auth
    pub fn api_key_value(&self) -> Option<&str> {
        match self {
            Self::Api { key } => Some(key),
            Self::OAuth { .. } => None,
        }
    }

    /// Get the access token if this is an OAuth auth
    pub fn access_token(&self) -> Option<&str> {
        match self {
            Self::OAuth { access, .. } => Some(access),
            Self::Api { .. } => None,
        }
    }

    /// Get the refresh token if this is an OAuth auth
    pub fn refresh_token(&self) -> Option<&str> {
        match self {
            Self::OAuth { refresh, .. } => Some(refresh),
            Self::Api { .. } => None,
        }
    }

    /// Check if this is an OAuth authentication
    pub fn is_oauth(&self) -> bool {
        matches!(self, Self::OAuth { .. })
    }

    /// Check if this is an API key authentication
    pub fn is_api_key(&self) -> bool {
        matches!(self, Self::Api { .. })
    }

    /// Get a display-safe representation of the auth type
    pub fn auth_type_display(&self) -> &'static str {
        match self {
            Self::Api { .. } => "api_key",
            Self::OAuth { .. } => "oauth",
        }
    }

    /// Get the subscription name if this is an OAuth auth with a name
    pub fn subscription_name(&self) -> Option<&str> {
        match self {
            Self::OAuth { name, .. } => name.as_deref(),
            Self::Api { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_creation() {
        let auth = ProviderAuth::api_key("sk-test-key");
        assert!(auth.is_api_key());
        assert!(!auth.is_oauth());
        assert_eq!(auth.api_key_value(), Some("sk-test-key"));
        assert_eq!(auth.access_token(), None);
    }

    #[test]
    fn test_oauth_creation() {
        let expires = chrono::Utc::now().timestamp_millis() + 3600000; // 1 hour from now
        let auth = ProviderAuth::oauth("access-token", "refresh-token", expires);
        assert!(auth.is_oauth());
        assert!(!auth.is_api_key());
        assert_eq!(auth.access_token(), Some("access-token"));
        assert_eq!(auth.refresh_token(), Some("refresh-token"));
        assert_eq!(auth.api_key_value(), None);
    }

    #[test]
    fn test_oauth_needs_refresh() {
        // Token expiring in 2 minutes - should need refresh
        let expires = chrono::Utc::now().timestamp_millis() + 2 * 60 * 1000;
        let auth = ProviderAuth::oauth("access", "refresh", expires);
        assert!(auth.needs_refresh());

        // Token expiring in 10 minutes - should not need refresh
        let expires = chrono::Utc::now().timestamp_millis() + 10 * 60 * 1000;
        let auth = ProviderAuth::oauth("access", "refresh", expires);
        assert!(!auth.needs_refresh());
    }

    #[test]
    fn test_oauth_is_expired() {
        // Expired token
        let expires = chrono::Utc::now().timestamp_millis() - 1000;
        let auth = ProviderAuth::oauth("access", "refresh", expires);
        assert!(auth.is_expired());

        // Valid token
        let expires = chrono::Utc::now().timestamp_millis() + 3600000;
        let auth = ProviderAuth::oauth("access", "refresh", expires);
        assert!(!auth.is_expired());
    }

    #[test]
    fn test_api_key_never_needs_refresh() {
        let auth = ProviderAuth::api_key("sk-test");
        assert!(!auth.needs_refresh());
        assert!(!auth.is_expired());
    }

    #[test]
    fn test_serde_api_key() {
        let auth = ProviderAuth::api_key("sk-test-key");
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"type\":\"api\""));
        assert!(json.contains("\"key\":\"sk-test-key\""));

        let parsed: ProviderAuth = serde_json::from_str(&json).unwrap();
        assert_eq!(auth, parsed);
    }

    #[test]
    fn test_serde_oauth() {
        let auth = ProviderAuth::oauth("access-token", "refresh-token", 1735600000000);
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"type\":\"oauth\""), "JSON was: {}", json);
        assert!(json.contains("\"access\":\"access-token\""));
        assert!(json.contains("\"refresh\":\"refresh-token\""));
        assert!(json.contains("\"expires\":1735600000000"));

        let parsed: ProviderAuth = serde_json::from_str(&json).unwrap();
        assert_eq!(auth, parsed);
    }

    #[test]
    fn test_auth_type_display() {
        let api = ProviderAuth::api_key("key");
        assert_eq!(api.auth_type_display(), "api_key");

        let oauth = ProviderAuth::oauth("access", "refresh", 0);
        assert_eq!(oauth.auth_type_display(), "oauth");
    }
}
