//! OAuth provider registry

use super::provider::OAuthProvider;
use super::providers::AnthropicProvider;
use std::collections::HashMap;

/// Registry of OAuth providers
pub struct ProviderRegistry {
    providers: HashMap<&'static str, Box<dyn OAuthProvider>>,
}

impl ProviderRegistry {
    /// Create a new provider registry with built-in providers
    pub fn new() -> Self {
        let mut registry = Self {
            providers: HashMap::new(),
        };

        // Register built-in providers
        registry.register(Box::new(AnthropicProvider::new()));

        // Future providers:
        // registry.register(Box::new(GitHubCopilotProvider::new()));
        // registry.register(Box::new(OpenAIProvider::new()));

        registry
    }

    /// Register a new provider
    pub fn register(&mut self, provider: Box<dyn OAuthProvider>) {
        self.providers.insert(provider.id(), provider);
    }

    /// Get a provider by ID
    pub fn get(&self, id: &str) -> Option<&dyn OAuthProvider> {
        self.providers.get(id).map(|p| p.as_ref())
    }

    /// List all registered providers
    pub fn list(&self) -> Vec<&dyn OAuthProvider> {
        self.providers.values().map(|p| p.as_ref()).collect()
    }

    /// Get all provider IDs
    pub fn provider_ids(&self) -> Vec<&'static str> {
        self.providers.keys().copied().collect()
    }

    /// Check if a provider is registered
    pub fn has_provider(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let registry = ProviderRegistry::new();

        // Should have Anthropic provider by default
        assert!(registry.has_provider("anthropic"));
    }

    #[test]
    fn test_registry_get() {
        let registry = ProviderRegistry::new();

        let provider = registry.get("anthropic");
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().id(), "anthropic");
    }

    #[test]
    fn test_registry_get_unknown() {
        let registry = ProviderRegistry::new();
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_registry_list() {
        let registry = ProviderRegistry::new();
        let providers = registry.list();

        assert!(!providers.is_empty());
        assert!(providers.iter().any(|p| p.id() == "anthropic"));
    }

    #[test]
    fn test_registry_provider_ids() {
        let registry = ProviderRegistry::new();
        let ids = registry.provider_ids();

        assert!(ids.contains(&"anthropic"));
    }

    #[test]
    fn test_registry_has_provider() {
        let registry = ProviderRegistry::new();

        assert!(registry.has_provider("anthropic"));
        assert!(!registry.has_provider("unknown"));
    }
}
