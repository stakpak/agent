//! Stakpak API Client
//!
//! Provides access to Stakpak's non-inference APIs:
//! - Sessions and checkpoints (via new `/v1/sessions` endpoints)
//! - MCP tool calls (memory, docs search, Slack)
//! - Billing and account
//! - Rulebooks

mod client;
mod models;
pub mod storage;

pub use client::StakpakApiClient;
pub use models::*;

/// Configuration for StakpakApiClient
#[derive(Clone, Debug)]
pub struct StakpakApiConfig {
    /// API key for authentication
    pub api_key: String,
    /// API endpoint URL (default: https://apiv2.stakpak.dev)
    pub api_endpoint: String,
    /// Base URL for downloading rulebooks/skills/playbooks
    /// If not set, api_endpoint is used for content downloads
    pub rulebook_base_url: Option<String>,
}

impl StakpakApiConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_endpoint: "https://apiv2.stakpak.dev".to_string(),
            rulebook_base_url: None,
        }
    }

    /// Set API endpoint
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.api_endpoint = endpoint.into();
        self
    }

    /// Set rulebook base URL
    pub fn with_rulebook_base_url(mut self, url: impl Into<String>) -> Self {
        self.rulebook_base_url = Some(url.into());
        self
    }

    /// Set rulebook base URL from environment variable
    pub fn with_rulebook_base_url_from_env(mut self) -> Self {
        if let Ok(url) = std::env::var("STAKPAK_RULEBOOK_BASE_URL") {
            self.rulebook_base_url = Some(url);
        }
        self
    }
}

impl Default for StakpakApiConfig {
    fn default() -> Self {
        Self::new(std::env::var("STAKPAK_API_KEY").unwrap_or_default())
    }
}
