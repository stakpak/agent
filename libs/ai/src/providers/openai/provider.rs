//! OpenAI provider implementation

use super::convert::{
    from_openai_response, from_responses_response, to_openai_request, to_responses_request,
};
use super::stream::{create_completions_stream, create_responses_stream};
use super::types::{ChatCompletionResponse, OpenAIConfig, ResponsesResponse};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::tls::create_platform_tls_client;
use crate::types::{
    GenerateRequest, GenerateResponse, GenerateStream, Headers, Model, OpenAIApiConfig,
    ProviderOptions,
};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;

/// OpenAI provider
pub struct OpenAIProvider {
    config: OpenAIConfig,
    client: Client,
}

impl OpenAIProvider {
    const OFFICIAL_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

    /// Create a new OpenAI provider
    ///
    /// Note: API key validation is skipped when a custom base URL is configured,
    /// as OpenAI-compatible providers like Ollama may not require authentication.
    pub fn new(mut config: OpenAIConfig) -> Result<Self> {
        let is_default_url = config.base_url == Self::OFFICIAL_OPENAI_BASE_URL;
        if config.api_key.is_empty() && is_default_url {
            return Err(Error::MissingApiKey("openai".to_string()));
        }

        // Normalize base_url: strip trailing slash to avoid double-slash in URL paths
        config.base_url = config.base_url.trim_end_matches('/').to_string();

        let client = create_platform_tls_client()?;
        Ok(Self { config, client })
    }

    fn should_use_responses_api(&self, request: &GenerateRequest) -> bool {
        match request.provider_options.as_ref() {
            Some(ProviderOptions::OpenAI(opts)) => match &opts.api_config {
                Some(OpenAIApiConfig::Responses(_)) => true,
                Some(OpenAIApiConfig::Completions(_)) => false,
                None => self.config.base_url == Self::OFFICIAL_OPENAI_BASE_URL,
            },
            Some(_) => false,
            None => self.config.base_url == Self::OFFICIAL_OPENAI_BASE_URL,
        }
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
        let headers = self.build_headers(request.options.headers.as_ref());

        if self.should_use_responses_api(&request) {
            let url = format!("{}/responses", self.config.base_url);
            let responses_req = to_responses_request(&request, false);

            let response = self
                .client
                .post(&url)
                .headers(headers.to_reqwest_headers())
                .json(&responses_req)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();
                return Err(Error::provider_error(format!(
                    "OpenAI Responses API error {}: {}",
                    status, error_text
                )));
            }

            let responses_resp: ResponsesResponse = response.json().await?;
            from_responses_response(responses_resp)
        } else {
            let url = format!("{}/chat/completions", self.config.base_url);
            let openai_req = to_openai_request(&request, false);

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
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let headers = self.build_headers(request.options.headers.as_ref());

        if self.should_use_responses_api(&request) {
            let url = format!("{}/responses", self.config.base_url);
            let responses_req = to_responses_request(&request, true);

            let req_builder = self
                .client
                .post(&url)
                .headers(headers.to_reqwest_headers())
                .json(&responses_req);

            let event_source = EventSource::new(req_builder).map_err(|e| {
                Error::stream_error(format!("Failed to create event source: {}", e))
            })?;

            create_responses_stream(event_source).await
        } else {
            let url = format!("{}/chat/completions", self.config.base_url);
            let openai_req = to_openai_request(&request, true);

            let req_builder = self
                .client
                .post(&url)
                .headers(headers.to_reqwest_headers())
                .json(&openai_req);

            let event_source = EventSource::new(req_builder).map_err(|e| {
                Error::stream_error(format!("Failed to create event source: {}", e))
            })?;

            create_completions_stream(event_source).await
        }
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        // Load from models.dev cache
        crate::registry::models_dev::load_models_for_provider("openai")
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = crate::registry::models_dev::load_models_for_provider("openai")?;
        Ok(models.into_iter().find(|m| m.id == id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GenerateRequest, Message, OpenAIOptions, ProviderOptions, Role};

    fn make_request(provider_options: Option<ProviderOptions>) -> GenerateRequest {
        let mut req = GenerateRequest::new(
            Model::custom("gpt-4.1-mini", "openai"),
            vec![Message::new(Role::User, "Hello")],
        );
        req.provider_options = provider_options;
        req
    }

    #[test]
    fn test_defaults_to_responses_for_official_openai_url() {
        let provider = OpenAIProvider::new(OpenAIConfig::new("test-key")).unwrap();
        let req = make_request(None);
        assert!(provider.should_use_responses_api(&req));
    }

    #[test]
    fn test_defaults_to_completions_for_custom_openai_compatible_url() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key").with_base_url("http://localhost:11434/v1"),
        )
        .unwrap();
        let req = make_request(None);
        assert!(!provider.should_use_responses_api(&req));
    }

    #[test]
    fn test_explicit_completions_overrides_official_default() {
        let provider = OpenAIProvider::new(OpenAIConfig::new("test-key")).unwrap();
        let req = make_request(Some(ProviderOptions::OpenAI(OpenAIOptions::completions())));
        assert!(!provider.should_use_responses_api(&req));
    }

    #[test]
    fn test_explicit_responses_overrides_custom_endpoint_default() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key").with_base_url("http://localhost:11434/v1"),
        )
        .unwrap();
        let req = make_request(Some(ProviderOptions::OpenAI(OpenAIOptions::responses())));
        assert!(provider.should_use_responses_api(&req));
    }
}
