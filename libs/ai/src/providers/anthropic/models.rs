//! Anthropic model definitions
//!
//! Static model definitions for Anthropic's Claude models with pricing and limits.

use crate::types::{Model, ModelCost, ModelLimit};

/// Provider identifier for Anthropic
pub const PROVIDER_ID: &str = "anthropic";

/// Get all Anthropic models
pub fn models() -> Vec<Model> {
    vec![claude_haiku_4_5(), claude_sonnet_4_5(), claude_opus_4_5()]
}

/// Get an Anthropic model by ID
pub fn get_model(id: &str) -> Option<Model> {
    models().into_iter().find(|m| m.id == id)
}

/// Get the default model for Anthropic
pub fn default_model() -> Model {
    claude_sonnet_4_5()
}

/// Claude Haiku 4.5 - Fast and affordable
pub fn claude_haiku_4_5() -> Model {
    Model {
        id: "claude-haiku-4-5-20251001".into(),
        name: "Claude Haiku 4.5".into(),
        provider: PROVIDER_ID.into(),
        reasoning: false,
        cost: Some(ModelCost {
            input: 1.0,
            output: 5.0,
            cache_read: Some(0.10),
            cache_write: Some(1.25),
        }),
        limit: ModelLimit {
            context: 200_000,
            output: 8_192,
        },
    }
}

/// Claude Sonnet 4.5 - Balanced performance
pub fn claude_sonnet_4_5() -> Model {
    Model {
        id: "claude-sonnet-4-5-20250929".into(),
        name: "Claude Sonnet 4.5".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.30),
            cache_write: Some(3.75),
        }),
        limit: ModelLimit {
            context: 200_000,
            output: 16_384,
        },
    }
}

/// Claude Opus 4.5 - Most capable
pub fn claude_opus_4_5() -> Model {
    Model {
        id: "claude-opus-4-5-20251101".into(),
        name: "Claude Opus 4.5".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 15.0,
            output: 75.0,
            cache_read: Some(1.50),
            cache_write: Some(18.75),
        }),
        limit: ModelLimit {
            context: 200_000,
            output: 32_000,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_models_list() {
        let all_models = models();
        assert_eq!(all_models.len(), 3);

        // Verify all models have the anthropic provider
        for model in &all_models {
            assert_eq!(model.provider, PROVIDER_ID);
        }
    }

    #[test]
    fn test_get_model() {
        let sonnet = get_model("claude-sonnet-4-5-20250929");
        assert!(sonnet.is_some());
        assert_eq!(sonnet.unwrap().name, "Claude Sonnet 4.5");

        let nonexistent = get_model("nonexistent-model");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_default_model() {
        let default = default_model();
        assert_eq!(default.id, "claude-sonnet-4-5-20250929");
    }

    #[test]
    fn test_model_pricing() {
        let sonnet = claude_sonnet_4_5();
        let cost = sonnet.cost.unwrap();

        assert_eq!(cost.input, 3.0);
        assert_eq!(cost.output, 15.0);
        assert_eq!(cost.cache_read, Some(0.30));
        assert_eq!(cost.cache_write, Some(3.75));
    }

    #[test]
    fn test_reasoning_support() {
        let haiku = claude_haiku_4_5();
        let sonnet = claude_sonnet_4_5();
        let opus = claude_opus_4_5();

        assert!(!haiku.reasoning);
        assert!(sonnet.reasoning);
        assert!(opus.reasoning);
    }
}
