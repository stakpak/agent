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
}

impl StakpakApiConfig {
    /// Create new config with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_endpoint: "https://apiv2.stakpak.dev".to_string(),
        }
    }

    /// Set API endpoint
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.api_endpoint = endpoint.into();
        self
    }
}

impl Default for StakpakApiConfig {
    fn default() -> Self {
        Self::new(std::env::var("STAKPAK_API_KEY").unwrap_or_default())
    }
}
