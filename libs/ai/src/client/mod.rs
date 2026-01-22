//! High-level client API

mod builder;
mod config;

pub use builder::ClientBuilder;
pub use config::{ClientConfig, InferenceConfig};

use crate::error::{Error, Result};
use crate::registry::ProviderRegistry;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream};

#[cfg(feature = "tracing")]
use tracing::Instrument;

#[cfg(feature = "tracing")]
use crate::tracing as gen_ai_tracing;

/// High-level inference client for AI generation
pub struct Inference {
    registry: ProviderRegistry,
    #[allow(dead_code)]
    config: ClientConfig,
}

impl Inference {
    /// Create a new inference client with default configuration
    ///
    /// Providers are auto-registered from environment variables:
    /// - `OPENAI_API_KEY` for OpenAI
    /// - `ANTHROPIC_API_KEY` for Anthropic
    /// - `GEMINI_API_KEY` for Google Gemini
    pub fn new() -> Self {
        Self::builder()
            .build()
            .expect("Failed to build Inference client")
    }

    /// Create an inference client with custom provider configuration
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use stakai::{Inference, InferenceConfig};
    ///
    /// let client = Inference::with_config(
    ///     InferenceConfig::new()
    ///         .openai("sk-...", None)
    ///         .anthropic("sk-ant-...", None)
    ///         .gemini("your-key", None)
    /// );
    /// ```
    pub fn with_config(config: InferenceConfig) -> Result<Self> {
        Self::builder().with_inference_config(config).build()
    }

    /// Create an inference client builder
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Generate a response
    ///
    /// When the `tracing` feature is enabled, this operation is automatically
    /// traced with [GenAI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/).
    ///
    /// # Arguments
    ///
    /// * `request` - Generation request with model identifier (e.g., "gpt-4" or "openai/gpt-4")
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{Inference, GenerateRequest, Message, Role};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Inference::new();
    /// let request = GenerateRequest::new(
    ///     "openai/gpt-4",
    ///     vec![Message::new(Role::User, "Hello!")]
    /// );
    /// let response = client.generate(&request).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Tracing
    ///
    /// When the `tracing` feature is enabled, spans are automatically emitted with:
    /// - `gen_ai.operation.name`: "chat"
    /// - `gen_ai.provider.name`: provider name (e.g., "openai", "anthropic")
    /// - `gen_ai.request.model`: model identifier
    /// - `gen_ai.input.messages`: JSON array of input messages (opt-in)
    /// - `gen_ai.output.messages`: JSON array of output messages (opt-in)
    /// - `gen_ai.usage.input_tokens`: prompt tokens used
    /// - `gen_ai.usage.output_tokens`: completion tokens used
    /// - `gen_ai.response.finish_reasons`: array of finish reasons
    pub async fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse> {
        #[cfg(feature = "tracing")]
        {
            let (provider_id, _) = self.parse_model(&request.model)?;
            let span = tracing::info_span!(
                "chat",
                "gen_ai.operation.name" = "chat",
                "gen_ai.provider.name" = %provider_id,
                "gen_ai.request.model" = %request.model,
                "gen_ai.request.temperature" = tracing::field::Empty,
                "gen_ai.request.max_tokens" = tracing::field::Empty,
                "gen_ai.request.top_p" = tracing::field::Empty,
                "gen_ai.request.frequency_penalty" = tracing::field::Empty,
                "gen_ai.request.presence_penalty" = tracing::field::Empty,
                "gen_ai.input.messages" = tracing::field::Empty,
                "gen_ai.output.messages" = tracing::field::Empty,
                "gen_ai.tool.definitions" = tracing::field::Empty,
                "gen_ai.usage.input_tokens" = tracing::field::Empty,
                "gen_ai.usage.output_tokens" = tracing::field::Empty,
                "gen_ai.response.finish_reasons" = tracing::field::Empty,
            );

            // Record optional request parameters
            if let Some(t) = request.options.temperature {
                span.record("gen_ai.request.temperature", t);
            }
            if let Some(m) = request.options.max_tokens {
                span.record("gen_ai.request.max_tokens", m as i64);
            }
            if let Some(p) = request.options.top_p {
                span.record("gen_ai.request.top_p", p);
            }
            if let Some(fp) = request.options.frequency_penalty {
                span.record("gen_ai.request.frequency_penalty", fp);
            }
            if let Some(pp) = request.options.presence_penalty {
                span.record("gen_ai.request.presence_penalty", pp);
            }

            // Record custom telemetry metadata and tool definitions
            {
                let _guard = span.enter();
                if let Some(ref metadata) = request.telemetry_metadata {
                    gen_ai_tracing::record_telemetry_metadata(metadata);
                }
                if let Some(ref tools) = request.options.tools {
                    gen_ai_tracing::record_tool_definitions(tools);
                }
            }

            // Clone data needed inside the async block
            let messages = request.messages.clone();

            return async {
                // Record input messages as span attribute
                gen_ai_tracing::record_input_messages(&messages);

                let response = self.generate_internal(request).await?;

                // Record response attributes
                tracing::Span::current().record(
                    "gen_ai.usage.input_tokens",
                    response.usage.prompt_tokens as i64,
                );
                tracing::Span::current().record(
                    "gen_ai.usage.output_tokens",
                    response.usage.completion_tokens as i64,
                );

                // finish_reasons is an array per OTel spec
                let finish_reason = format!("{:?}", response.finish_reason.unified);
                let finish_reasons_json =
                    serde_json::to_string(&vec![&finish_reason]).unwrap_or_default();
                tracing::Span::current().record(
                    "gen_ai.response.finish_reasons",
                    finish_reasons_json.as_str(),
                );

                // Record response content as span attribute
                gen_ai_tracing::record_response_content(&response, &finish_reason);

                Ok(response)
            }
            .instrument(span)
            .await;
        }

        #[cfg(not(feature = "tracing"))]
        self.generate_internal(request).await
    }

    /// Internal generate implementation
    async fn generate_internal(&self, request: &GenerateRequest) -> Result<GenerateResponse> {
        let (provider_id, model_id) = self.parse_model(&request.model)?;
        let provider = self.registry.get_provider(&provider_id)?;

        let mut req = request.clone();
        req.model = model_id.to_string();
        provider.generate(req).await
    }

    /// Generate a streaming response
    ///
    /// When the `tracing` feature is enabled, the stream is automatically
    /// traced with [GenAI semantic conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/).
    /// Token usage is recorded when the stream completes (on the `Finish` event).
    ///
    /// # Arguments
    ///
    /// * `request` - Generation request with model identifier
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{Inference, GenerateRequest, Message, Role, StreamEvent};
    /// # use futures::StreamExt;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Inference::new();
    /// let request = GenerateRequest::new(
    ///     "openai/gpt-4",
    ///     vec![Message::new(Role::User, "Count to 5")]
    /// );
    /// let mut stream = client.stream(&request).await?;
    ///
    /// while let Some(event) = stream.next().await {
    ///     match event? {
    ///         StreamEvent::TextDelta { delta, .. } => print!("{}", delta),
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Tracing
    ///
    /// When the `tracing` feature is enabled, spans are automatically emitted with:
    /// - `gen_ai.operation.name`: "chat" (streaming is still a chat operation)
    /// - `gen_ai.provider.name`: provider name (e.g., "openai", "anthropic")
    /// - `gen_ai.request.model`: model identifier
    /// - `gen_ai.input.messages`: JSON array of input messages (opt-in)
    /// - `gen_ai.output.messages`: JSON array of output messages (opt-in, recorded on finish)
    /// - `gen_ai.usage.input_tokens`: prompt tokens (recorded on stream finish)
    /// - `gen_ai.usage.output_tokens`: completion tokens (recorded on stream finish)
    /// - `gen_ai.response.finish_reasons`: array of finish reasons (recorded on stream finish)
    pub async fn stream(&self, request: &GenerateRequest) -> Result<GenerateStream> {
        #[cfg(feature = "tracing")]
        {
            let (provider_id, _) = self.parse_model(&request.model)?;
            let span = tracing::info_span!(
                "chat",
                "gen_ai.operation.name" = "chat",
                "gen_ai.provider.name" = %provider_id,
                "gen_ai.request.model" = %request.model,
                "gen_ai.request.temperature" = tracing::field::Empty,
                "gen_ai.request.max_tokens" = tracing::field::Empty,
                "gen_ai.request.top_p" = tracing::field::Empty,
                "gen_ai.request.frequency_penalty" = tracing::field::Empty,
                "gen_ai.request.presence_penalty" = tracing::field::Empty,
                "gen_ai.input.messages" = tracing::field::Empty,
                "gen_ai.output.messages" = tracing::field::Empty,
                "gen_ai.tool.definitions" = tracing::field::Empty,
                "gen_ai.usage.input_tokens" = tracing::field::Empty,
                "gen_ai.usage.output_tokens" = tracing::field::Empty,
                "gen_ai.response.finish_reasons" = tracing::field::Empty,
            );

            // Record optional request parameters
            if let Some(t) = request.options.temperature {
                span.record("gen_ai.request.temperature", t);
            }
            if let Some(m) = request.options.max_tokens {
                span.record("gen_ai.request.max_tokens", m as i64);
            }
            if let Some(p) = request.options.top_p {
                span.record("gen_ai.request.top_p", p);
            }
            if let Some(fp) = request.options.frequency_penalty {
                span.record("gen_ai.request.frequency_penalty", fp);
            }
            if let Some(pp) = request.options.presence_penalty {
                span.record("gen_ai.request.presence_penalty", pp);
            }

            // Record input messages, custom telemetry metadata, and tool definitions
            {
                let _guard = span.enter();
                gen_ai_tracing::record_input_messages(&request.messages);
                if let Some(ref metadata) = request.telemetry_metadata {
                    gen_ai_tracing::record_telemetry_metadata(metadata);
                }
                if let Some(ref tools) = request.options.tools {
                    gen_ai_tracing::record_tool_definitions(tools);
                }
            }

            // Create the inner stream, then wrap it with our span
            let inner_stream = self.stream_internal(request).await?;

            // Return a stream that will record usage and completion when it finishes
            Ok(GenerateStream::with_span(Box::pin(inner_stream), span))
        }

        #[cfg(not(feature = "tracing"))]
        self.stream_internal(request).await
    }

    /// Internal stream implementation
    async fn stream_internal(&self, request: &GenerateRequest) -> Result<GenerateStream> {
        let (provider_id, model_id) = self.parse_model(&request.model)?;
        let provider = self.registry.get_provider(&provider_id)?;

        let mut req = request.clone();
        req.model = model_id.to_string();
        provider.stream(req).await
    }

    /// Parse model string into provider and model ID
    pub(crate) fn parse_model<'a>(&self, model: &'a str) -> Result<(String, &'a str)> {
        if let Some((provider, model_id)) = model.split_once('/') {
            // Explicit provider/model format
            Ok((provider.to_string(), model_id))
        } else {
            // Auto-detect provider from model name
            let provider = self.detect_provider(model)?;
            Ok((provider, model))
        }
    }

    /// Detect provider from model name using heuristics
    pub(crate) fn detect_provider(&self, model: &str) -> Result<String> {
        let model_lower = model.to_lowercase();

        if model_lower.starts_with("gpt-") || model_lower.starts_with("o1-") {
            Ok("openai".to_string())
        } else if model_lower.starts_with("claude-") {
            Ok("anthropic".to_string())
        } else if model_lower.starts_with("gemini-") {
            Ok("google".to_string())
        } else {
            Err(Error::UnknownProvider(model.to_string()))
        }
    }

    /// Get the provider registry
    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }
}

impl Default for Inference {
    fn default() -> Self {
        Self::new()
    }
}
