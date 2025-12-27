//! High-level client API

mod builder;
mod config;

pub use builder::ClientBuilder;
pub use config::{ClientConfig, InferenceConfig};

use crate::error::{Error, Result};
use crate::registry::ProviderRegistry;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream};

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
    /// # Arguments
    ///
    /// * `request` - Generation request with model identifier (e.g., "gpt-4" or "openai:gpt-4")
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{Inference, GenerateRequest, Message, Role};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Inference::new();
    /// let request = GenerateRequest::new(
    ///     "openai:gpt-4",
    ///     vec![Message::new(Role::User, "Hello!")]
    /// );
    /// let response = client.generate(&request).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse> {
        let (provider_id, model_id) = self.parse_model(&request.model)?;
        let provider = self.registry.get_provider(&provider_id)?;

        let mut req = request.clone();
        req.model = model_id.to_string();
        provider.generate(req).await
    }

    /// Generate a streaming response
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
    ///     "openai:gpt-4",
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
    pub async fn stream(&self, request: &GenerateRequest) -> Result<GenerateStream> {
        let (provider_id, model_id) = self.parse_model(&request.model)?;
        let provider = self.registry.get_provider(&provider_id)?;

        let mut req = request.clone();
        req.model = model_id.to_string();
        provider.stream(req).await
    }

    /// Parse model string into provider and model ID
    pub(crate) fn parse_model<'a>(&self, model: &'a str) -> Result<(String, &'a str)> {
        if let Some((provider, model_id)) = model.split_once(':') {
            // Explicit provider:model format
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
