//! Gemini model definitions
//!
//! Static model definitions for Google's Gemini models with pricing and limits.

use crate::types::{Model, ModelCost, ModelLimit};

/// Provider identifier for Gemini (Google)
pub const PROVIDER_ID: &str = "google";

/// Get all Gemini models
pub fn models() -> Vec<Model> {
    vec![
        gemini_3_pro(),
        gemini_3_flash(),
        gemini_2_5_pro(),
        gemini_2_5_flash(),
        gemini_2_5_flash_lite(),
    ]
}

/// Get a Gemini model by ID
pub fn get_model(id: &str) -> Option<Model> {
    models().into_iter().find(|m| m.id == id)
}

/// Get the default model for Gemini
pub fn default_model() -> Model {
    gemini_3_pro()
}

/// Gemini 3 Pro - Latest flagship model
pub fn gemini_3_pro() -> Model {
    Model {
        id: "gemini-3-pro-preview".into(),
        name: "Gemini 3 Pro".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 2.0,
            output: 12.0,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 1_000_000,
            output: 65_536,
        },
    }
}

/// Gemini 3 Flash - Fast latest generation
pub fn gemini_3_flash() -> Model {
    Model {
        id: "gemini-3-flash-preview".into(),
        name: "Gemini 3 Flash".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 0.50,
            output: 3.0,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 1_000_000,
            output: 65_536,
        },
    }
}

/// Gemini 2.5 Pro - Powerful multimodal model
pub fn gemini_2_5_pro() -> Model {
    Model {
        id: "gemini-2.5-pro".into(),
        name: "Gemini 2.5 Pro".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 1.25,
            output: 10.0,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 1_000_000,
            output: 65_536,
        },
    }
}

/// Gemini 2.5 Flash - Fast and efficient
pub fn gemini_2_5_flash() -> Model {
    Model {
        id: "gemini-2.5-flash".into(),
        name: "Gemini 2.5 Flash".into(),
        provider: PROVIDER_ID.into(),
        reasoning: true,
        cost: Some(ModelCost {
            input: 0.30,
            output: 2.50,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 1_000_000,
            output: 65_536,
        },
    }
}

/// Gemini 2.5 Flash Lite - Smallest and most affordable
pub fn gemini_2_5_flash_lite() -> Model {
    Model {
        id: "gemini-2.5-flash-lite".into(),
        name: "Gemini 2.5 Flash Lite".into(),
        provider: PROVIDER_ID.into(),
        reasoning: false,
        cost: Some(ModelCost {
            input: 0.10,
            output: 0.40,
            cache_read: None,
            cache_write: None,
        }),
        limit: ModelLimit {
            context: 1_000_000,
            output: 65_536,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_models_list() {
        let all_models = models();
        assert_eq!(all_models.len(), 5);

        // Verify all models have the google provider
        for model in &all_models {
            assert_eq!(model.provider, PROVIDER_ID);
        }
    }

    #[test]
    fn test_get_model() {
        let pro = get_model("gemini-3-pro-preview");
        assert!(pro.is_some());
        assert_eq!(pro.unwrap().name, "Gemini 3 Pro");

        let nonexistent = get_model("nonexistent-model");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_default_model() {
        let default = default_model();
        assert_eq!(default.id, "gemini-3-pro-preview");
    }

    #[test]
    fn test_large_context_windows() {
        // All Gemini models have 1M token context
        for model in models() {
            assert_eq!(model.limit.context, 1_000_000);
        }
    }

    #[test]
    fn test_reasoning_support() {
        let flash_lite = gemini_2_5_flash_lite();
        let pro = gemini_3_pro();

        assert!(!flash_lite.reasoning);
        assert!(pro.reasoning);
    }
}
