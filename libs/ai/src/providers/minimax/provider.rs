//! MiniMax provider implementation

use super::convert::{to_minimax_request, from_minimax_response, parse_minimax_error};
use super::stream::create_stream;
use super::types::MiniMaxConfig;
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::openai::types::{ChatCompletionResponse};
use crate::providers::tls::create_platform_tls_client;
use crate::types::{
    GenerateRequest, GenerateResponse, GenerateStream, Headers, Model
};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;

/// MiniMax provider
///
/// Routes inference requests through MiniMax's OpenAI-compatible API.
pub struct MiniMaxProvider {
    config: MiniMaxConfig,
    client: Client,
}

impl MiniMaxProvider {
    /// Create a new MiniMax provider
    pub fn new(config: MiniMaxConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(Error::MissingApiKey("minimax".to_string()));
        }

        let client = create_platform_tls_client()?;
        Ok(Self { config, client })
    }

    /// Create provider from environment
    pub fn from_env() -> Result<Self> {
        Self::new(MiniMaxConfig::default())
    }
}

#[async_trait]
impl Provider for MiniMaxProvider {
    fn provider_id(&self) -> &str {
        "minimax"
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
        let url = format!("{}/chat/completions", self.config.base_url);

        let minimax_req = to_minimax_request(&request, false);

        let headers = self.build_headers(request.options.headers.as_ref());

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&minimax_req)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            let friendly_error = parse_minimax_error(&error_text, status.as_u16());
            return Err(Error::provider_error(friendly_error));
        }

        let resp: ChatCompletionResponse = response.json().await?;
        from_minimax_response(resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = format!("{}/chat/completions", self.config.base_url);

        let minimax_req = to_minimax_request(&request, true);

        let headers = self.build_headers(request.options.headers.as_ref());

        let req_builder = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&minimax_req);

        let event_source = EventSource::new(req_builder)
            .map_err(|e| Error::stream_error(format!("Failed to create event source: {}", e)))?;

        create_stream(event_source).await
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        crate::registry::models_dev::load_models_for_provider("minimax")
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = crate::registry::models_dev::load_models_for_provider("minimax")?;
        Ok(models.into_iter().find(|m| m.id == id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id() {
        let config = MiniMaxConfig::new("test-key");
        let provider = MiniMaxProvider::new(config).unwrap();
        assert_eq!(provider.provider_id(), "minimax");
    }

    #[test]
    fn test_missing_api_key() {
        let config = MiniMaxConfig::new("");
        let result = MiniMaxProvider::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_headers() {
        let config = MiniMaxConfig::new("test-key");
        let provider = MiniMaxProvider::new(config).unwrap();
        let headers = provider.build_headers(None);
        let reqwest_headers = headers.to_reqwest_headers();
        assert_eq!(
            reqwest_headers.get("authorization").unwrap(),
            "Bearer test-key"
        );
        assert_eq!(
            reqwest_headers.get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_parse_minimax_error_auth() {
        let error = r#"{"error":{"type":"authentication_error","message":"Invalid API key"}}"#;
        let result = parse_minimax_error(error, 401);
        assert!(result.contains("Authentication failed"));
        assert!(result.contains("Invalid API key"));
    }

    #[test]
    fn test_parse_minimax_error_rate_limit() {
        let error = r#"{"error":{"type":"rate_limit_error","message":"Too many requests"}}"#;
        let result = parse_minimax_error(error, 429);
        assert!(result.contains("Rate limited"));
    }

    #[test]
    fn test_parse_minimax_error_fallback() {
        let result = parse_minimax_error("not json", 500);
        assert!(result.contains("MiniMax API error 500"));
    }

    #[test]
    fn test_default_config_base_url() {
        let config = MiniMaxConfig::new("key");
        assert_eq!(config.base_url, "https://api.minimax.io/v1");
    }

    #[test]
    fn test_config_with_base_url() {
        let config = MiniMaxConfig::new("key").with_base_url("https://custom.example.com/v1");
        assert_eq!(config.base_url, "https://custom.example.com/v1");
    }
}
