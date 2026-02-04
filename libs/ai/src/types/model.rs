//! Unified model types for all AI providers
//!
//! This module provides a single `Model` struct that replaces provider-specific
//! model enums (AnthropicModel, OpenAIModel, GeminiModel) and related types.

use serde::{Deserialize, Serialize};

/// Unified model representation across all providers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Model {
    /// Model identifier sent to the API (e.g., "claude-sonnet-4-5-20250929")
    pub id: String,
    /// Human-readable name (e.g., "Claude Sonnet 4.5")
    pub name: String,
    /// Provider identifier (e.g., "anthropic", "openai", "google")
    pub provider: String,
    /// Extended thinking/reasoning support
    pub reasoning: bool,
    /// Pricing per 1M tokens (None for custom/unknown models)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelCost>,
    /// Token limits
    pub limit: ModelLimit,
    /// Release date (YYYY-MM-DD format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
}

impl Model {
    /// Create a new model with all fields
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        provider: impl Into<String>,
        reasoning: bool,
        cost: Option<ModelCost>,
        limit: ModelLimit,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            provider: provider.into(),
            reasoning,
            cost,
            limit,
            release_date: None,
        }
    }

    /// Create a custom model with minimal info (no pricing)
    pub fn custom(id: impl Into<String>, provider: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            provider: provider.into(),
            reasoning: false,
            cost: None,
            limit: ModelLimit::default(),
            release_date: None,
        }
    }

    /// Check if this model has pricing information
    pub fn has_pricing(&self) -> bool {
        self.cost.is_some()
    }

    /// Get the display name (name field)
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Get the model ID used for API calls
    pub fn model_id(&self) -> &str {
        &self.id
    }

    /// Get the provider name
    pub fn provider_name(&self) -> &str {
        &self.provider
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Pricing information per 1M tokens
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelCost {
    /// Cost per 1M input tokens
    pub input: f64,
    /// Cost per 1M output tokens
    pub output: f64,
    /// Cost per 1M cached input tokens (if supported)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    /// Cost per 1M tokens written to cache (if supported)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

impl ModelCost {
    /// Create a new cost struct with basic input/output pricing
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_read: None,
            cache_write: None,
        }
    }

    /// Create a cost struct with cache pricing
    pub fn with_cache(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Self {
        Self {
            input,
            output,
            cache_read: Some(cache_read),
            cache_write: Some(cache_write),
        }
    }

    /// Calculate cost for given token counts (in tokens, not millions)
    pub fn calculate(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output;
        input_cost + output_cost
    }

    /// Calculate cost with cache tokens
    pub fn calculate_with_cache(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> f64 {
        let base_cost = self.calculate(input_tokens, output_tokens);
        let cache_read_cost = self
            .cache_read
            .map(|rate| (cache_read_tokens as f64 / 1_000_000.0) * rate)
            .unwrap_or(0.0);
        let cache_write_cost = self
            .cache_write
            .map(|rate| (cache_write_tokens as f64 / 1_000_000.0) * rate)
            .unwrap_or(0.0);
        base_cost + cache_read_cost + cache_write_cost
    }
}

/// Token limits for the model
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelLimit {
    /// Maximum context window size in tokens
    pub context: u64,
    /// Maximum output tokens
    pub output: u64,
}

impl ModelLimit {
    /// Create a new limit struct
    pub fn new(context: u64, output: u64) -> Self {
        Self { context, output }
    }
}

impl Default for ModelLimit {
    fn default() -> Self {
        Self {
            context: 128_000,
            output: 8_192,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_creation() {
        let model = Model::new(
            "claude-sonnet-4-5-20250929",
            "Claude Sonnet 4.5",
            "anthropic",
            true,
            Some(ModelCost::with_cache(3.0, 15.0, 0.30, 3.75)),
            ModelLimit::new(200_000, 16_384),
        );

        assert_eq!(model.id, "claude-sonnet-4-5-20250929");
        assert_eq!(model.name, "Claude Sonnet 4.5");
        assert_eq!(model.provider, "anthropic");
        assert!(model.reasoning);
        assert!(model.has_pricing());
    }

    #[test]
    fn test_custom_model() {
        let model = Model::custom("llama3", "ollama");

        assert_eq!(model.id, "llama3");
        assert_eq!(model.name, "llama3");
        assert_eq!(model.provider, "ollama");
        assert!(!model.reasoning);
        assert!(!model.has_pricing());
    }

    #[test]
    fn test_cost_calculation() {
        let cost = ModelCost::new(3.0, 15.0);

        // 1000 input tokens, 500 output tokens
        let total = cost.calculate(1000, 500);
        // (1000/1M) * 3.0 + (500/1M) * 15.0 = 0.003 + 0.0075 = 0.0105
        assert!((total - 0.0105).abs() < 0.0001);
    }

    #[test]
    fn test_cost_with_cache() {
        let cost = ModelCost::with_cache(3.0, 15.0, 0.30, 3.75);

        let total = cost.calculate_with_cache(1000, 500, 2000, 1000);
        // base: 0.0105
        // cache_read: (2000/1M) * 0.30 = 0.0006
        // cache_write: (1000/1M) * 3.75 = 0.00375
        // total: 0.0105 + 0.0006 + 0.00375 = 0.01485
        assert!((total - 0.01485).abs() < 0.0001);
    }

    #[test]
    fn test_model_display() {
        let model = Model::new(
            "gpt-5",
            "GPT-5",
            "openai",
            false,
            None,
            ModelLimit::default(),
        );

        assert_eq!(format!("{}", model), "GPT-5");
    }

    #[test]
    fn test_serialization() {
        let model = Model::new(
            "claude-sonnet-4-5-20250929",
            "Claude Sonnet 4.5",
            "anthropic",
            true,
            Some(ModelCost::new(3.0, 15.0)),
            ModelLimit::new(200_000, 16_384),
        );

        let json = serde_json::to_string(&model).unwrap();
        assert!(json.contains("\"id\":\"claude-sonnet-4-5-20250929\""));
        assert!(json.contains("\"provider\":\"anthropic\""));

        let deserialized: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(model, deserialized);
    }
}
