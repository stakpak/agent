use super::types::OpenRouterConfig;
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::openai::convert::{from_openai_response, to_openai_request};
use crate::providers::openai::stream::create_completions_stream;
use crate::providers::openai::types::ChatCompletionResponse;
use crate::providers::tls::create_platform_tls_client;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers, Model};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;

pub struct OpenRouterProvider {
    config: OpenRouterConfig,
    client: Client,
}

impl OpenRouterProvider {
    pub fn new(config: OpenRouterConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(Error::MissingApiKey("openrouter".to_string()));
        }
        let client = create_platform_tls_client()?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn provider_id(&self) -> &str {
        "openrouter"
    }

    fn build_headers(&self, custom_headers: Option<&Headers>) -> Headers {
        let mut headers = Headers::new();

        headers.insert("Authorization", format!("Bearer {}", self.config.api_key));
        headers.insert("Content-Type", "application/json");

        if let Some(referer) = &self.config.http_referer {
            headers.insert("HTTP-Referer", referer.clone());
        }

        if let Some(title) = &self.config.site_title {
            headers.insert("X-OpenRouter-Title", title.clone());
        }

        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        headers
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers(request.options.headers.as_ref());

        let openai_req = to_openai_request(&request, false);

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&openai_req)
            .send()
            .await
            .map_err(|e| Error::provider_error(format!("OpenRouter request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "OpenRouter returned error {}: {}",
                status, text
            )));
        }

        let openai_resp: ChatCompletionResponse = response.json().await?;
        from_openai_response(openai_resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers(request.options.headers.as_ref());

        let openai_req = to_openai_request(&request, true);

        let request_builder = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&openai_req);

        let event_source = EventSource::new(request_builder)
            .map_err(|e| Error::provider_error(format!("Failed to create event source: {}", e)))?;

        create_completions_stream(event_source).await
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        crate::registry::models_dev::load_models_for_provider("openrouter")
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = crate::registry::models_dev::load_models_for_provider("openrouter")?;
        Ok(models.into_iter().find(|m| m.id == id))
    }
}
