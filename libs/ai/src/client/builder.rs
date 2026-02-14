//! Inference client builder

use super::{ClientConfig, Inference, InferenceConfig};
use crate::error::Result;
use crate::provider::Provider;
use crate::providers::{
    anthropic::AnthropicProvider, gemini::GeminiProvider, openai::OpenAIProvider,
    stakpak::StakpakProvider,
};
use crate::registry::ProviderRegistry;

#[cfg(feature = "bedrock")]
use crate::providers::bedrock::BedrockProvider;

/// Builder for creating an Inference client
#[derive(Default)]
pub struct ClientBuilder {
    registry: Option<ProviderRegistry>,
    config: ClientConfig,
}

impl ClientBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure providers using InferenceConfig
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use stakai::{Inference, InferenceConfig};
    ///
    /// let client = Inference::builder()
    ///     .with_inference_config(
    ///         InferenceConfig::new()
    ///             .openai("sk-...", None)
    ///             .anthropic("sk-ant-...", None)
    ///     )
    ///     .build()?;
    /// # Ok::<(), stakai::Error>(())
    /// ```
    pub fn with_inference_config(mut self, inference_config: InferenceConfig) -> Self {
        let mut registry = self.registry.take().unwrap_or_default();

        // Register OpenAI if configured
        if let Some(config) = inference_config.openai_config
            && let Ok(provider) = OpenAIProvider::new(config)
        {
            registry = registry.register("openai", provider);
        }

        // Register Anthropic if configured
        if let Some(config) = inference_config.anthropic_config
            && let Ok(provider) = AnthropicProvider::new(config)
        {
            registry = registry.register("anthropic", provider);
        }

        // Register Gemini if configured
        if let Some(config) = inference_config.gemini_config
            && let Ok(provider) = GeminiProvider::new(config)
        {
            registry = registry.register("google", provider);
        }

        // Register Stakpak if configured
        if let Some(config) = inference_config.stakpak_config
            && let Ok(provider) = StakpakProvider::new(config)
        {
            registry = registry.register("stakpak", provider);
        }

        // Register Bedrock if configured
        #[cfg(feature = "bedrock")]
        if let Some(config) = inference_config.bedrock_config {
            let provider = BedrockProvider::new(config);
            registry = registry.register("amazon-bedrock", provider);
        }

        self.registry = Some(registry);
        self.config = inference_config.client_config;
        self
    }

    /// Use a custom provider registry
    pub fn with_registry(mut self, registry: ProviderRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Register a provider
    pub fn register_provider<P: Provider + 'static>(
        mut self,
        id: impl Into<String>,
        provider: P,
    ) -> Self {
        let registry = self.registry.take().unwrap_or_default();
        self.registry = Some(registry.register(id, provider));
        self
    }

    /// Set client configuration
    pub fn with_config(mut self, config: ClientConfig) -> Self {
        self.config = config;
        self
    }

    /// Set default temperature
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.config.default_temperature = Some(temperature);
        self
    }

    /// Set default max tokens
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.config.default_max_tokens = Some(max_tokens);
        self
    }

    /// Build the inference client
    pub fn build(self) -> Result<Inference> {
        Ok(Inference {
            registry: self.registry.unwrap_or_default(),
            config: self.config,
        })
    }
}
