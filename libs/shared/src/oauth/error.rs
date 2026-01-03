//! OAuth error types

use thiserror::Error;

/// Errors that can occur during OAuth operations
#[derive(Error, Debug)]
pub enum OAuthError {
    /// PKCE challenge was not initialized before token exchange
    #[error("PKCE challenge not initialized. Call generate_auth_url() first.")]
    PkceNotInitialized,

    /// Token exchange failed
    #[error("Token exchange failed: {0}")]
    TokenExchangeFailed(String),

    /// Token refresh failed
    #[error("Token refresh failed: {0}")]
    TokenRefreshFailed(String),

    /// API key creation failed
    #[error("Failed to create API key from OAuth tokens")]
    ApiKeyCreationFailed,

    /// Unknown authentication method
    #[error("Unknown authentication method: {0}")]
    UnknownMethod(String),

    /// Invalid header value
    #[error("Invalid header value")]
    InvalidHeader,

    /// HTTP request error
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// JSON parsing error
    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Invalid authorization code format
    #[error("Invalid authorization code format: {0}")]
    InvalidCodeFormat(String),

    /// Provider not found
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    /// OAuth not supported for this method
    #[error("OAuth not supported for this authentication method")]
    OAuthNotSupported,

    /// File I/O error
    #[error("File I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// TOML serialization error
    #[error("TOML serialization error: {0}")]
    TomlSerError(#[from] toml::ser::Error),

    /// TOML deserialization error
    #[error("TOML deserialization error: {0}")]
    TomlDeError(#[from] toml::de::Error),

    /// Authentication required
    #[error("Authentication required. Run 'stakpak auth login' to authenticate.")]
    AuthRequired,

    /// Token expired and refresh failed
    #[error("Token expired. Please re-authenticate with 'stakpak auth login'.")]
    TokenExpired,
}

impl OAuthError {
    /// Create a token exchange failed error
    pub fn token_exchange_failed(msg: impl Into<String>) -> Self {
        Self::TokenExchangeFailed(msg.into())
    }

    /// Create a token refresh failed error
    pub fn token_refresh_failed(msg: impl Into<String>) -> Self {
        Self::TokenRefreshFailed(msg.into())
    }

    /// Create a parse error
    pub fn parse_error(msg: impl Into<String>) -> Self {
        Self::ParseError(msg.into())
    }

    /// Create an invalid code format error
    pub fn invalid_code_format(msg: impl Into<String>) -> Self {
        Self::InvalidCodeFormat(msg.into())
    }

    /// Create a provider not found error
    pub fn provider_not_found(provider: impl Into<String>) -> Self {
        Self::ProviderNotFound(provider.into())
    }

    /// Create an unknown method error
    pub fn unknown_method(method: impl Into<String>) -> Self {
        Self::UnknownMethod(method.into())
    }
}

/// Result type alias for OAuth operations
pub type OAuthResult<T> = Result<T, OAuthError>;
