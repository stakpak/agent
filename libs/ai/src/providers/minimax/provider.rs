//! MiniMax provider implementation

use super::convert::to_minimax_request;
use super::stream::create_stream;
use super::types::MiniMaxConfig;
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::openai::types::{ChatCompletionResponse, ChatUsage};
use crate::providers::tls::create_platform_tls_client;
use crate::types::{
    FinishReason, FinishReasonKind, GenerateRequest, GenerateResponse, GenerateStream, Headers,
    InputTokenDetails, Model, ModelCost, ModelLimit, OutputTokenDetails, ResponseContent, ToolCall,
    Usage,
};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;
use serde_json::json;

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

    /// Return the static list of available MiniMax models
    fn static_models() -> Vec<Model> {
        vec![
            Model {
                id: "MiniMax-M2.7".to_string(),
                provider: "minimax".to_string(),
                name: "MiniMax M2.7".to_string(),
                reasoning: false,
                cost: Some(ModelCost::new(0.2, 1.1)),
                limit: ModelLimit::new(1_000_000, 131_072),
                release_date: None,
            },
            Model {
                id: "MiniMax-M2.7-highspeed".to_string(),
                provider: "minimax".to_string(),
                name: "MiniMax M2.7 Highspeed".to_string(),
                reasoning: false,
                cost: Some(ModelCost::new(0.2, 1.1)),
                limit: ModelLimit::new(1_000_000, 131_072),
                release_date: None,
            },
            Model {
                id: "MiniMax-M2.5".to_string(),
                provider: "minimax".to_string(),
                name: "MiniMax M2.5".to_string(),
                reasoning: false,
                cost: Some(ModelCost::new(0.2, 1.1)),
                limit: ModelLimit::new(204_000, 131_072),
                release_date: None,
            },
            Model {
                id: "MiniMax-M2.5-highspeed".to_string(),
                provider: "minimax".to_string(),
                name: "MiniMax M2.5 Highspeed".to_string(),
                reasoning: false,
                cost: Some(ModelCost::new(0.2, 1.1)),
                limit: ModelLimit::new(204_000, 131_072),
                release_date: None,
            },
        ]
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
        Ok(Self::static_models())
    }
}

/// Convert MiniMax (OpenAI-compatible) response to SDK response
fn from_minimax_response(resp: ChatCompletionResponse) -> Result<GenerateResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| Error::invalid_response("No choices in response"))?;

    let content = parse_minimax_message(&choice.message)?;

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

    let usage = usage_from_chat_usage(&resp.usage);

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

/// Convert OpenAI-compatible ChatUsage to SDK Usage
fn usage_from_chat_usage(usage: &ChatUsage) -> Usage {
    let cache_read = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens)
        .unwrap_or(0);

    Usage::with_details(
        InputTokenDetails {
            total: Some(usage.prompt_tokens),
            no_cache: Some(usage.prompt_tokens.saturating_sub(cache_read)),
            cache_read: (cache_read > 0).then_some(cache_read),
            cache_write: None,
        },
        OutputTokenDetails {
            total: Some(usage.completion_tokens),
            text: None,
            reasoning: usage
                .completion_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens),
        },
        Some(serde_json::to_value(usage).unwrap_or_default()),
    )
}

/// Parse MiniMax message content
fn parse_minimax_message(
    msg: &crate::providers::openai::types::ChatMessage,
) -> Result<Vec<ResponseContent>> {
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
                metadata: None,
            }));
        }
    }

    Ok(content)
}

/// Parse MiniMax API error and return user-friendly message
pub(crate) fn parse_minimax_error(error_text: &str, status_code: u16) -> String {
    // Try to parse as JSON error
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(error_text)
        && let Some(error) = json.get("error")
    {
        let message = error.get("message").and_then(|m| m.as_str()).unwrap_or("");
        let error_type = error.get("type").and_then(|t| t.as_str()).unwrap_or("");

        // Check for rate limit
        if error_type == "rate_limit_error" || status_code == 429 {
            return format!(
                "Rate limited. Please wait a moment and try again. {}",
                message
            );
        }

        // Check for authentication errors
        if error_type == "authentication_error" || status_code == 401 {
            return format!(
                "Authentication failed. Please check your MiniMax API key. {}",
                message
            );
        }

        // Return the message if we have one
        if !message.is_empty() {
            return message.to_string();
        }
    }

    // Fallback to raw error
    format!("MiniMax API error {}: {}", status_code, error_text)
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
    fn test_static_models() {
        let models = MiniMaxProvider::static_models();
        assert_eq!(models.len(), 4);
        assert!(models.iter().any(|m| m.id == "MiniMax-M2.7"));
        assert!(models.iter().any(|m| m.id == "MiniMax-M2.7-highspeed"));
        assert!(models.iter().any(|m| m.id == "MiniMax-M2.5"));
        assert!(models.iter().any(|m| m.id == "MiniMax-M2.5-highspeed"));
        for model in &models {
            assert_eq!(model.provider, "minimax");
        }
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
