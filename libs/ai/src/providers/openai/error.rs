//! OpenAI-specific errors

use thiserror::Error;

/// OpenAI provider errors
#[derive(Error, Debug)]
pub enum OpenAIError {
    /// HTTP error
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON parsing error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// API error response
    #[error("OpenAI API error: {0}")]
    ApiError(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}
