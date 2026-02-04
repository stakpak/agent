//! OpenAI model definitions
//!
//! Static model definitions for OpenAI's GPT and reasoning models with pricing and limits.

use crate::types::{Model, ModelCost, ModelLimit};

/// Provider identifier for OpenAI
pub const PROVIDER_ID: &str = "openai";

/// Get all OpenAI models
pub fn models() -> Vec<Model> {
    vec![
        gpt_5(),
        gpt_5_1(),
        gpt_5_mini(),
        gpt_5_nano(),
        o3(),
        o4_mini(),
    ]
}

/// Get an OpenAI model by ID
/// Get an OpenAI model by ID
/// Supports both exact match and prefix match (e.g., "gpt-5" matches "gpt-5-2025-08-07")
pub fn get_model(id: &str) -> Option<Model> {
    models()
        .into_iter()
        .find(|m| m.id == id || m.id.starts_with(&format!("{}-", id)))
}

/// Get the default model for OpenAI
pub fn default_model() -> Model {
    gpt_5()
}

/// GPT-5 - Main flagship model
pub fn gpt_5() -> Model {
    Model {
        id: "gpt-5-2025-08-07".into(),
        name: "GPT-5".into(),
        provider: PROVIDER_ID.into(),
        reasoning: false,
        cost: Some(ModelCost {
            input: 1.25,
            output: 10.0,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 400_000,
            output: 16_384,
        },
    }
}

/// GPT-5.1 - Updated flagship model
pub fn gpt_5_1() -> Model {
    Model {
        id: "gpt-5.1-2025-11-13".into(),
        name: "GPT-5.1".into(),
        provider: PROVIDER_ID.into(),
        reasoning: false,
        cost: Some(ModelCost {
            input: 1.50,
            output: 12.0,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 400_000,
            output: 16_384,
        },
    }
}

/// GPT-5 Mini - Smaller, faster model
pub fn gpt_5_mini() -> Model {
    Model {
        id: "gpt-5-mini-2025-08-07".into(),
        name: "GPT-5 Mini".into(),
        provider: PROVIDER_ID.into(),
        reasoning: false,
        cost: Some(ModelCost {
            input: 0.25,
            output: 2.0,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 400_000,
            output: 16_384,
        },
    }
}

/// GPT-5 Nano - Smallest and fastest
pub fn gpt_5_nano() -> Model {
    Model {
        id: "gpt-5-nano-2025-08-07".into(),
        name: "GPT-5 Nano".into(),
        provider: PROVIDER_ID.into(),
        reasoning: false,
        cost: Some(ModelCost {
            input: 0.05,
            output: 0.40,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 400_000,
            output: 16_384,
        },
    }
}

/// O3 - Advanced reasoning model
pub fn o3() -> Model {
    Model {
        id: "o3-2025-04-16".into(),
        name: "O3".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 2.0,
            output: 8.0,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 200_000,
            output: 100_000,
        },
    }
}

/// O4 Mini - Smaller reasoning model
pub fn o4_mini() -> Model {
    Model {
        id: "o4-mini-2025-04-16".into(),
        name: "O4 Mini".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 1.10,
            output: 4.40,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 200_000,
            output: 100_000,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_models_list() {
        let all_models = models();
        assert_eq!(all_models.len(), 6);

        // Verify all models have the openai provider
        for model in &all_models {
            assert_eq!(model.provider, PROVIDER_ID);
        }
    }

    #[test]
    fn test_get_model() {
        let gpt5 = get_model("gpt-5-2025-08-07");
        assert!(gpt5.is_some());
        assert_eq!(gpt5.unwrap().name, "GPT-5");

        let nonexistent = get_model("nonexistent-model");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_default_model() {
        let default = default_model();
        assert_eq!(default.id, "gpt-5-2025-08-07");
    }

    #[test]
    fn test_reasoning_models() {
        let gpt5 = gpt_5();
        let o3_model = o3();
        let o4_model = o4_mini();

        assert!(!gpt5.reasoning);
        assert!(o3_model.reasoning);
        assert!(o4_model.reasoning);
    }
}
