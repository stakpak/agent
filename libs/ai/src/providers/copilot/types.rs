//! GitHub Copilot provider types

/// Configuration for the GitHub Copilot provider.
///
/// The `github_token` (OAuth token from device flow) is exchanged for a
/// short-lived Copilot API token before every request (cached in the provider).
#[derive(Debug, Clone)]
pub struct CopilotConfig {
    /// GitHub OAuth token obtained via the device flow (`ghu_...`).
    /// Used to fetch a short-lived Copilot API token via
    /// `GET https://api.github.com/copilot_internal/v2/token`.
    pub github_token: String,
    /// Optional override for the Copilot chat API base URL.
    /// When `None`, the URL returned by the token exchange endpoint is used.
    pub base_url_override: Option<String>,
}

impl CopilotConfig {
    /// Fallback base URL if the token exchange response has no `endpoints.api`.
    pub const FALLBACK_BASE_URL: &'static str = "https://api.githubcopilot.com";

    /// Create a new Copilot config with a GitHub OAuth token.
    pub fn new(github_token: impl Into<String>) -> Self {
        Self {
            github_token: github_token.into(),
            base_url_override: None,
        }
    }

    /// Override the chat API base URL 
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url_override = Some(base_url.into().trim_end_matches('/').to_string());
        self
    }
}

/// A cached short-lived Copilot API token.
#[derive(Debug, Clone)]
pub struct CachedCopilotToken {
    /// The short-lived token sent as `Bearer` to the Copilot chat API.
    pub token: String,
    /// Unix timestamp (seconds) when this token expires.
    pub expires_at: u64,
    /// Resolved chat API base URL (`endpoints.api` from the token response).
    pub api_base: String,
}
