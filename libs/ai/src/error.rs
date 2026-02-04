//! Error types for the AI SDK

use thiserror::Error;

/// Result type alias using the SDK's Error type
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for the AI SDK
#[derive(Error, Debug)]
pub enum Error {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Failed to parse JSON response
    #[error("JSON parsing failed: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Invalid response from provider
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Provider not found in registry
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    /// Unknown provider for model
    #[error("Unknown provider for model: {0}")]
    UnknownProvider(String),

    /// Invalid model format
    #[error("Invalid model format: {0}")]
    InvalidModel(String),

    /// API key not found
    #[error("API key not found for provider: {0}")]
    MissingApiKey(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// Provider-specific error
    #[error("Provider error: {0}")]
    ProviderError(String),

    /// Streaming error
    #[error("Streaming error: {0}")]
    StreamError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Network error (fetch failed, timeout, etc.)
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Create a new provider error
    pub fn provider_error(msg: impl Into<String>) -> Self {
        Self::ProviderError(msg.into())
    }

    /// Create a new invalid response error
    pub fn invalid_response(msg: impl Into<String>) -> Self {
        Self::InvalidResponse(msg.into())
    }

    /// Create a new stream error
    pub fn stream_error(msg: impl Into<String>) -> Self {
        Self::StreamError(msg.into())
    }
}
