//! Anthropic provider implementation

use super::convert::{from_anthropic_response_with_warnings, to_anthropic_request};
use super::stream::create_stream;
use super::types::{AnthropicConfig, AnthropicResponse};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::tls::create_platform_tls_client;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers, Model};
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
        if config.auth.is_empty() {
            return Err(Error::MissingApiKey("anthropic".to_string()));
        }

        let client = create_platform_tls_client()?;
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
        self.build_headers_with_cache(custom_headers, false)
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        // Load from models.dev cache
        crate::registry::models_dev::load_models_for_provider("anthropic")
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = crate::registry::models_dev::load_models_for_provider("anthropic")?;
        Ok(models.into_iter().find(|m| m.id == id))
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let url = format!("{}messages", self.config.base_url);
        let conversion_result = to_anthropic_request(&request, &self.config, false)?;

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
        let conversion_result = to_anthropic_request(&request, &self.config, true)?;

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

        // Apply authentication (works for both API key and OAuth)
        let (auth_header, auth_value) = self.config.auth.to_header();
        headers.insert(auth_header, auth_value);

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
}
