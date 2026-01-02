//! Static dispatcher for zero-cost provider abstraction

use crate::error::{Error, Result};
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream};

/// Provider kind for static dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI provider
    OpenAI,
    /// Anthropic provider (future)
    Anthropic,
    /// Google Gemini provider (future)
    Google,
}

impl ProviderKind {
    /// Get provider ID string
    pub fn as_str(&self) -> &str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Self::OpenAI),
            "anthropic" => Ok(Self::Anthropic),
            "google" | "gemini" => Ok(Self::Google),
            _ => Err(Error::UnknownProvider(s.to_string())),
        }
    }
}

/// Static dispatcher for compile-time provider routing
pub struct ProviderDispatcher;

impl ProviderDispatcher {
    /// Generate using static dispatch
    pub async fn generate(
        _kind: ProviderKind,
        _request: GenerateRequest,
    ) -> Result<GenerateResponse> {
        // Will be implemented when providers are added
        Err(Error::Other("Not implemented yet".to_string()))
    }

    /// Stream using static dispatch
    pub async fn stream(_kind: ProviderKind, _request: GenerateRequest) -> Result<GenerateStream> {
        // Will be implemented when providers are added
        Err(Error::Other("Not implemented yet".to_string()))
    }
}
