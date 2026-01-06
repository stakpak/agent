//! Anthropic provider implementation

use super::convert::{from_anthropic_response_with_warnings, to_anthropic_request};
use super::stream::create_stream;
use super::types::{AnthropicConfig, AnthropicResponse};
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
        if config.api_key.is_empty() {
            return Err(Error::MissingApiKey("anthropic".to_string()));
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
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn provider_id(&self) -> &str {
        "anthropic"
    }

    fn build_headers(&self, custom_headers: Option<&Headers>) -> Headers {
        self.build_headers_with_cache(custom_headers, false)
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let url = format!("{}messages", self.config.base_url);
        let conversion_result = to_anthropic_request(&request, false)?;

        let headers = self.build_headers_with_cache(
            request.options.headers.as_ref(),
            conversion_result.has_cache_control,
        );

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&conversion_result.request)
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
        from_anthropic_response_with_warnings(anthropic_resp, conversion_result.warnings)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = format!("{}messages", self.config.base_url);
        let conversion_result = to_anthropic_request(&request, true)?;

        let headers = self.build_headers_with_cache(
            request.options.headers.as_ref(),
            conversion_result.has_cache_control,
        );

        let req_builder = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&conversion_result.request);

        let event_source = EventSource::new(req_builder)
            .map_err(|e| Error::stream_error(format!("Failed to create event source: {}", e)))?;

        // Note: Streaming doesn't return warnings in the current implementation
        // Warnings would need to be communicated via the stream events
        create_stream(event_source).await
    }
}

impl AnthropicProvider {
    /// Build headers with optional cache control beta feature
    fn build_headers_with_cache(
        &self,
        custom_headers: Option<&Headers>,
        has_cache_control: bool,
    ) -> Headers {
        let mut headers = Headers::new();

        // Anthropic uses x-api-key header
        headers.insert("x-api-key", &self.config.api_key);
        headers.insert("anthropic-version", &self.config.anthropic_version);
        headers.insert("Content-Type", "application/json");

        // Collect beta features
        let mut betas = self.config.beta_features.clone();

        // Auto-add prompt caching beta if cache_control is used and not already present
        const PROMPT_CACHING_BETA: &str = "prompt-caching-2024-07-31";
        if has_cache_control && !betas.iter().any(|b| b == PROMPT_CACHING_BETA) {
            betas.push(PROMPT_CACHING_BETA.to_string());
        }

        // Add beta features if any
        if !betas.is_empty() {
            headers.insert("anthropic-beta", betas.join(","));
        }

        // Merge custom headers (they can override defaults)
        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        headers
    }

    /// List available Anthropic models
    pub async fn list_models(&self) -> Result<Vec<String>> {
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
