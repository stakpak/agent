//! High-level client API

mod builder;
mod config;

pub use builder::ClientBuilder;
pub use config::{ClientConfig, InferenceConfig};

use crate::error::Result;
use crate::registry::ProviderRegistry;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream};

#[cfg(feature = "tracing")]
use tracing::Instrument;

#[cfg(feature = "tracing")]
use crate::tracing as gen_ai_tracing;

/// High-level inference client for AI generation
#[derive(Clone)]
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
    /// # use stakai::{Inference, GenerateRequest, Message, Model, Role};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Inference::new();
    /// let request = GenerateRequest::new(
    ///     Model::custom("gpt-4", "openai"),
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
            let span = tracing::info_span!(
                "chat",
                "gen_ai.operation.name" = "chat",
                "gen_ai.provider.name" = %request.model.provider,
                "gen_ai.request.model" = %request.model.id,
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
                // Non-standard: Cache token metrics (not part of OTel GenAI semantic conventions)
                "gen_ai.usage.cache_read_input_tokens" = tracing::field::Empty,
                "gen_ai.usage.cache_write_input_tokens" = tracing::field::Empty,
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

                // Non-standard: Cache token metrics (not part of OTel GenAI semantic conventions)
                if let Some(cache_read) = response.usage.cache_read_tokens() {
                    tracing::Span::current()
                        .record("gen_ai.usage.cache_read_input_tokens", cache_read as i64);
                }
                if let Some(cache_write) = response.usage.cache_write_tokens() {
                    tracing::Span::current()
                        .record("gen_ai.usage.cache_write_input_tokens", cache_write as i64);
                }

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
        let provider = self.registry.get_provider(&request.model.provider)?;
        provider.generate(request.clone()).await
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
    /// # use stakai::{Inference, GenerateRequest, Message, Model, Role, StreamEvent};
    /// # use futures::StreamExt;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Inference::new();
    /// let request = GenerateRequest::new(
    ///     Model::custom("gpt-4", "openai"),
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
            let span = tracing::info_span!(
                "chat",
                "gen_ai.operation.name" = "chat",
                "gen_ai.provider.name" = %request.model.provider,
                "gen_ai.request.model" = %request.model.id,
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
                // Non-standard: Cache token metrics (not part of OTel GenAI semantic conventions)
                "gen_ai.usage.cache_read_input_tokens" = tracing::field::Empty,
                "gen_ai.usage.cache_write_input_tokens" = tracing::field::Empty,
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
        let provider = self.registry.get_provider(&request.model.provider)?;
        provider.stream(request.clone()).await
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
