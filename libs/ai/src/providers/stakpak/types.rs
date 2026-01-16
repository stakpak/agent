//! Stakpak-specific types
//!
//! Stakpak uses an OpenAI-compatible API, so we reuse OpenAI types for
//! request/response serialization.

/// Configuration for Stakpak provider
#[derive(Debug, Clone)]
pub struct StakpakConfig {
    /// API key
    pub api_key: String,
    /// Base URL (default: https://apiv2.stakpak.dev)
    pub base_url: String,
}

impl StakpakConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://apiv2.stakpak.dev".to_string(),
        }
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for StakpakConfig {
    fn default() -> Self {
        Self::new(std::env::var("STAKPAK_API_KEY").unwrap_or_else(|_| String::new()))
    }
}
