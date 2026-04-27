//! MiniMax-specific types

/// Configuration for MiniMax provider
#[derive(Debug, Clone)]
pub struct MiniMaxConfig {
    /// API key
    pub api_key: String,
    /// Base URL (default: https://api.minimax.io/v1)
    pub base_url: String,
}

impl MiniMaxConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.minimax.io/v1".to_string(),
        }
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for MiniMaxConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("MINIMAX_API_KEY").unwrap_or_else(|_| String::new()),
            base_url: "https://api.minimax.io/v1".to_string(),
        }
    }
}
