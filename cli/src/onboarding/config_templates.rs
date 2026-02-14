//! Configuration templates for different provider setups
//!
//! # Provider Configuration
//!
//! Providers are configured in a `providers` HashMap where:
//! - The key becomes the model prefix for custom providers
//! - Built-in types: `openai`, `anthropic`, `gemini`
//! - Custom type: `custom` for OpenAI-compatible providers
//!
//! # Built-in Providers
//!
//! For built-in providers, you can use model names directly without a prefix:
//!
//! ```toml
//! [profiles.default]
//! smart_model = "claude-sonnet-4-5"  # auto-detected as anthropic
//! eco_model = "gpt-4-turbo"          # auto-detected as openai
//!
//! [profiles.default.providers.openai]
//! type = "openai"
//! api_key = "sk-..."  # or use auth.toml / OPENAI_API_KEY env var
//!
//! [profiles.default.providers.anthropic]
//! type = "anthropic"
//! api_key = "sk-ant-..."  # or use auth.toml / ANTHROPIC_API_KEY env var
//! ```
//!
//! # Custom Providers
//!
//! For OpenAI-compatible providers (Ollama, vLLM, etc.):
//!
//! ```toml
//! [profiles.default]
//! smart_model = "claude-sonnet-4-5"  # built-in, auto-detected
//! eco_model = "offline/llama3"       # custom provider
//!
//! [profiles.default.providers.offline]
//! type = "custom"
//! api_endpoint = "http://localhost:11434/v1"
//! # api_key is optional for local providers
//! ```
//!
//! # Model Routing
//!
//! - Built-in models: `claude-sonnet-4-5`, `gpt-4`, `gemini-2.5-pro` → auto-detected
//! - Custom providers: `offline/llama3` → routes to `offline` provider, sends `llama3` to API
//!
//! # Credential Resolution Order
//!
//! For built-in providers, credentials are resolved in order:
//! 1. `auth.toml` (profile-specific)
//! 2. `auth.toml` (shared/all)
//! 3. `config.toml` providers section
//! 4. Environment variable (e.g., `ANTHROPIC_API_KEY`)

use crate::config::ProfileConfig;
use crate::config::ProviderType;
use stakpak_shared::models::integrations::anthropic::AnthropicModel;
use stakpak_shared::models::integrations::gemini::GeminiModel;
use stakpak_shared::models::integrations::openai::OpenAIModel;
use stakpak_shared::models::llm::ProviderConfig;

/// Generate OpenAI profile configuration (credentials stored separately in auth.toml)
pub fn generate_openai_profile() -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(OpenAIModel::default_smart_model()),
        eco_model: Some(OpenAIModel::default_eco_model()),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        "openai".to_string(),
        ProviderConfig::OpenAI {
            api_key: None,
            api_endpoint: None,
        },
    );
    profile
}

/// Generate Gemini profile configuration (credentials stored separately in auth.toml)
pub fn generate_gemini_profile() -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(GeminiModel::default_smart_model()),
        eco_model: Some(GeminiModel::default_eco_model()),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        "gemini".to_string(),
        ProviderConfig::Gemini {
            api_key: None,
            api_endpoint: None,
        },
    );
    profile
}

/// Generate Anthropic profile configuration (credentials stored separately in auth.toml)
pub fn generate_anthropic_profile() -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(AnthropicModel::default_smart_model()),
        eco_model: Some(AnthropicModel::default_eco_model()),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        "anthropic".to_string(),
        ProviderConfig::Anthropic {
            api_key: None,
            api_endpoint: None,
            access_token: None,
        },
    );
    profile
}

/// Generate custom provider profile configuration
///
/// This creates a profile with a custom OpenAI-compatible provider (e.g., LiteLLM, Ollama).
/// Model names are automatically prefixed with the provider name.
///
/// # Arguments
/// * `provider_name` - Name of the provider (e.g., "litellm", "ollama") - becomes the model prefix
/// * `api_endpoint` - API endpoint URL as required by the provider (e.g., "http://localhost:4000")
/// * `api_key` - Optional API key (some providers like Ollama don't require auth)
/// * `smart_model` - Smart model name/path (e.g., "claude-opus" or "anthropic/claude-opus" for LiteLLM)
/// * `eco_model` - Eco model name/path (e.g., "claude-haiku" or "anthropic/claude-haiku")
///
/// # Example
/// For LiteLLM with Anthropic models:
/// ```ignore
/// generate_custom_provider_profile(
///     "litellm".to_string(),
///     "http://localhost:4000".to_string(),
///     Some("sk-litellm".to_string()),
///     "anthropic/claude-opus".to_string(),  // Will become "litellm/anthropic/claude-opus"
///     "anthropic/claude-haiku".to_string(), // Will become "litellm/anthropic/claude-haiku"
/// )
/// ```
pub fn generate_custom_provider_profile(
    provider_name: String,
    api_endpoint: String,
    api_key: Option<String>,
    smart_model: String,
    eco_model: String,
) -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(format!("{}/{}", provider_name, smart_model)),
        eco_model: Some(format!("{}/{}", provider_name, eco_model)),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        provider_name,
        ProviderConfig::Custom {
            api_key,
            api_endpoint,
        },
    );
    profile
}

/// Hybrid model configuration for smart or eco model
#[derive(Debug, Clone)]
pub struct HybridModelConfig {
    pub provider: HybridProvider,
    pub model: String,
    pub api_key: String,
}

/// Provider options for hybrid configuration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HybridProvider {
    OpenAI,
    Gemini,
    Anthropic,
}

impl HybridProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            HybridProvider::OpenAI => "OpenAI",
            HybridProvider::Gemini => "Gemini",
            HybridProvider::Anthropic => "Anthropic",
        }
    }
}

/// Generate hybrid configuration (mix providers)
pub fn generate_hybrid_config(smart: HybridModelConfig, eco: HybridModelConfig) -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(smart.model.clone()),
        eco_model: Some(eco.model.clone()),
        ..ProfileConfig::default()
    };

    // Add provider configs based on what's needed
    if smart.provider == HybridProvider::OpenAI || eco.provider == HybridProvider::OpenAI {
        profile.providers.insert(
            "openai".to_string(),
            ProviderConfig::OpenAI {
                api_key: if smart.provider == HybridProvider::OpenAI {
                    Some(smart.api_key.clone())
                } else {
                    Some(eco.api_key.clone())
                },
                api_endpoint: None,
            },
        );
    }

    if smart.provider == HybridProvider::Gemini || eco.provider == HybridProvider::Gemini {
        profile.providers.insert(
            "gemini".to_string(),
            ProviderConfig::Gemini {
                api_key: if smart.provider == HybridProvider::Gemini {
                    Some(smart.api_key.clone())
                } else {
                    Some(eco.api_key.clone())
                },
                api_endpoint: None,
            },
        );
    }

    if smart.provider == HybridProvider::Anthropic || eco.provider == HybridProvider::Anthropic {
        profile.providers.insert(
            "anthropic".to_string(),
            ProviderConfig::Anthropic {
                api_key: if smart.provider == HybridProvider::Anthropic {
                    Some(smart.api_key.clone())
                } else {
                    Some(eco.api_key.clone())
                },
                api_endpoint: None,
                access_token: None,
            },
        );
    }

    profile
}

/// Convert profile config to TOML string for preview
pub fn config_to_toml_preview(profile: &ProfileConfig) -> String {
    let mut toml = String::from("[profiles.default]\n");

    if let Some(provider) = &profile.provider {
        toml.push_str(&format!(
            "provider = \"{}\"\n",
            match provider {
                ProviderType::Remote => "remote",
                ProviderType::Local => "local",
            }
        ));
    }

    if let Some(ref smart_model) = profile.smart_model {
        toml.push_str(&format!("smart_model = \"{}\"\n", smart_model));
    }

    if let Some(ref eco_model) = profile.eco_model {
        toml.push_str(&format!("eco_model = \"{}\"\n", eco_model));
    }

    // Output providers in the new unified format
    for (name, config) in &profile.providers {
        toml.push_str(&format!("\n[profiles.default.providers.{}]\n", name));

        match config {
            ProviderConfig::OpenAI {
                api_key,
                api_endpoint,
            } => {
                toml.push_str("type = \"openai\"\n");
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Anthropic {
                api_key,
                api_endpoint,
                access_token,
            } => {
                toml.push_str("type = \"anthropic\"\n");
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
                if let Some(token) = access_token {
                    toml.push_str(&format!(
                        "access_token = \"{}\"\n",
                        if token.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Gemini {
                api_key,
                api_endpoint,
            } => {
                toml.push_str("type = \"gemini\"\n");
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
            } => {
                toml.push_str("type = \"custom\"\n");
                toml.push_str(&format!("api_endpoint = \"{}\"\n", api_endpoint));
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Stakpak {
                api_key,
                api_endpoint,
            } => {
                toml.push_str("type = \"stakpak\"\n");
                toml.push_str(&format!(
                    "api_key = \"{}\"\n",
                    if api_key.is_empty() { "" } else { "***" }
                ));
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
            }
            ProviderConfig::Bedrock {
                region,
                profile_name,
            } => {
                toml.push_str("type = \"amazon-bedrock\"\n");
                toml.push_str(&format!("region = \"{}\"\n", region));
                if let Some(profile) = profile_name {
                    toml.push_str(&format!("profile_name = \"{}\"\n", profile));
                }
            }
        }
    }

    toml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_custom_provider_profile() {
        let profile = generate_custom_provider_profile(
            "litellm".to_string(),
            "http://localhost:4000".to_string(),
            Some("sk-1234".to_string()),
            "claude-opus".to_string(),
            "claude-haiku".to_string(),
        );

        assert!(matches!(profile.provider, Some(ProviderType::Local)));
        assert_eq!(profile.smart_model, Some("litellm/claude-opus".to_string()));
        assert_eq!(profile.eco_model, Some("litellm/claude-haiku".to_string()));

        // Check providers HashMap
        let provider = profile
            .providers
            .get("litellm")
            .expect("litellm provider should exist");
        match provider {
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
            } => {
                assert_eq!(api_endpoint, "http://localhost:4000");
                assert_eq!(api_key, &Some("sk-1234".to_string()));
            }
            _ => panic!("Expected Custom provider"),
        }
    }

    #[test]
    fn test_generate_custom_provider_profile_without_api_key() {
        let profile = generate_custom_provider_profile(
            "ollama".to_string(),
            "http://localhost:11434/v1".to_string(),
            None,
            "llama3".to_string(),
            "llama3".to_string(),
        );

        assert!(matches!(profile.provider, Some(ProviderType::Local)));
        assert_eq!(profile.smart_model, Some("ollama/llama3".to_string()));
        assert_eq!(profile.eco_model, Some("ollama/llama3".to_string()));

        let provider = profile
            .providers
            .get("ollama")
            .expect("ollama provider should exist");
        match provider {
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
            } => {
                assert_eq!(api_endpoint, "http://localhost:11434/v1");
                assert!(api_key.is_none());
            }
            _ => panic!("Expected Custom provider"),
        }
    }

    #[test]
    fn test_config_to_toml_preview_with_custom_provider() {
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("litellm/claude-opus".to_string()),
            eco_model: Some("litellm/claude-haiku".to_string()),
            ..ProfileConfig::default()
        };
        profile.providers.insert(
            "litellm".to_string(),
            ProviderConfig::Custom {
                api_endpoint: "http://localhost:4000".to_string(),
                api_key: Some("sk-1234".to_string()),
            },
        );

        let toml = config_to_toml_preview(&profile);

        assert!(toml.contains("provider = \"local\""));
        assert!(toml.contains("smart_model = \"litellm/claude-opus\""));
        assert!(toml.contains("eco_model = \"litellm/claude-haiku\""));
        assert!(toml.contains("[profiles.default.providers.litellm]"));
        assert!(toml.contains("type = \"custom\""));
        assert!(toml.contains("api_endpoint = \"http://localhost:4000\""));
        assert!(toml.contains("api_key = \"***\"")); // Should be masked
    }

    #[test]
    fn test_config_to_toml_preview_custom_provider_no_api_key() {
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("ollama/llama3".to_string()),
            eco_model: Some("ollama/llama3".to_string()),
            ..ProfileConfig::default()
        };
        profile.providers.insert(
            "ollama".to_string(),
            ProviderConfig::Custom {
                api_endpoint: "http://localhost:11434/v1".to_string(),
                api_key: None,
            },
        );

        let toml = config_to_toml_preview(&profile);

        assert!(toml.contains("[profiles.default.providers.ollama]"));
        assert!(toml.contains("type = \"custom\""));
        assert!(toml.contains("api_endpoint = \"http://localhost:11434/v1\""));
        // api_key line should not appear since it's None
        let lines: Vec<&str> = toml.lines().collect();
        let has_api_key_in_ollama_section = lines
            .iter()
            .skip_while(|l| !l.contains("providers.ollama"))
            .take_while(|l| !l.starts_with('[') || l.contains("providers.ollama"))
            .any(|l| l.contains("api_key"));
        assert!(!has_api_key_in_ollama_section);
    }
}
