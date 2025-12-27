//! OpenAI provider implementation

use super::convert::{from_openai_response, to_openai_request};
use super::stream::create_stream;
use super::types::{ChatCompletionResponse, OpenAIConfig};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;

/// OpenAI provider
pub struct OpenAIProvider {
    config: OpenAIConfig,
    client: Client,
}

impl OpenAIProvider {
    /// Create a new OpenAI provider
    pub fn new(config: OpenAIConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(Error::MissingApiKey("openai".to_string()));
        }

        let client = Client::new();
        Ok(Self { config, client })
    }

    /// Create provider from environment
    pub fn from_env() -> Result<Self> {
        Self::new(OpenAIConfig::default())
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    fn provider_id(&self) -> &str {
        "openai"
    }

    fn build_headers(&self, custom_headers: Option<&Headers>) -> Headers {
        let mut headers = Headers::new();

        headers.insert("Authorization", format!("Bearer {}", self.config.api_key));
        headers.insert("Content-Type", "application/json");

        if let Some(org) = &self.config.organization {
            headers.insert("OpenAI-Organization", org);
        }

        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        headers
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let url = format!("{}/chat/completions", self.config.base_url);
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
                "OpenAI API error {}: {}",
                status, error_text
            )));
        }

        let openai_resp: ChatCompletionResponse = response.json().await?;
        from_openai_response(openai_resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let openai_req = to_openai_request(&request, true);

        let headers = self.build_headers(request.options.headers.as_ref());

        let req_builder = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&openai_req);

        let event_source = EventSource::new(req_builder)
            .map_err(|e| Error::stream_error(format!("Failed to create event source: {}", e)))?;

        create_stream(event_source).await
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        // Simplified - in production, call /v1/models endpoint
        Ok(vec![
            "gpt-4".to_string(),
            "gpt-4-turbo-preview".to_string(),
            "gpt-3.5-turbo".to_string(),
        ])
    }
}
