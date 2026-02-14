//! Bedrock provider implementation
//!
//! Uses `aws-sdk-bedrockruntime` for SigV4 authentication and transport.
//! The request body is the same Anthropic Messages API format, adapted by
//! the `convert` module.

use super::convert::to_bedrock_body;
use super::stream::create_stream;
use super::types::BedrockConfig;
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::anthropic::convert::from_anthropic_response_with_warnings;
use crate::providers::anthropic::types::{AnthropicConfig, AnthropicResponse};
use crate::types::{
    CacheStrategy, GenerateRequest, GenerateResponse, GenerateStream, Headers, Model,
};
use async_trait::async_trait;
use aws_sdk_bedrockruntime::Client as BedrockClient;
use aws_sdk_bedrockruntime::primitives::Blob;
use tokio::sync::OnceCell;

/// AWS Bedrock provider for Anthropic Claude models
///
/// Authentication is handled by the AWS credential chain — no API keys needed.
/// The provider uses `aws-sdk-bedrockruntime` for SigV4 signing and transport.
///
/// The AWS client is lazily initialized on first use, so construction is sync.
///
/// # Example
///
/// ```rust,no_run
/// use stakai::providers::bedrock::{BedrockConfig, BedrockProvider};
///
/// // Sync construction — no .await needed
/// let provider = BedrockProvider::new(BedrockConfig::new("us-east-1"));
/// ```
pub struct BedrockProvider {
    config: BedrockConfig,
    /// Lazily initialized AWS Bedrock client
    client: OnceCell<BedrockClient>,
    /// Internal Anthropic config used for request conversion
    /// (Bedrock uses the same message format as direct Anthropic)
    anthropic_config: AnthropicConfig,
}

impl BedrockProvider {
    /// Create a new Bedrock provider
    ///
    /// Construction is sync — the AWS SDK client is lazily initialized
    /// on the first API call (since credential loading may involve network calls).
    pub fn new(config: BedrockConfig) -> Self {
        let anthropic_config =
            AnthropicConfig::new("bedrock-internal").with_cache_strategy(CacheStrategy::Auto);

        Self {
            config,
            client: OnceCell::new(),
            anthropic_config,
        }
    }

    /// Create provider from environment variables
    ///
    /// Reads `AWS_REGION` (or `AWS_DEFAULT_REGION`) and uses the default
    /// AWS credential chain.
    pub fn from_env() -> Self {
        Self::new(BedrockConfig::from_env())
    }

    /// Get or initialize the AWS Bedrock client
    async fn client(&self) -> Result<&BedrockClient> {
        self.client
            .get_or_try_init(|| async {
                let sdk_config = Self::build_aws_config(&self.config).await?;
                Ok(Self::build_client(&sdk_config, &self.config))
            })
            .await
    }

    /// Build AWS SDK config from BedrockConfig
    async fn build_aws_config(config: &BedrockConfig) -> Result<aws_config::SdkConfig> {
        let mut loader =
            aws_config::from_env().region(aws_config::Region::new(config.region.clone()));

        if let Some(ref profile) = config.profile_name {
            loader = loader.profile_name(profile);
        }

        Ok(loader.load().await)
    }

    /// Build the Bedrock runtime client, applying endpoint override if configured
    fn build_client(sdk_config: &aws_config::SdkConfig, config: &BedrockConfig) -> BedrockClient {
        if let Some(ref endpoint) = config.endpoint_override {
            let bedrock_config = aws_sdk_bedrockruntime::config::Builder::from(sdk_config)
                .endpoint_url(endpoint)
                .build();
            BedrockClient::from_conf(bedrock_config)
        } else {
            BedrockClient::new(sdk_config)
        }
    }
}

/// Map Bedrock InvokeModel errors to stakai Error types
///
/// Converts the SDK error to the crate-level `aws_sdk_bedrockruntime::Error` enum
/// (which has `From<SdkError<InvokeModelError>>` impl), then maps to stakai errors.
///
/// Mapping:
/// - ThrottlingException, ServiceQuotaExceededException → RateLimitExceeded
/// - AccessDeniedException → ProviderError (auth/permissions)
/// - ValidationException → InvalidResponse (bad request body)
/// - ResourceNotFoundException → ProviderNotFound (bad model ID)
/// - ModelTimeoutException, ModelNotReadyException → ProviderError (transient)
/// - ServiceUnavailableException, InternalServerException → ProviderError (service issue)
/// - ModelErrorException → ProviderError (model processing failure)
fn map_invoke_model_error(err: impl Into<aws_sdk_bedrockruntime::Error>) -> Error {
    map_bedrock_error(err.into())
}

/// Map Bedrock InvokeModelWithResponseStream errors to stakai Error types
fn map_invoke_stream_error(err: impl Into<aws_sdk_bedrockruntime::Error>) -> Error {
    map_bedrock_error(err.into())
}

/// Shared error mapping for all Bedrock operations
///
/// Maps the crate-level `aws_sdk_bedrockruntime::Error` enum to stakai error types.
fn map_bedrock_error(err: aws_sdk_bedrockruntime::Error) -> Error {
    use aws_sdk_bedrockruntime::Error as BedrockError;

    match &err {
        BedrockError::ThrottlingException(_) => {
            Error::RateLimitExceeded(format!("Bedrock throttling: {}", err))
        }
        BedrockError::ServiceQuotaExceededException(_) => {
            Error::RateLimitExceeded(format!("Bedrock quota exceeded: {}", err))
        }
        BedrockError::AccessDeniedException(_) => Error::provider_error(format!(
            "Bedrock access denied (check IAM permissions for bedrock:InvokeModel): {}",
            err
        )),
        BedrockError::ValidationException(_) => {
            Error::invalid_response(format!("Bedrock validation error: {}", err))
        }
        BedrockError::ResourceNotFoundException(_) => Error::ProviderNotFound(format!(
            "Bedrock model not found (check model ID and region): {}",
            err
        )),
        BedrockError::ModelTimeoutException(_) => {
            Error::provider_error(format!("Bedrock model timeout: {}", err))
        }
        BedrockError::ModelNotReadyException(_) => Error::provider_error(format!(
            "Bedrock model not ready (SDK will auto-retry up to 5 times): {}",
            err
        )),
        BedrockError::ServiceUnavailableException(_) => {
            Error::provider_error(format!("Bedrock service unavailable: {}", err))
        }
        BedrockError::InternalServerException(_) => {
            Error::provider_error(format!("Bedrock internal server error: {}", err))
        }
        BedrockError::ModelErrorException(_) => {
            Error::provider_error(format!("Bedrock model error: {}", err))
        }
        _ => Error::provider_error(format!("Bedrock error: {}", err)),
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    fn provider_id(&self) -> &str {
        "bedrock"
    }

    fn build_headers(&self, _custom_headers: Option<&Headers>) -> Headers {
        // Bedrock doesn't use custom HTTP headers for auth — SigV4 is handled by the SDK.
        // Return empty headers since the SDK manages everything.
        Headers::new()
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        // Load from models.dev cache — provider ID is "amazon-bedrock" in the registry
        crate::registry::models_dev::load_models_for_provider("amazon-bedrock")
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().find(|m| m.id == id))
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let conversion_result = to_bedrock_body(&request, &self.anthropic_config)?;

        let body_bytes = serde_json::to_vec(&conversion_result.body)
            .map_err(|e| Error::invalid_response(format!("Failed to serialize body: {}", e)))?;

        let client = self.client().await?;
        let response = client
            .invoke_model()
            .model_id(&conversion_result.model_id)
            .content_type("application/json")
            .accept("application/json")
            .body(Blob::new(body_bytes))
            .send()
            .await
            .map_err(map_invoke_model_error)?;

        // Parse the response body
        let response_bytes = response.body().as_ref();
        let anthropic_resp: AnthropicResponse =
            serde_json::from_slice(response_bytes).map_err(|e| {
                Error::invalid_response(format!("Failed to parse Bedrock response: {}", e))
            })?;

        from_anthropic_response_with_warnings(anthropic_resp, conversion_result.warnings)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let conversion_result = to_bedrock_body(&request, &self.anthropic_config)?;

        let body_bytes = serde_json::to_vec(&conversion_result.body)
            .map_err(|e| Error::invalid_response(format!("Failed to serialize body: {}", e)))?;

        let client = self.client().await?;
        let response = client
            .invoke_model_with_response_stream()
            .model_id(&conversion_result.model_id)
            .content_type("application/json")
            .body(Blob::new(body_bytes))
            .send()
            .await
            .map_err(map_invoke_stream_error)?;

        create_stream(response.body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::anthropic::types::AnthropicResponse;

    #[test]
    fn test_error_mapping_throttling() {
        let err = aws_sdk_bedrockruntime::Error::ThrottlingException(
            aws_sdk_bedrockruntime::types::error::ThrottlingException::builder()
                .message("Rate exceeded")
                .build(),
        );
        let mapped = map_bedrock_error(err);
        assert!(
            matches!(mapped, Error::RateLimitExceeded(_)),
            "ThrottlingException should map to RateLimitExceeded, got: {:?}",
            mapped
        );
    }

    #[test]
    fn test_error_mapping_access_denied() {
        let err = aws_sdk_bedrockruntime::Error::AccessDeniedException(
            aws_sdk_bedrockruntime::types::error::AccessDeniedException::builder()
                .message("Not authorized")
                .build(),
        );
        let mapped = map_bedrock_error(err);
        let msg = format!("{}", mapped);
        assert!(
            msg.contains("access denied"),
            "AccessDeniedException should mention access denied, got: {}",
            msg
        );
    }

    #[test]
    fn test_error_mapping_resource_not_found() {
        let err = aws_sdk_bedrockruntime::Error::ResourceNotFoundException(
            aws_sdk_bedrockruntime::types::error::ResourceNotFoundException::builder()
                .message("Model not found")
                .build(),
        );
        let mapped = map_bedrock_error(err);
        assert!(
            matches!(mapped, Error::ProviderNotFound(_)),
            "ResourceNotFoundException should map to ProviderNotFound, got: {:?}",
            mapped
        );
    }

    #[test]
    fn test_error_mapping_validation() {
        let err = aws_sdk_bedrockruntime::Error::ValidationException(
            aws_sdk_bedrockruntime::types::error::ValidationException::builder()
                .message("Invalid request")
                .build(),
        );
        let mapped = map_bedrock_error(err);
        let msg = format!("{}", mapped);
        assert!(
            msg.contains("validation error"),
            "ValidationException should mention validation, got: {}",
            msg
        );
    }

    #[test]
    fn test_error_mapping_quota_exceeded() {
        let err = aws_sdk_bedrockruntime::Error::ServiceQuotaExceededException(
            aws_sdk_bedrockruntime::types::error::ServiceQuotaExceededException::builder()
                .message("Quota exceeded")
                .build(),
        );
        let mapped = map_bedrock_error(err);
        assert!(
            matches!(mapped, Error::RateLimitExceeded(_)),
            "ServiceQuotaExceededException should map to RateLimitExceeded, got: {:?}",
            mapped
        );
    }

    #[test]
    fn test_response_deserialization_from_bedrock_body() {
        // Bedrock returns the same JSON body as direct Anthropic API
        let response_json = serde_json::json!({
            "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "Hello! How can I help you today?"
                }
            ],
            "model": "anthropic.claude-sonnet-4-5-20250929-v1:0",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 25,
                "output_tokens": 15
            }
        });

        let response: AnthropicResponse =
            serde_json::from_value(response_json).expect("Should deserialize Bedrock response");

        assert_eq!(response.id, "msg_01XFDUDYJgAACzvnptvVoYEL");
        assert_eq!(response.role, "assistant");
        assert_eq!(response.content.len(), 1);
        let in_tokens = response.usage.input_tokens;
        assert_eq!(in_tokens, 25);
        assert_eq!(response.usage.output_tokens, 15);

        // Verify it converts to GenerateResponse
        let gen_response =
            from_anthropic_response_with_warnings(response, vec![]).expect("Should convert");
        assert!(!gen_response.content.is_empty());
    }

    #[test]
    fn test_response_deserialization_with_cache_tokens() {
        let response_json = serde_json::json!({
            "id": "msg_cached",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Cached response"}],
            "model": "anthropic.claude-sonnet-4-5-20250929-v1:0",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 10,
                "cache_creation_input_tokens": 50,
                "cache_read_input_tokens": 30
            }
        });

        let response: AnthropicResponse =
            serde_json::from_value(response_json).expect("Should deserialize cached response");

        assert_eq!(response.usage.cache_creation_input_tokens, Some(50));
        assert_eq!(response.usage.cache_read_input_tokens, Some(30));
    }

    #[test]
    fn test_provider_id() {
        let provider = BedrockProvider::new(BedrockConfig::new("us-east-1"));
        assert_eq!(provider.provider_id(), "bedrock");
    }

    #[test]
    fn test_build_headers_returns_empty() {
        let provider = BedrockProvider::new(BedrockConfig::new("us-east-1"));
        let headers = provider.build_headers(None);
        assert!(headers.is_empty());
    }
}
