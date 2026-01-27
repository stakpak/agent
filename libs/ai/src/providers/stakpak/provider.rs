//! Stakpak provider implementation
//!
//! Stakpak provides an OpenAI-compatible API, so we reuse the OpenAI
//! conversion and streaming logic.

use super::types::StakpakProviderConfig;
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::openai::convert::{from_openai_response, to_openai_request};
use crate::providers::openai::stream::create_stream;
use crate::providers::openai::types::ChatCompletionResponse;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;

/// Stakpak provider
///
/// Routes inference requests through Stakpak's OpenAI-compatible API.
pub struct StakpakProvider {
    config: StakpakProviderConfig,
    client: Client,
}

impl StakpakProvider {
    /// Create a new Stakpak provider
    pub fn new(config: StakpakProviderConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(Error::MissingApiKey("stakpak".to_string()));
        }

        let client = Client::new();
        Ok(Self { config, client })
    }

    /// Create provider from environment
    pub fn from_env() -> Result<Self> {
        Self::new(StakpakProviderConfig::default())
    }
}

#[async_trait]
impl Provider for StakpakProvider {
    fn provider_id(&self) -> &str {
        "stakpak"
    }

    fn build_headers(&self, custom_headers: Option<&Headers>) -> Headers {
        let mut headers = Headers::new();

        headers.insert("Authorization", format!("Bearer {}", self.config.api_key));
        headers.insert("Content-Type", "application/json");

        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        headers
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let url = format!("{}/v1/chat/completions", self.config.base_url);

        // Stakpak uses OpenAI-compatible API, reuse OpenAI conversion
        let openai_req = to_openai_request(&request, false);

        let headers = self.build_headers(request.options.headers.as_ref());

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&openai_req)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "Stakpak API error {}: {}",
                status, error_text
            )));
        }

        let openai_resp: ChatCompletionResponse = response.json().await?;
        from_openai_response(openai_resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = format!("{}/v1/chat/completions", self.config.base_url);

        // Stakpak uses OpenAI-compatible API, reuse OpenAI conversion
        let openai_req = to_openai_request(&request, true);

        let headers = self.build_headers(request.options.headers.as_ref());

        let req_builder = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&openai_req);

        let event_source = EventSource::new(req_builder)
            .map_err(|e| Error::stream_error(format!("Failed to create event source: {}", e)))?;

        // Reuse OpenAI stream parsing since Stakpak uses OpenAI-compatible streaming
        create_stream(event_source).await
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        // Stakpak supports routing to various providers
        Ok(vec![
            "anthropic/claude-sonnet-4-5-20250929".to_string(),
            "anthropic/claude-haiku-4-5-20250929".to_string(),
            "anthropic/claude-opus-4-5-20250929".to_string(),
            "openai/gpt-5".to_string(),
            "openai/gpt-5-mini".to_string(),
            "google/gemini-2.5-flash".to_string(),
            "google/gemini-2.5-pro".to_string(),
        ])
    }
}
