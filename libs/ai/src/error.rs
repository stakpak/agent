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

    /// Try to parse raw stream data as a provider error response.
    ///
    /// When an upstream provider (Claude, OpenAI, Gemini, etc.) is down or
    /// overloaded, it may send an error JSON like
    /// `{"error": {"type": "...", "message": "..."}}` instead of a normal
    /// streaming chunk. This helper detects that pattern and returns a
    /// [`ProviderError`](Error::ProviderError). If the data isn't a
    /// recognized error format, it falls back to `fallback_label` with a
    /// 200-char preview of the raw data.
    pub fn from_unparseable_chunk(data: &str, fallback_label: &str) -> Self {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
            && let Some(error) = json.get("error")
        {
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            let error_type = error
                .get("type")
                .or_else(|| error.get("status"))
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            return Self::ProviderError(format!("{}: {}", error_type, message));
        }

        let preview: String = data.chars().take(200).collect();
        Self::StreamError(format!("{}: {}", fallback_label, preview))
    }
}
