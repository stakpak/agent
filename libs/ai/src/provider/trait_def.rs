//! Provider trait definition

use crate::error::Result;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers, Model};
use async_trait::async_trait;

/// Trait for AI provider implementations
#[async_trait]
pub trait Provider: Send + Sync {
    /// Provider identifier (e.g., "openai", "anthropic")
    fn provider_id(&self) -> &str;

    /// Build provider-specific headers (auth, version, etc.)
    /// Merges provider defaults with custom headers from request
    fn build_headers(&self, custom_headers: Option<&Headers>) -> Headers;

    /// Generate a response (non-streaming)
    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse>;

    /// Generate a streaming response
    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream>;

    /// List available models with full metadata
    async fn list_models(&self) -> Result<Vec<Model>> {
        Ok(vec![])
    }

    /// Get a specific model by ID (default implementation)
    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        Ok(self.list_models().await?.into_iter().find(|m| m.id == id))
    }
}
