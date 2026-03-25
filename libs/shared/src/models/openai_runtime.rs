use crate::models::auth::ProviderAuth;
use crate::models::integrations::openai::OpenAIConfig as InputOpenAIConfig;
use crate::models::llm::ProviderConfig;
pub use stakai::providers::openai::runtime::{
    CodexBackendProfile, CompatibleBackendProfile, OfficialBackendProfile, OpenAIBackendProfile,
};
use stakai::types::{CompletionsConfig, OpenAIApiConfig, OpenAIOptions, ResponsesConfig};
use thiserror::Error;

const OFFICIAL_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAIResolvedAuth {
    ApiKey {
        key: String,
    },
    OAuthBearer {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
    },
}

impl OpenAIResolvedAuth {
    pub fn authorization_token(&self) -> &str {
        match self {
            Self::ApiKey { key } => key,
            Self::OAuthBearer { access_token, .. } => access_token,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenAIResolvedConfig {
    pub auth: OpenAIResolvedAuth,
    pub backend: OpenAIBackendProfile,
    pub default_api_mode: OpenAIApiConfig,
}

impl OpenAIResolvedConfig {
    pub fn to_stakai_config(&self) -> stakai::providers::openai::OpenAIConfig {
        let mut config = stakai::providers::openai::OpenAIConfig::new(
            self.auth.authorization_token().to_string(),
        );

        match &self.backend {
            OpenAIBackendProfile::Official(profile) => {
                if profile.base_url != OFFICIAL_OPENAI_BASE_URL {
                    config = config.with_base_url(profile.base_url.clone());
                }
            }
            OpenAIBackendProfile::Compatible(profile) => {
                config = config.with_base_url(profile.base_url.clone());
            }
            OpenAIBackendProfile::Codex(profile) => {
                config = config
                    .with_base_url(profile.base_url.clone())
                    .with_custom_header("originator", profile.originator.clone())
                    .with_custom_header("ChatGPT-Account-Id", profile.chatgpt_account_id.clone());
            }
        }

        match self.default_api_mode {
            OpenAIApiConfig::Responses(_) => {
                config.with_default_openai_options(OpenAIOptions::responses())
            }
            OpenAIApiConfig::Completions(_) => config,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenAIBackendResolutionInput {
    provider_config: Option<ProviderConfig>,
    auth: Option<ProviderAuth>,
}

impl OpenAIBackendResolutionInput {
    pub fn new(provider_config: Option<ProviderConfig>, auth: Option<ProviderAuth>) -> Self {
        Self {
            provider_config,
            auth,
        }
    }

    fn provider_fields(&self) -> Result<OpenAIProviderFields, OpenAIResolutionError> {
        match self.provider_config.as_ref() {
            None => Ok(OpenAIProviderFields::default()),
            Some(ProviderConfig::OpenAI { api_endpoint, .. }) => Ok(OpenAIProviderFields {
                api_endpoint: api_endpoint.clone(),
            }),
            Some(other) => Err(OpenAIResolutionError::UnsupportedProviderConfig(
                other.provider_type().to_string(),
            )),
        }
    }
}

#[derive(Debug, Default, Clone)]
struct OpenAIProviderFields {
    api_endpoint: Option<String>,
}

#[derive(Debug, Error)]
pub enum OpenAIResolutionError {
    #[error("OpenAI runtime resolution only supports openai provider config, got {0}")]
    UnsupportedProviderConfig(String),
    #[error("ChatGPT Plus/Pro OAuth credentials are missing required chatgpt_account_id claim")]
    MissingCodexAccountId,
}

pub fn resolve_openai_runtime(
    input: OpenAIBackendResolutionInput,
) -> Result<Option<OpenAIResolvedConfig>, OpenAIResolutionError> {
    let provider_fields = input.provider_fields()?;
    let Some(auth) = input.auth else {
        return Ok(None);
    };

    match auth {
        ProviderAuth::Api { key } => {
            let base_url = provider_fields
                .api_endpoint
                .unwrap_or_else(|| OFFICIAL_OPENAI_BASE_URL.to_string());
            let (backend, default_api_mode) = if base_url == OFFICIAL_OPENAI_BASE_URL {
                (
                    OpenAIBackendProfile::Official(OfficialBackendProfile { base_url }),
                    OpenAIApiConfig::Responses(ResponsesConfig::default()),
                )
            } else {
                (
                    OpenAIBackendProfile::Compatible(CompatibleBackendProfile { base_url }),
                    OpenAIApiConfig::Completions(CompletionsConfig::default()),
                )
            };

            Ok(Some(OpenAIResolvedConfig {
                auth: OpenAIResolvedAuth::ApiKey { key },
                backend,
                default_api_mode,
            }))
        }
        ProviderAuth::OAuth {
            access,
            refresh,
            expires,
            ..
        } => {
            let Some(chatgpt_account_id) = InputOpenAIConfig::extract_chatgpt_account_id(&access)
            else {
                return Err(OpenAIResolutionError::MissingCodexAccountId);
            };

            Ok(Some(OpenAIResolvedConfig {
                auth: OpenAIResolvedAuth::OAuthBearer {
                    access_token: access,
                    refresh_token: if refresh.is_empty() {
                        None
                    } else {
                        Some(refresh)
                    },
                    expires_at: Some(expires),
                },
                backend: OpenAIBackendProfile::Codex(CodexBackendProfile {
                    base_url: provider_fields
                        .api_endpoint
                        .unwrap_or_else(|| InputOpenAIConfig::OPENAI_CODEX_BASE_URL.to_string()),
                    originator: "stakpak".to_string(),
                    chatgpt_account_id,
                }),
                default_api_mode: OpenAIApiConfig::Responses(ResponsesConfig::default()),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn test_to_stakai_config_for_codex_oauth() {
        let resolved = OpenAIResolvedConfig {
            auth: OpenAIResolvedAuth::OAuthBearer {
                access_token: "access-token".to_string(),
                refresh_token: Some("refresh-token".to_string()),
                expires_at: Some(123),
            },
            backend: OpenAIBackendProfile::Codex(CodexBackendProfile {
                base_url: InputOpenAIConfig::OPENAI_CODEX_BASE_URL.to_string(),
                originator: "stakpak".to_string(),
                chatgpt_account_id: "acct_test_123".to_string(),
            }),
            default_api_mode: OpenAIApiConfig::Responses(ResponsesConfig::default()),
        };

        let config = resolved.to_stakai_config();

        assert_eq!(config.api_key, "access-token");
        assert_eq!(config.base_url, InputOpenAIConfig::OPENAI_CODEX_BASE_URL);
        assert_eq!(
            config.custom_headers.get("ChatGPT-Account-Id"),
            Some(&"acct_test_123".to_string())
        );
        assert_eq!(
            config.custom_headers.get("originator"),
            Some(&"stakpak".to_string())
        );
        assert!(matches!(
            config.default_openai_options,
            Some(OpenAIOptions {
                api_config: Some(OpenAIApiConfig::Responses(_)),
                ..
            })
        ));
    }

    #[test]
    fn test_resolve_openai_runtime_for_oauth_codex() {
        let payload = serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_test_789"
            }
        });
        let encoded_payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let access_token = format!("header.{}.signature", encoded_payload);

        let auth = ProviderAuth::oauth_with_name(
            access_token,
            "refresh-token",
            i64::MAX,
            "ChatGPT Plus/Pro",
        );
        let resolved = resolve_openai_runtime(OpenAIBackendResolutionInput::new(
            Some(ProviderConfig::openai_with_auth(auth.clone())),
            Some(auth),
        ));

        assert!(resolved.is_ok());
    }
}
