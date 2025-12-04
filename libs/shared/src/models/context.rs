use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextPricingTier {
    pub label: String,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub upper_bound: Option<u64>, // None means infinite/rest
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelContextInfo {
    pub max_tokens: u64,
    pub pricing_tiers: Vec<ContextPricingTier>,
    pub approach_warning_threshold: f64, // e.g. 0.8 for 80%
}

impl Default for ModelContextInfo {
    fn default() -> Self {
        Self {
            max_tokens: 128_000,
            pricing_tiers: vec![],
            approach_warning_threshold: 0.8,
        }
    }
}

pub trait ContextAware {
    /// Returns context information for the model
    fn context_info(&self) -> ModelContextInfo;

    /// Returns the display name of the model
    fn model_name(&self) -> String;
}
