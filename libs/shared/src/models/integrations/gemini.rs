//! Gemini provider configuration and model definitions
//!
//! This module contains configuration types and model enums for Google Gemini.
//! Request/response types for API communication are in `libs/ai/src/providers/gemini/`.

use crate::models::model_pricing::{ContextAware, ContextPricingTier, ModelContextInfo};
use serde::{Deserialize, Serialize};

/// Configuration for Gemini provider
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct GeminiConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
}

impl GeminiConfig {
    /// Create config with API key
    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            api_endpoint: None,
        }
    }

    /// Create config from ProviderAuth (only supports API key for Gemini)
    pub fn from_provider_auth(auth: &crate::models::auth::ProviderAuth) -> Option<Self> {
        match auth {
            crate::models::auth::ProviderAuth::Api { key } => Some(Self::with_api_key(key)),
            crate::models::auth::ProviderAuth::OAuth { .. } => None, // Gemini doesn't support OAuth in this impl
        }
    }

    /// Merge with credentials from ProviderAuth, preserving existing endpoint
    pub fn with_provider_auth(mut self, auth: &crate::models::auth::ProviderAuth) -> Option<Self> {
        match auth {
            crate::models::auth::ProviderAuth::Api { key } => {
                self.api_key = Some(key.clone());
                Some(self)
            }
            crate::models::auth::ProviderAuth::OAuth { .. } => None,
        }
    }
}

/// Gemini model identifiers
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum GeminiModel {
    #[default]
    #[serde(rename = "gemini-3-pro-preview")]
    Gemini3Pro,
    #[serde(rename = "gemini-3-flash-preview")]
    Gemini3Flash,
    #[serde(rename = "gemini-2.5-pro")]
    Gemini25Pro,
    #[serde(rename = "gemini-2.5-flash")]
    Gemini25Flash,
    #[serde(rename = "gemini-2.5-flash-lite")]
    Gemini25FlashLite,
}

impl std::fmt::Display for GeminiModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeminiModel::Gemini3Pro => write!(f, "gemini-3-pro-preview"),
            GeminiModel::Gemini3Flash => write!(f, "gemini-3-flash-preview"),
            GeminiModel::Gemini25Pro => write!(f, "gemini-2.5-pro"),
            GeminiModel::Gemini25Flash => write!(f, "gemini-2.5-flash"),
            GeminiModel::Gemini25FlashLite => write!(f, "gemini-2.5-flash-lite"),
        }
    }
}

impl GeminiModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize Gemini model".to_string())
    }

    /// Default smart model for Gemini
    pub const DEFAULT_SMART_MODEL: GeminiModel = GeminiModel::Gemini3Pro;

    /// Default eco model for Gemini
    pub const DEFAULT_ECO_MODEL: GeminiModel = GeminiModel::Gemini3Flash;

    /// Default recovery model for Gemini
    pub const DEFAULT_RECOVERY_MODEL: GeminiModel = GeminiModel::Gemini3Flash;

    /// Get default smart model as string
    pub fn default_smart_model() -> String {
        Self::DEFAULT_SMART_MODEL.to_string()
    }

    /// Get default eco model as string
    pub fn default_eco_model() -> String {
        Self::DEFAULT_ECO_MODEL.to_string()
    }

    /// Get default recovery model as string
    pub fn default_recovery_model() -> String {
        Self::DEFAULT_RECOVERY_MODEL.to_string()
    }
}

impl ContextAware for GeminiModel {
    fn context_info(&self) -> ModelContextInfo {
        match self {
            GeminiModel::Gemini3Pro => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![
                    ContextPricingTier {
                        label: "<200k tokens".to_string(),
                        input_cost_per_million: 2.0,
                        output_cost_per_million: 12.0,
                        upper_bound: Some(200_000),
                    },
                    ContextPricingTier {
                        label: ">200k tokens".to_string(),
                        input_cost_per_million: 4.0,
                        output_cost_per_million: 18.0,
                        upper_bound: None,
                    },
                ],
                approach_warning_threshold: 0.8,
            },
            GeminiModel::Gemini25Pro => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![
                    ContextPricingTier {
                        label: "<200k tokens".to_string(),
                        input_cost_per_million: 1.25,
                        output_cost_per_million: 10.0,
                        upper_bound: Some(200_000),
                    },
                    ContextPricingTier {
                        label: ">200k tokens".to_string(),
                        input_cost_per_million: 2.50,
                        output_cost_per_million: 15.0,
                        upper_bound: None,
                    },
                ],
                approach_warning_threshold: 0.8,
            },
            GeminiModel::Gemini25Flash => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.30,
                    output_cost_per_million: 2.50,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            },
            GeminiModel::Gemini3Flash => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.50,
                    output_cost_per_million: 3.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            },
            GeminiModel::Gemini25FlashLite => ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.1,
                    output_cost_per_million: 0.4,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            },
        }
    }

    fn model_name(&self) -> String {
        match self {
            GeminiModel::Gemini3Pro => "Gemini 3 Pro".to_string(),
            GeminiModel::Gemini3Flash => "Gemini 3 Flash".to_string(),
            GeminiModel::Gemini25Pro => "Gemini 2.5 Pro".to_string(),
            GeminiModel::Gemini25Flash => "Gemini 2.5 Flash".to_string(),
            GeminiModel::Gemini25FlashLite => "Gemini 2.5 Flash Lite".to_string(),
        }
    }
}
