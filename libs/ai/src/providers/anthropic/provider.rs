//! Anthropic provider implementation

use super::convert::{from_anthropic_response, to_anthropic_request};
use super::stream::create_stream;
use super::types::{AnthropicAuth, AnthropicConfig, AnthropicResponse};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;

/// Anthropic provider
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: Client,
}

impl AnthropicProvider {
    /// Environment variable for API key
    pub const API_KEY_ENV: &'static str = "ANTHROPIC_API_KEY";

    /// Default API version
    pub const DEFAULT_VERSION: &'static str = "2023-06-01";

    /// Create a new Anthropic provider
    pub fn new(config: AnthropicConfig) -> Result<Self> {
        // Validate that we have some form of authentication
        match &config.auth {
            AnthropicAuth::ApiKey(key) if key.is_empty() => {
                return Err(Error::MissingApiKey("anthropic".to_string()));
            }
            AnthropicAuth::OAuth { access_token } if access_token.is_empty() => {
                return Err(Error::MissingApiKey("anthropic (OAuth token)".to_string()));
            }
            _ => {}
        }

        let client = Client::new();
        Ok(Self { config, client })
    }

    /// Create provider from environment
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var(Self::API_KEY_ENV)
            .map_err(|_| Error::MissingApiKey("anthropic".to_string()))?;

        Self::new(AnthropicConfig::new(api_key))
    }

    /// Create provider with OAuth access token
    pub fn with_oauth(access_token: impl Into<String>) -> Result<Self> {
        Self::new(AnthropicConfig::with_oauth(access_token))
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn provider_id(&self) -> &str {
        "anthropic"
    }

    fn build_headers(&self, custom_headers: Option<&Headers>) -> Headers {
        let mut headers = Headers::new();

        // Apply authentication based on auth type
        match &self.config.auth {
            AnthropicAuth::ApiKey(api_key) => {
                headers.insert("x-api-key", api_key);
            }
            AnthropicAuth::OAuth { access_token } => {
                headers.insert("authorization", format!("Bearer {}", access_token));
            }
        }

        headers.insert("anthropic-version", &self.config.anthropic_version);
        headers.insert("Content-Type", "application/json");

        // Add beta features if any
        if !self.config.beta_features.is_empty() {
            headers.insert("anthropic-beta", self.config.beta_features.join(","));
        }

        // Merge custom headers (they can override defaults)
        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        headers
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let url = format!("{}messages", self.config.base_url);
        let is_oauth = self.config.auth.is_oauth();
        let anthropic_req = to_anthropic_request(&request, false, is_oauth)?;

        let headers = self.build_headers(request.options.headers.as_ref());

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&anthropic_req)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "Anthropic API error {}: {}",
                status, error_text
            )));
        }

        let anthropic_resp: AnthropicResponse = response.json().await?;
        from_anthropic_response(anthropic_resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = format!("{}messages", self.config.base_url);
        let is_oauth = self.config.auth.is_oauth();
        let anthropic_req = to_anthropic_request(&request, true, is_oauth)?;

        let headers = self.build_headers(request.options.headers.as_ref());

        let req_builder = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&anthropic_req);

        let event_source = EventSource::new(req_builder)
            .map_err(|e| Error::stream_error(format!("Failed to create event source: {}", e)))?;

        create_stream(event_source).await
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        // Anthropic doesn't have a models endpoint, return known models
        Ok(vec![
            "claude-3-5-sonnet-20241022".to_string(),
            "claude-3-5-haiku-20241022".to_string(),
            "claude-3-opus-20240229".to_string(),
            "claude-3-sonnet-20240229".to_string(),
            "claude-3-haiku-20240307".to_string(),
        ])
    }
}
