//! OpenAI provider configuration and model metadata
//!
//! Agent runtime messages and tool calls live in `stakai`.

use crate::models::model_pricing::{ContextAware, ContextPricingTier, ModelContextInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// =============================================================================
// Provider Configuration
// =============================================================================

/// Configuration for OpenAI provider
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OpenAIConfig {
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
}

impl OpenAIConfig {
    pub const OPENAI_CODEX_BASE_URL: &'static str = "https://chatgpt.com/backend-api/codex";
    const OPENAI_AUTH_CLAIM: &'static str = "https://api.openai.com/auth";

    /// Create config with API key
    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            api_endpoint: None,
        }
    }

    /// Decode an OpenAI access token and extract the ChatGPT account ID.
    ///
    /// This intentionally reads the JWT payload without signature verification.
    /// The claim is only used for request routing/header construction; OpenAI's
    /// servers still validate the bearer token on use.
    pub fn extract_chatgpt_account_id(access_token: &str) -> Option<String> {
        let claims = crate::jwt::decode_jwt_payload_unverified(access_token)?;
        let auth_claim = claims.get(Self::OPENAI_AUTH_CLAIM)?;

        match auth_claim {
            Value::Object(map) => map
                .get("chatgpt_account_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            Value::String(raw_json) => {
                serde_json::from_str::<Value>(raw_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("chatgpt_account_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })
            }
            _ => None,
        }
    }
}

// =============================================================================
// Model Definitions
// =============================================================================

/// OpenAI model identifiers
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum OpenAIModel {
    // Reasoning Models
    #[serde(rename = "o3-2025-04-16")]
    O3,
    #[serde(rename = "o4-mini-2025-04-16")]
    O4Mini,

    #[default]
    #[serde(rename = "gpt-5-2025-08-07")]
    GPT5,
    #[serde(rename = "gpt-5.1-2025-11-13")]
    GPT51,
    #[serde(rename = "gpt-5-mini-2025-08-07")]
    GPT5Mini,
    #[serde(rename = "gpt-5-nano-2025-08-07")]
    GPT5Nano,

    Custom(String),
}

impl OpenAIModel {
    pub fn from_string(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| "Failed to deserialize OpenAI model".to_string())
    }
}

impl ContextAware for OpenAIModel {
    fn context_info(&self) -> ModelContextInfo {
        let model_name = self.to_string();

        if model_name.starts_with("o3") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 2.0,
                    output_cost_per_million: 8.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("o4-mini") {
            return ModelContextInfo {
                max_tokens: 200_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 1.10,
                    output_cost_per_million: 4.40,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("gpt-5-mini") {
            return ModelContextInfo {
                max_tokens: 400_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.25,
                    output_cost_per_million: 2.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("gpt-5-nano") {
            return ModelContextInfo {
                max_tokens: 400_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 0.05,
                    output_cost_per_million: 0.40,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        if model_name.starts_with("gpt-5") {
            return ModelContextInfo {
                max_tokens: 400_000,
                pricing_tiers: vec![ContextPricingTier {
                    label: "Standard".to_string(),
                    input_cost_per_million: 1.25,
                    output_cost_per_million: 10.0,
                    upper_bound: None,
                }],
                approach_warning_threshold: 0.8,
            };
        }

        ModelContextInfo::default()
    }

    fn model_name(&self) -> String {
        match self {
            OpenAIModel::O3 => "O3".to_string(),
            OpenAIModel::O4Mini => "O4-mini".to_string(),
            OpenAIModel::GPT5 => "GPT-5".to_string(),
            OpenAIModel::GPT51 => "GPT-5.1".to_string(),
            OpenAIModel::GPT5Mini => "GPT-5 Mini".to_string(),
            OpenAIModel::GPT5Nano => "GPT-5 Nano".to_string(),
            OpenAIModel::Custom(name) => format!("Custom ({})", name),
        }
    }
}

impl std::fmt::Display for OpenAIModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenAIModel::O3 => write!(f, "o3-2025-04-16"),
            OpenAIModel::O4Mini => write!(f, "o4-mini-2025-04-16"),
            OpenAIModel::GPT5Nano => write!(f, "gpt-5-nano-2025-08-07"),
            OpenAIModel::GPT5Mini => write!(f, "gpt-5-mini-2025-08-07"),
            OpenAIModel::GPT5 => write!(f, "gpt-5-2025-08-07"),
            OpenAIModel::GPT51 => write!(f, "gpt-5.1-2025-11-13"),
            OpenAIModel::Custom(model_name) => write!(f, "{}", model_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_chatgpt_account_id_from_access_token() {
        use base64::Engine;

        let claim = json!({
            "chatgpt_account_id": "acct_test_123"
        });
        let payload = json!({
            "https://api.openai.com/auth": claim
        });
        let encoded_payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let access_token = format!("header.{}.signature", encoded_payload);

        assert_eq!(
            OpenAIConfig::extract_chatgpt_account_id(&access_token),
            Some("acct_test_123".to_string())
        );
    }

    #[test]
    fn test_extract_chatgpt_account_id_returns_none_for_missing_claim() {
        use base64::Engine;

        let payload = json!({
            "sub": "user_123"
        });
        let encoded_payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let access_token = format!("header.{}.signature", encoded_payload);

        assert_eq!(
            OpenAIConfig::extract_chatgpt_account_id(&access_token),
            None
        );
    }

    #[test]
    fn test_extract_chatgpt_account_id_returns_none_for_invalid_token_shape() {
        assert_eq!(OpenAIConfig::extract_chatgpt_account_id("not-a-jwt"), None);
    }

    #[test]
    fn test_extract_chatgpt_account_id_returns_none_for_invalid_claim_json() {
        use base64::Engine;

        let payload = json!({
            "https://api.openai.com/auth": "{not-json}"
        });
        let encoded_payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let access_token = format!("header.{}.signature", encoded_payload);

        assert_eq!(
            OpenAIConfig::extract_chatgpt_account_id(&access_token),
            None
        );
    }
}
