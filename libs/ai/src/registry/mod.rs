//! Provider registry for runtime provider management

pub mod models_dev;

use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::types::Model;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for managing AI providers
#[derive(Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Register a provider
    pub fn register<P: Provider + 'static>(mut self, id: impl Into<String>, provider: P) -> Self {
        self.providers.insert(id.into(), Arc::new(provider));
        self
    }

    /// Get a provider by ID
    pub fn get_provider(&self, id: &str) -> Result<Arc<dyn Provider>> {
        self.providers
            .get(id)
            .cloned()
            .ok_or_else(|| Error::ProviderNotFound(id.to_string()))
    }

    /// List all registered provider IDs
    pub fn list_providers(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Check if a provider is registered
    pub fn has_provider(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    /// Get all models from all configured providers
    pub async fn models(&self) -> Result<Vec<Model>> {
        let mut all_models = Vec::new();
        for provider in self.providers.values() {
            all_models.extend(provider.list_models().await?);
        }
        Ok(all_models)
    }

    /// Find a model by ID across all configured providers
    pub async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        for provider in self.providers.values() {
            if let Some(model) = provider.get_model(id).await? {
                return Ok(Some(model));
            }
        }
        Ok(None)
    }

    /// Get all models from a specific provider
    pub async fn models_for_provider(&self, provider_id: &str) -> Result<Vec<Model>> {
        if let Some(provider) = self.providers.get(provider_id) {
            provider.list_models().await
        } else {
            Err(Error::ProviderNotFound(provider_id.to_string()))
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        let mut registry = Self::new();

        // Register OpenAI if API key is available
        use crate::providers::openai::{OpenAIConfig, OpenAIProvider};
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY")
            && !api_key.is_empty()
            && let Ok(provider) = OpenAIProvider::new(OpenAIConfig::new(api_key))
        {
            registry = registry.register("openai", provider);
        }

        // Register Anthropic if API key is available
        use crate::providers::anthropic::{AnthropicConfig, AnthropicProvider};
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY")
            && !api_key.is_empty()
            && let Ok(provider) = AnthropicProvider::new(AnthropicConfig::new(api_key))
        {
            registry = registry.register("anthropic", provider);
        }

        // Register Gemini if API key is available
        use crate::providers::gemini::{GeminiConfig, GeminiProvider};
        if let Ok(api_key) = std::env::var("GEMINI_API_KEY")
            && !api_key.is_empty()
            && let Ok(provider) = GeminiProvider::new(GeminiConfig::new(api_key))
        {
            registry = registry.register("google", provider);
        }

        registry
    }
}
