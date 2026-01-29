//! Stakpak provider implementation

use super::stream::create_stream;
use super::types::{StakpakProviderConfig, StakpakResponse};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::openai::convert::to_openai_request;
use crate::providers::tls::create_platform_tls_client;
use crate::types::{
    FinishReason, FinishReasonKind, GenerateRequest, GenerateResponse, GenerateStream, Headers,
    InputTokenDetails, OutputTokenDetails, ResponseContent, ToolCall, Usage,
};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;
use serde_json::json;

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

        let client = create_platform_tls_client()?;
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

        // Stakpak uses OpenAI-compatible API for requests
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

        let resp: StakpakResponse = response.json().await?;
        from_stakpak_response(resp)
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

/// Convert Stakpak response to SDK response
fn from_stakpak_response(resp: StakpakResponse) -> Result<GenerateResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| Error::invalid_response("No choices in response"))?;

    let content = parse_stakpak_message(&choice.message)?;

    let finish_reason = match choice.finish_reason.as_deref() {
        Some("stop") => FinishReason::with_raw(FinishReasonKind::Stop, "stop"),
        Some("length") => FinishReason::with_raw(FinishReasonKind::Length, "length"),
        Some("tool_calls") => FinishReason::with_raw(FinishReasonKind::ToolCalls, "tool_calls"),
        Some("content_filter") => {
            FinishReason::with_raw(FinishReasonKind::ContentFilter, "content_filter")
        }
        Some(raw) => FinishReason::with_raw(FinishReasonKind::Other, raw),
        None => FinishReason::other(),
    };

    let prompt_tokens = resp.usage.prompt_tokens;
    let completion_tokens = resp.usage.completion_tokens;

    // Extract cache tokens from Stakpak's response format
    let details = resp.usage.prompt_tokens_details.as_ref();
    let cache_read = details.and_then(|d| d.cache_read_input_tokens).unwrap_or(0);
    let cache_write = details
        .and_then(|d| d.cache_write_input_tokens)
        .unwrap_or(0);

    let usage = Usage::with_details(
        InputTokenDetails {
            total: Some(prompt_tokens),
            no_cache: Some(
                prompt_tokens
                    .saturating_sub(cache_read)
                    .saturating_sub(cache_write),
            ),
            cache_read: (cache_read > 0).then_some(cache_read),
            cache_write: (cache_write > 0).then_some(cache_write),
        },
        OutputTokenDetails {
            total: Some(completion_tokens),
            text: None,
            reasoning: None,
        },
        Some(serde_json::to_value(&resp.usage).unwrap_or_default()),
    );

    Ok(GenerateResponse {
        content,
        usage,
        finish_reason,
        metadata: Some(json!({
            "id": resp.id,
            "model": resp.model,
            "created": resp.created,
            "object": resp.object,
        })),
        warnings: None,
    })
}

/// Parse Stakpak message content
fn parse_stakpak_message(msg: &super::types::StakpakMessage) -> Result<Vec<ResponseContent>> {
    let mut content = Vec::new();

    // Handle text content
    if let Some(content_value) = &msg.content
        && let Some(text) = content_value.as_str()
        && !text.is_empty()
    {
        content.push(ResponseContent::Text {
            text: text.to_string(),
        });
    }

    // Handle tool calls
    if let Some(tool_calls) = &msg.tool_calls {
        for tc in tool_calls {
            content.push(ResponseContent::ToolCall(ToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| json!({})),
            }));
        }
    }

    Ok(content)
}
