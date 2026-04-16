//! OpenAI provider implementation

use super::convert::{
    from_openai_response, from_responses_response, to_openai_request, to_responses_request,
};
use super::runtime::{CodexBackendProfile, CompatibleBackendProfile, OfficialBackendProfile};
use super::stream::{
    create_completions_stream, create_responses_stream, create_responses_stream_from_response,
};
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
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct CodexModelsCacheEntry {
    models: Vec<Model>,
    fetched_at: Instant,
}

#[derive(Debug, Deserialize)]
struct CodexModelRecord {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    reasoning: bool,
    #[serde(default, alias = "context_window", alias = "context_length")]
    context_window: Option<u64>,
    #[serde(default, alias = "max_output_tokens", alias = "max_completion_tokens")]
    max_output_tokens: Option<u64>,
    #[serde(default)]
    release_date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CodexModelsResponse {
    Envelope { data: Vec<CodexModelRecord> },
    Array(Vec<CodexModelRecord>),
}

impl CodexModelsResponse {
    fn into_models(self) -> Vec<Model> {
        let records = match self {
            Self::Envelope { data } => data,
            Self::Array(data) => data,
        };

        records
            .into_iter()
            .map(|record| {
                let mut model = Model::new(
                    record.id.clone(),
                    record.name.unwrap_or_else(|| record.id.clone()),
                    "openai",
                    record.reasoning,
                    None,
                    crate::types::ModelLimit::new(
                        record.context_window.unwrap_or(128_000),
                        record.max_output_tokens.unwrap_or(8_192),
                    ),
                );
                model.release_date = record.release_date;
                model
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiMode {
    Responses,
    Completions,
}

fn apply_additional_headers(headers: &mut Headers, additional_headers: &Headers) {
    headers.merge_with(additional_headers);
}

fn apply_codex_headers(
    headers: &mut Headers,
    profile: &CodexBackendProfile,
    additional_headers: &Headers,
) {
    headers.merge_with(additional_headers);
    headers.insert("originator", profile.originator.clone());
    headers.insert("ChatGPT-Account-Id", profile.chatgpt_account_id.clone());
}

fn build_codex_responses_request(
    request: &GenerateRequest,
    stream: bool,
) -> super::types::ResponsesRequest {
    let mut responses_req = to_responses_request(request, stream);
    let instructions = request
        .messages
        .iter()
        .filter(|message| matches!(message.role, crate::types::Role::System))
        .filter_map(|message| message.text())
        .collect::<Vec<_>>()
        .join("\n\n");

    responses_req.instructions = Some(instructions);
    responses_req.store = Some(false);
    responses_req.max_output_tokens = None;
    responses_req.input.retain(|item| {
        !matches!(
            item.get("role").and_then(|role| role.as_str()),
            Some("system") | Some("developer")
        )
    });
    responses_req
}

#[async_trait]
trait OpenAIModelCatalog {
    async fn list_models(
        &self,
        client: &Client,
        headers: &Headers,
        base_url: &str,
    ) -> Result<Vec<Model>>;
}

#[derive(Debug, Default)]
struct OfficialModelCatalog;

#[async_trait]
impl OpenAIModelCatalog for OfficialModelCatalog {
    async fn list_models(
        &self,
        _client: &Client,
        _headers: &Headers,
        _base_url: &str,
    ) -> Result<Vec<Model>> {
        crate::registry::models_dev::load_models_for_provider("openai")
    }
}

#[derive(Debug, Default)]
struct CompatibleModelCatalog;

#[async_trait]
impl OpenAIModelCatalog for CompatibleModelCatalog {
    async fn list_models(
        &self,
        _client: &Client,
        _headers: &Headers,
        _base_url: &str,
    ) -> Result<Vec<Model>> {
        crate::registry::models_dev::load_models_for_provider("openai")
    }
}

#[derive(Debug, Default)]
struct CodexModelCatalog {
    cache: Mutex<Option<CodexModelsCacheEntry>>,
}

impl CodexModelCatalog {
    const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

    fn cached_models(&self) -> Option<Vec<Model>> {
        let Ok(cache) = self.cache.lock() else {
            return None;
        };
        let entry = cache.as_ref()?;
        if entry.fetched_at.elapsed() <= Self::CACHE_TTL {
            return Some(entry.models.clone());
        }
        None
    }

    fn store_models(&self, models: &[Model]) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(CodexModelsCacheEntry {
                models: models.to_vec(),
                fetched_at: Instant::now(),
            });
        }
    }
}

#[async_trait]
impl OpenAIModelCatalog for CodexModelCatalog {
    async fn list_models(
        &self,
        client: &Client,
        headers: &Headers,
        base_url: &str,
    ) -> Result<Vec<Model>> {
        if let Some(models) = self.cached_models() {
            return Ok(models);
        }

        let response = client
            .get(format!("{base_url}/models"))
            .headers(headers.to_reqwest_headers())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "OpenAI Codex models API error {}: {}",
                status, error_text
            )));
        }

        let response_body: CodexModelsResponse = response.json().await?;
        let models = response_body.into_models();
        self.store_models(&models);
        Ok(models)
    }
}

#[derive(Debug, Default, Clone)]
struct CodexStreamTransport;

impl CodexStreamTransport {
    fn build_headers(&self, mut headers: Headers, request: &GenerateRequest) -> Headers {
        headers.insert("Accept", "text/event-stream");
        headers.insert("OpenAI-Beta", "responses=experimental");

        if let Some(ProviderOptions::OpenAI(options)) = request.provider_options.as_ref()
            && let Some(OpenAIApiConfig::Responses(config)) = options.api_config.as_ref()
            && let Some(session_id) = config.session_id.as_ref()
        {
            headers.insert("session_id", session_id.clone());
        }

        headers
    }

    async fn stream(
        &self,
        client: &Client,
        base_url: &str,
        headers: &Headers,
        request: &super::types::ResponsesRequest,
    ) -> Result<GenerateStream> {
        let response = client
            .post(format!("{base_url}/responses"))
            .headers(headers.to_reqwest_headers())
            .json(request)
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

        create_responses_stream_from_response(response).await
    }
}

#[derive(Debug)]
struct OfficialBackend {
    profile: OfficialBackendProfile,
    additional_headers: Headers,
    model_catalog: OfficialModelCatalog,
}

#[derive(Debug)]
struct CompatibleBackend {
    profile: CompatibleBackendProfile,
    additional_headers: Headers,
    model_catalog: CompatibleModelCatalog,
}

#[derive(Debug)]
struct CodexBackend {
    profile: CodexBackendProfile,
    additional_headers: Headers,
    stream_transport: CodexStreamTransport,
    model_catalog: CodexModelCatalog,
}

#[derive(Debug)]
enum OpenAIBackend {
    Official(OfficialBackend),
    Compatible(CompatibleBackend),
    Codex(CodexBackend),
}

impl OpenAIBackend {
    fn base_url(&self) -> &str {
        match self {
            Self::Official(backend) => &backend.profile.base_url,
            Self::Compatible(backend) => &backend.profile.base_url,
            Self::Codex(backend) => &backend.profile.base_url,
        }
    }

    fn apply_headers(&self, headers: &mut Headers) {
        match self {
            Self::Official(backend) => {
                apply_additional_headers(headers, &backend.additional_headers)
            }
            Self::Compatible(backend) => {
                apply_additional_headers(headers, &backend.additional_headers)
            }
            Self::Codex(backend) => {
                apply_codex_headers(headers, &backend.profile, &backend.additional_headers)
            }
        }
    }
}

/// OpenAI provider
pub struct OpenAIProvider {
    config: OpenAIConfig,
    client: Client,
    backend: OpenAIBackend,
}

impl OpenAIProvider {
    const OFFICIAL_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
    const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

    /// Create a new OpenAI provider
    ///
    /// Note: API key validation is skipped when a custom base URL is configured,
    /// as OpenAI-compatible providers like Ollama may not require authentication.
    pub fn new(mut config: OpenAIConfig) -> Result<Self> {
        let is_default_url = config.base_url == Self::OFFICIAL_OPENAI_BASE_URL;
        if config.api_key.is_empty() && is_default_url {
            return Err(Error::MissingApiKey("openai".to_string()));
        }

        config.base_url = config.base_url.trim_end_matches('/').to_string();
        let backend = Self::resolve_backend(&config)?;
        let client = create_platform_tls_client()?;
        Ok(Self {
            config,
            client,
            backend,
        })
    }

    fn resolve_backend(config: &OpenAIConfig) -> Result<OpenAIBackend> {
        let base_url = config.base_url.clone();
        if base_url == Self::OFFICIAL_OPENAI_BASE_URL {
            return Ok(OpenAIBackend::Official(OfficialBackend {
                profile: OfficialBackendProfile { base_url },
                additional_headers: config.custom_headers.clone(),
                model_catalog: OfficialModelCatalog,
            }));
        }

        if base_url == Self::CODEX_BASE_URL || base_url.ends_with("/backend-api/codex") {
            let mut additional_headers = config.custom_headers.clone();
            let chatgpt_account_id =
                additional_headers
                    .remove("ChatGPT-Account-Id")
                    .ok_or_else(|| {
                        Error::ConfigError(
                            "Codex backend requires ChatGPT-Account-Id header during resolution"
                                .to_string(),
                        )
                    })?;
            let originator = additional_headers
                .remove("originator")
                .unwrap_or_else(|| "stakpak".to_string());

            return Ok(OpenAIBackend::Codex(CodexBackend {
                profile: CodexBackendProfile {
                    base_url,
                    originator,
                    chatgpt_account_id,
                },
                additional_headers,
                stream_transport: CodexStreamTransport,
                model_catalog: CodexModelCatalog::default(),
            }));
        }

        Ok(OpenAIBackend::Compatible(CompatibleBackend {
            profile: CompatibleBackendProfile { base_url },
            additional_headers: config.custom_headers.clone(),
            model_catalog: CompatibleModelCatalog,
        }))
    }

    fn requested_api_mode(request: &GenerateRequest) -> Option<ApiMode> {
        match request.provider_options.as_ref() {
            Some(ProviderOptions::OpenAI(opts)) => match &opts.api_config {
                Some(OpenAIApiConfig::Responses(_)) => Some(ApiMode::Responses),
                Some(OpenAIApiConfig::Completions(_)) => Some(ApiMode::Completions),
                None => None,
            },
            _ => None,
        }
    }

    fn effective_api_mode(&self, request: &GenerateRequest) -> ApiMode {
        if let Some(mode) = Self::requested_api_mode(request) {
            return match self.backend {
                OpenAIBackend::Codex(_) => ApiMode::Responses,
                _ => mode,
            };
        }

        match &self.backend {
            OpenAIBackend::Official(_) => ApiMode::Responses,
            OpenAIBackend::Compatible(_) => self
                .config
                .default_openai_options
                .as_ref()
                .and_then(|options| options.api_config.as_ref())
                .map(|api_config| match api_config {
                    OpenAIApiConfig::Responses(_) => ApiMode::Responses,
                    OpenAIApiConfig::Completions(_) => ApiMode::Completions,
                })
                .unwrap_or(ApiMode::Completions),
            OpenAIBackend::Codex(_) => ApiMode::Responses,
        }
    }

    fn build_responses_request(
        &self,
        request: &GenerateRequest,
        stream: bool,
    ) -> super::types::ResponsesRequest {
        match &self.backend {
            OpenAIBackend::Codex(_) => build_codex_responses_request(request, stream),
            _ => to_responses_request(request, stream),
        }
    }

    fn build_stream_headers(&self, request: &GenerateRequest) -> Headers {
        let headers = self.build_headers(request.options.headers.as_ref());
        match &self.backend {
            OpenAIBackend::Codex(backend) => {
                backend.stream_transport.build_headers(headers, request)
            }
            _ => headers,
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

        self.backend.apply_headers(&mut headers);

        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        headers
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let headers = self.build_headers(request.options.headers.as_ref());

        if matches!(self.effective_api_mode(&request), ApiMode::Responses) {
            let url = format!("{}/responses", self.backend.base_url());
            let responses_req = self.build_responses_request(&request, false);

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
            let url = format!("{}/chat/completions", self.backend.base_url());
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
        let api_mode = self.effective_api_mode(&request);
        let headers = if matches!(api_mode, ApiMode::Responses) {
            self.build_stream_headers(&request)
        } else {
            self.build_headers(request.options.headers.as_ref())
        };

        if matches!(api_mode, ApiMode::Responses) {
            let url = format!("{}/responses", self.backend.base_url());
            let responses_req = self.build_responses_request(&request, true);

            match &self.backend {
                OpenAIBackend::Codex(backend) => {
                    backend
                        .stream_transport
                        .stream(
                            &self.client,
                            backend.profile.base_url.as_str(),
                            &headers,
                            &responses_req,
                        )
                        .await
                }
                _ => {
                    let req_builder = self
                        .client
                        .post(&url)
                        .headers(headers.to_reqwest_headers())
                        .json(&responses_req);

                    let event_source = EventSource::new(req_builder).map_err(|e| {
                        Error::stream_error(format!("Failed to create event source: {}", e))
                    })?;

                    create_responses_stream(event_source).await
                }
            }
        } else {
            let url = format!("{}/chat/completions", self.backend.base_url());
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
        let headers = self.build_headers(None);
        match &self.backend {
            OpenAIBackend::Codex(backend) => match backend
                .model_catalog
                .list_models(&self.client, &headers, backend.profile.base_url.as_str())
                .await
            {
                Ok(models) => Ok(models),
                Err(_error) => crate::registry::models_dev::load_models_for_provider("openai"),
            },
            OpenAIBackend::Official(backend) => {
                backend
                    .model_catalog
                    .list_models(&self.client, &headers, backend.profile.base_url.as_str())
                    .await
            }
            OpenAIBackend::Compatible(backend) => {
                backend
                    .model_catalog
                    .list_models(&self.client, &headers, backend.profile.base_url.as_str())
                    .await
            }
        }
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().find(|model| model.id == id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
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
        assert!(matches!(
            provider.effective_api_mode(&req),
            ApiMode::Responses
        ));
    }

    #[test]
    fn test_defaults_to_completions_for_custom_openai_compatible_url() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key").with_base_url("http://localhost:11434/v1"),
        )
        .unwrap();
        let req = make_request(None);
        assert!(matches!(
            provider.effective_api_mode(&req),
            ApiMode::Completions
        ));
    }

    #[test]
    fn test_explicit_completions_overrides_official_default() {
        let provider = OpenAIProvider::new(OpenAIConfig::new("test-key")).unwrap();
        let req = make_request(Some(ProviderOptions::OpenAI(OpenAIOptions::completions())));
        assert!(matches!(
            provider.effective_api_mode(&req),
            ApiMode::Completions
        ));
    }

    #[test]
    fn test_explicit_responses_overrides_custom_endpoint_default() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key").with_base_url("http://localhost:11434/v1"),
        )
        .unwrap();
        let req = make_request(Some(ProviderOptions::OpenAI(OpenAIOptions::responses())));
        assert!(matches!(
            provider.effective_api_mode(&req),
            ApiMode::Responses
        ));
    }

    #[test]
    fn test_codex_config_defaults_to_responses_api() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url("https://chatgpt.com/backend-api/codex")
                .with_custom_header("ChatGPT-Account-Id", "acct_test_123")
                .with_default_openai_options(OpenAIOptions::responses()),
        )
        .unwrap();
        let req = make_request(None);
        assert!(matches!(
            provider.effective_api_mode(&req),
            ApiMode::Responses
        ));
    }

    #[test]
    fn test_codex_backend_requires_account_id_during_resolution() {
        let result = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url("https://chatgpt.com/backend-api/codex")
                .with_default_openai_options(OpenAIOptions::responses()),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_build_headers_includes_config_custom_headers() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_custom_header("originator", "stakpak")
                .with_custom_header("ChatGPT-Account-Id", "acct_test_123"),
        )
        .unwrap();

        let headers = provider.build_headers(None);

        assert_eq!(headers.get("originator"), Some(&"stakpak".to_string()));
        assert_eq!(
            headers.get("ChatGPT-Account-Id"),
            Some(&"acct_test_123".to_string())
        );
    }

    #[test]
    fn test_codex_responses_request_uses_instructions_field() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url("https://chatgpt.com/backend-api/codex")
                .with_custom_header("ChatGPT-Account-Id", "acct_test_123")
                .with_default_openai_options(OpenAIOptions::responses()),
        )
        .expect("provider");
        let mut req = GenerateRequest::new(
            Model::custom("codex-mini-latest", "openai"),
            vec![
                Message::new(Role::System, "You are a helpful assistant"),
                Message::new(Role::User, "Hello"),
            ],
        );
        req.provider_options = Some(ProviderOptions::OpenAI(OpenAIOptions::responses()));

        let responses_req = provider.build_responses_request(&req, false);

        assert_eq!(
            responses_req.instructions,
            Some("You are a helpful assistant".to_string())
        );
        assert_eq!(responses_req.store, Some(false));
        assert!(responses_req.max_output_tokens.is_none());
        assert_eq!(responses_req.input.len(), 1);
        assert_eq!(responses_req.input[0]["role"], "user");
    }

    #[test]
    fn test_codex_stream_headers_include_sse_beta_and_session_id() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url("https://chatgpt.com/backend-api/codex")
                .with_custom_header("ChatGPT-Account-Id", "acct_test_123")
                .with_default_openai_options(OpenAIOptions::responses()),
        )
        .expect("provider");
        let mut req = GenerateRequest::new(
            Model::custom("codex-mini-latest", "openai"),
            vec![Message::new(Role::User, "Hello")],
        );
        req.provider_options = Some(ProviderOptions::OpenAI(OpenAIOptions {
            api_config: Some(OpenAIApiConfig::Responses(crate::types::ResponsesConfig {
                session_id: Some("session-123".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        }));

        let headers = provider.build_stream_headers(&req);

        assert_eq!(
            headers.get("Accept"),
            Some(&"text/event-stream".to_string())
        );
        assert_eq!(
            headers.get("OpenAI-Beta"),
            Some(&"responses=experimental".to_string())
        );
        assert_eq!(headers.get("session_id"), Some(&"session-123".to_string()));
    }

    #[test]
    fn test_codex_responses_request_strips_max_output_tokens() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url("https://chatgpt.com/backend-api/codex")
                .with_custom_header("ChatGPT-Account-Id", "acct_test_123")
                .with_default_openai_options(OpenAIOptions::responses()),
        )
        .expect("provider");
        let mut req = GenerateRequest::new(
            Model::custom("codex-mini-latest", "openai"),
            vec![
                Message::new(Role::System, "You are a helpful assistant"),
                Message::new(Role::User, "Hello"),
            ],
        );
        req.options.max_tokens = Some(512);
        req.provider_options = Some(ProviderOptions::OpenAI(OpenAIOptions::responses()));

        let responses_req = provider.build_responses_request(&req, false);

        assert!(responses_req.max_output_tokens.is_none());
    }

    #[tokio::test]
    async fn test_list_models_uses_codex_models_endpoint() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/backend-api/codex/models")
            .match_header("authorization", "Bearer test-key")
            .match_header("chatgpt-account-id", "acct_test_123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"data":[{"id":"codex-mini-latest","name":"Codex Mini Latest","reasoning":true,"context_window":200000,"max_output_tokens":8192}]}"#,
            )
            .expect(1)
            .create();

        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url(format!("{}/backend-api/codex", server.url()))
                .with_custom_header("ChatGPT-Account-Id", "acct_test_123"),
        )
        .expect("provider");

        let models = provider.list_models().await.expect("codex models");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "codex-mini-latest");
        assert_eq!(models[0].name, "Codex Mini Latest");
        assert!(models[0].reasoning);
        mock.assert();
    }

    #[tokio::test]
    async fn test_list_models_caches_codex_models_for_ttl_window() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/backend-api/codex/models")
            .match_header("authorization", "Bearer test-key")
            .match_header("chatgpt-account-id", "acct_test_123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"id":"codex-mini-latest","name":"Codex Mini Latest"}]}"#)
            .expect(1)
            .create();

        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url(format!("{}/backend-api/codex", server.url()))
                .with_custom_header("ChatGPT-Account-Id", "acct_test_123"),
        )
        .expect("provider");

        let first = provider.list_models().await.expect("first model list");
        let second = provider.list_models().await.expect("second model list");

        assert_eq!(first, second);
        mock.assert();
    }
}
