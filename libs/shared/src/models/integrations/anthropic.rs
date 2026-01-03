//! Anthropic provider configuration and model definitions
//!
//! This module contains configuration types and model enums for Anthropic.
//! Request/response types for API communication are in `libs/ai/src/providers/anthropic/`.

use crate::models::model_pricing::{ContextAware, ContextPricingTier, ModelContextInfo};
use serde::{Deserialize, Serialize};

/// Anthropic model identifiers
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum AnthropicModel {
    #[serde(rename = "claude-haiku-4-5-20251001")]
    Claude45Haiku,
    #[serde(rename = "claude-sonnet-4-5-20250929")]
    Claude45Sonnet,
    #[serde(rename = "claude-opus-4-5-20251101")]
    Claude45Opus,
}

impl std::fmt::Display for AnthropicModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnthropicModel::Claude45Haiku => write!(f, "claude-haiku-4-5-20251001"),
            AnthropicModel::Claude45Sonnet => write!(f, "claude-sonnet-4-5-20250929"),
            AnthropicModel::Claude45Opus => write!(f, "claude-opus-4-5-20251101"),
        }
    }
}

impl AnthropicModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize Anthropic model".to_string())
    }

    /// Default smart model for Anthropic
    pub const DEFAULT_SMART_MODEL: AnthropicModel = AnthropicModel::Claude45Opus;

    /// Default eco model for Anthropic
    pub const DEFAULT_ECO_MODEL: AnthropicModel = AnthropicModel::Claude45Haiku;

    /// Default recovery model for Anthropic
    pub const DEFAULT_RECOVERY_MODEL: AnthropicModel = AnthropicModel::Claude45Haiku;

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

impl ContextAware for AnthropicModel {
    fn context_info(&self) -> ModelContextInfo {
        let model_name = self.to_string();

        if model_name.starts_with("claude-haiku") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 1.0,
                    output_cost_per_million: 5.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("claude-sonnet") {
            return ModelContextInfo {
                max_tokens: 1_000_000,
                pricing_tiers: vec![
                    ContextPricingTier {
                        label: "<200K tokens".to_string(),
                        input_cost_per_million: 3.0,
                        output_cost_per_million: 15.0,
                        upper_bound: Some(200_000),
                    },
                    ContextPricingTier {
                        label: ">200K tokens".to_string(),
                        input_cost_per_million: 6.0,
                        output_cost_per_million: 22.5,
                        upper_bound: None,
                    },
                ],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("claude-opus") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 5.0,
                    output_cost_per_million: 25.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        panic!("Unknown model: {}", model_name);
    }

    fn model_name(&self) -> String {
        match self {
            AnthropicModel::Claude45Sonnet => "Claude Sonnet 4.5".to_string(),
            AnthropicModel::Claude45Haiku => "Claude Haiku 4.5".to_string(),
            AnthropicModel::Claude45Opus => "Claude Opus 4.5".to_string(),
        }
    }
}

/// Configuration for Anthropic provider
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct AnthropicConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
    /// OAuth access token (takes precedence over api_key when set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

impl AnthropicConfig {
    /// Create config with API key
    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            api_endpoint: None,
            access_token: None,
        }
    }

    /// Create config with OAuth access token
    pub fn with_access_token(access_token: impl Into<String>) -> Self {
        Self {
            access_token: Some(access_token.into()),
            api_endpoint: None,
            api_key: None,
        }
    }

    /// Create config from ProviderAuth
    pub fn from_provider_auth(auth: &crate::models::auth::ProviderAuth) -> Self {
        match auth {
            crate::models::auth::ProviderAuth::Api { key } => Self::with_api_key(key),
            crate::models::auth::ProviderAuth::OAuth { access, .. } => {
                Self::with_access_token(access)
            }
        }
    }

    /// Get the effective credential (OAuth token takes precedence)
    pub fn effective_credential(&self) -> Option<&str> {
        self.access_token.as_deref().or(self.api_key.as_deref())
    }

    /// Check if using OAuth
    pub fn is_oauth(&self) -> bool {
        self.access_token.is_some()
    }

    /// Merge with credentials from ProviderAuth, preserving existing endpoint
    pub fn with_provider_auth(mut self, auth: &crate::models::auth::ProviderAuth) -> Self {
        match auth {
            crate::models::auth::ProviderAuth::Api { key } => {
                self.api_key = Some(key.clone());
                self.access_token = None;
            }
            crate::models::auth::ProviderAuth::OAuth { access, .. } => {
                self.access_token = Some(access.clone());
                self.api_key = None;
            }
        }
        self
    }
}
