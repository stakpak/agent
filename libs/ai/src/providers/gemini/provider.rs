//! Gemini provider implementation

use super::convert::{from_gemini_response, to_gemini_request};
use super::stream::create_stream;
use super::types::{GeminiConfig, GeminiResponse};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers};
use async_trait::async_trait;
use reqwest::Client;

/// Gemini provider
pub struct GeminiProvider {
    config: GeminiConfig,
    client: Client,
}

impl GeminiProvider {
    /// Environment variable for API key
    pub const API_KEY_ENV: &'static str = "GEMINI_API_KEY";

    /// Create a new Gemini provider
    pub fn new(config: GeminiConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(Error::MissingApiKey("gemini".to_string()));
        }

        let client = Client::new();
        Ok(Self { config, client })
    }

    /// Create provider from environment
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var(Self::API_KEY_ENV)
            .map_err(|_| Error::MissingApiKey("gemini".to_string()))?;

        Self::new(GeminiConfig::new(api_key))
    }

    /// Build URL for Gemini API
    fn get_url(&self, model: &str, stream: bool) -> String {
        let action = if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        format!(
            "{}models/{}:{}?key={}",
            self.config.base_url, model, action, self.config.api_key
        )
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn provider_id(&self) -> &str {
        "google"
    }

    fn build_headers(&self, custom_headers: Option<&Headers>) -> Headers {
        let mut headers = Headers::new();

        // Gemini supports x-goog-api-key header as alternative to URL param
        // But we're using URL param for simplicity
        headers.insert("Content-Type", "application/json");

        // Merge custom headers
        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        headers
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let url = self.get_url(&request.model, false);
        let gemini_req = to_gemini_request(&request)?;

        let headers = self.build_headers(request.options.headers.as_ref());

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&gemini_req)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "Gemini API error {}: {}",
                status, error_text
            )));
        }

        let gemini_resp: GeminiResponse = response.json().await?;
        from_gemini_response(gemini_resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = self.get_url(&request.model, true);
        let gemini_req = to_gemini_request(&request)?;

        let headers = self.build_headers(request.options.headers.as_ref());

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&gemini_req)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "Gemini API error {}: {}",
                status, error_text
            )));
        }

        create_stream(response).await
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        // Gemini has a models endpoint, but for simplicity return known models
        Ok(vec![
            "gemini-2.0-flash-exp".to_string(),
            "gemini-1.5-pro".to_string(),
            "gemini-1.5-flash".to_string(),
            "gemini-1.0-pro".to_string(),
        ])
    }
}
