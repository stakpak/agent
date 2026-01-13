//! Configuration templates for different provider setups

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
/// * `provider_name` - Name of the provider (e.g., "litellm", "ollama")
/// * `api_endpoint` - API endpoint URL (e.g., "http://localhost:4000")
/// * `api_key` - Optional API key (some providers like Ollama don't require auth)
/// * `smart_model` - Smart model name without provider prefix (e.g., "claude-opus")
/// * `eco_model` - Eco model name without provider prefix (e.g., "claude-haiku")
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
        }
    }

    toml
}

#[cfg(test)]
mod tests {
    use super::*;
    use stakpak_shared::models::integrations::openai::OpenAIConfig;

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
    fn test_config_to_toml_preview_with_custom_providers() {
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

    #[test]
    fn test_migrate_byom_to_providers() {
        // Old BYOM config using openai.api_endpoint
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("qwen/qwen-2.5-coder-32b".to_string()),
            eco_model: Some("qwen/qwen-2.5-coder-32b".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("sk-old-key".to_string()),
                api_endpoint: Some("http://localhost:4000".to_string()),
            }),
            ..ProfileConfig::default()
        };

        profile.migrate_legacy_providers();

        // Should have providers entry now
        let provider = profile
            .providers
            .get("qwen")
            .expect("qwen provider should exist");
        match provider {
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
            } => {
                assert_eq!(api_endpoint, "http://localhost:4000");
                assert_eq!(api_key, &Some("sk-old-key".to_string()));
            }
            _ => panic!("Expected Custom provider"),
        }

        // openai config should be cleared (endpoint was for BYOM, not actual OpenAI)
        assert!(profile.openai.is_none());
    }

    #[test]
    fn test_migrate_byom_preserves_real_openai_config() {
        // A profile with actual OpenAI config (no custom api_endpoint)
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("gpt-4".to_string()),
            eco_model: Some("gpt-4o-mini".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("sk-openai-key".to_string()),
                api_endpoint: None, // No custom endpoint = real OpenAI
            }),
            ..ProfileConfig::default()
        };

        profile.migrate_legacy_providers();

        // Should migrate to providers HashMap as OpenAI type
        assert!(profile.openai.is_none()); // Consumed by migration
        let openai = profile
            .providers
            .get("openai")
            .expect("openai should exist");
        match openai {
            ProviderConfig::OpenAI { api_key, .. } => {
                assert_eq!(api_key, &Some("sk-openai-key".to_string()));
            }
            _ => panic!("Expected OpenAI provider"),
        }
    }

    #[test]
    fn test_migrate_byom_no_provider_prefix_in_model() {
        // BYOM config where model doesn't have a prefix (edge case)
        // Should use "custom" as the provider name
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("some-model".to_string()), // No "/" prefix
            eco_model: Some("some-model".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("sk-key".to_string()),
                api_endpoint: Some("http://localhost:4000".to_string()),
            }),
            ..ProfileConfig::default()
        };

        profile.migrate_legacy_providers();

        // Should migrate, using "custom" as provider name (no prefix in model)
        let provider = profile
            .providers
            .get("custom")
            .expect("custom provider should exist");
        assert!(matches!(provider, ProviderConfig::Custom { .. }));
    }

    #[test]
    fn test_migrate_byom_already_has_provider() {
        // Profile that already has the provider in providers HashMap should not be modified
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("litellm/claude".to_string()),
            eco_model: Some("litellm/claude".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("old-key".to_string()),
                api_endpoint: Some("http://old-endpoint".to_string()),
            }),
            ..ProfileConfig::default()
        };
        profile.providers.insert(
            "litellm".to_string(),
            ProviderConfig::Custom {
                api_endpoint: "http://localhost:4000".to_string(),
                api_key: Some("new-key".to_string()),
            },
        );

        profile.migrate_legacy_providers();

        // Should not change providers - litellm already existed
        let provider = profile
            .providers
            .get("litellm")
            .expect("litellm should exist");
        match provider {
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
            } => {
                assert_eq!(api_key, &Some("new-key".to_string()));
                assert_eq!(api_endpoint, "http://localhost:4000");
            }
            _ => panic!("Expected Custom provider"),
        }
    }
}
