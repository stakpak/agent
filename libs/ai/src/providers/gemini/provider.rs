//! Gemini provider implementation

use super::convert::{from_gemini_response, to_gemini_request};
use super::stream::create_stream;
use super::types::{GeminiConfig, GeminiResponse};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::tls::create_platform_tls_client;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers, Model};
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

        let client = create_platform_tls_client()?;
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
        if stream {
            format!(
                "{}models/{}:{}?alt=sse&key={}",
                self.config.base_url, model, action, self.config.api_key
            )
        } else {
            format!(
                "{}models/{}:{}?key={}",
                self.config.base_url, model, action, self.config.api_key
            )
        }
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
        let url = self.get_url(&request.model.id, false);
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

        let response_text = response.text().await?;

        let gemini_resp: GeminiResponse = serde_json::from_str(&response_text).map_err(|e| {
            Error::provider_error(format!("Failed to parse Gemini response: {}", e))
        })?;
        from_gemini_response(gemini_resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = self.get_url(&request.model.id, true);
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

    async fn list_models(&self) -> Result<Vec<Model>> {
        // Load from models.dev cache (uses "google" as provider ID)
        crate::registry::models_dev::load_models_for_provider("google")
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = crate::registry::models_dev::load_models_for_provider("google")?;
        Ok(models.into_iter().find(|m| m.id == id))
    }
}
