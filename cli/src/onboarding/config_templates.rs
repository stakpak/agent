//! Configuration templates for different provider setups

use crate::config::CustomProvider;
use crate::config::ProfileConfig;
use crate::config::ProviderType;
use stakpak_shared::models::integrations::anthropic::{AnthropicConfig, AnthropicModel};
use stakpak_shared::models::integrations::gemini::{GeminiConfig, GeminiModel};
use stakpak_shared::models::integrations::openai::{OpenAIConfig, OpenAIModel};

/// Generate OpenAI profile configuration (credentials stored separately in auth.toml)
pub fn generate_openai_profile() -> ProfileConfig {
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(OpenAIModel::default_smart_model()),
        eco_model: Some(OpenAIModel::default_eco_model()),
        openai: Some(OpenAIConfig {
            api_key: None,
            api_endpoint: None,
        }),
        ..ProfileConfig::default()
    }
}

/// Generate Gemini profile configuration (credentials stored separately in auth.toml)
pub fn generate_gemini_profile() -> ProfileConfig {
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(GeminiModel::default_smart_model()),
        eco_model: Some(GeminiModel::default_eco_model()),
        gemini: Some(GeminiConfig {
            api_key: None,
            api_endpoint: None,
        }),
        ..ProfileConfig::default()
    }
}

/// Generate Anthropic profile configuration (credentials stored separately in auth.toml)
pub fn generate_anthropic_profile() -> ProfileConfig {
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(AnthropicModel::default_smart_model()),
        eco_model: Some(AnthropicModel::default_eco_model()),
        anthropic: Some(AnthropicConfig {
            api_key: None,
            api_endpoint: None,
            access_token: None,
        }),
        ..ProfileConfig::default()
    }
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
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(format!("{}/{}", provider_name, smart_model)),
        eco_model: Some(format!("{}/{}", provider_name, eco_model)),
        custom_providers: Some(vec![CustomProvider {
            name: provider_name,
            api_endpoint,
            api_key,
        }]),
        ..ProfileConfig::default()
    }
}

/// Migrate old BYOM configuration to use custom_providers
///
/// Old BYOM configs used `openai.api_endpoint` to specify a custom endpoint.
/// This function migrates those configs to use the new `custom_providers` field.
///
/// Migration rules:
/// - If `openai.api_endpoint` is set and `custom_providers` is None, migrate
/// - If model has a "/" prefix, use that as provider name; otherwise use the profile name
/// - Clear the openai config after migration (it was only used for BYOM, not real OpenAI)
pub fn migrate_byom_to_custom_provider(
    mut profile: ProfileConfig,
    profile_name: &str,
) -> ProfileConfig {
    // Skip if already has custom_providers
    if profile.custom_providers.is_some() {
        return profile;
    }

    // Check if this is a BYOM config (has openai.api_endpoint set)
    // Extract info from openai config if it exists and has an api_endpoint
    let Some(mut openai) = profile.openai.take() else {
        return profile;
    };

    let Some(api_endpoint) = openai.api_endpoint.take() else {
        // Not a BYOM config - restore openai and return
        profile.openai = Some(openai);
        return profile;
    };

    let api_key = openai.api_key;

    // Extract provider name from smart_model (e.g., "litellm/claude" -> "litellm")
    // If no "/" found, use the profile name as the provider name
    let (provider_name, needs_model_update) = profile
        .smart_model
        .as_ref()
        .and_then(|m| {
            if m.contains('/') {
                m.split('/').next().map(|p| (p.to_string(), false))
            } else {
                None
            }
        })
        .unwrap_or_else(|| (profile_name.to_string(), true));

    // Update model names if they don't have provider prefix
    if needs_model_update {
        if let Some(ref model) = profile.smart_model {
            profile.smart_model = Some(format!("{}/{}", provider_name, model));
        }
        if let Some(ref model) = profile.eco_model {
            profile.eco_model = Some(format!("{}/{}", provider_name, model));
        }
    }

    // Create custom provider
    profile.custom_providers = Some(vec![CustomProvider {
        name: provider_name,
        api_endpoint,
        api_key,
    }]);

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
        profile.openai = Some(OpenAIConfig {
            api_key: if smart.provider == HybridProvider::OpenAI {
                Some(smart.api_key.clone())
            } else {
                Some(eco.api_key.clone())
            },
            api_endpoint: None,
        });
    }

    if smart.provider == HybridProvider::Gemini || eco.provider == HybridProvider::Gemini {
        profile.gemini = Some(GeminiConfig {
            api_key: if smart.provider == HybridProvider::Gemini {
                Some(smart.api_key.clone())
            } else {
                Some(eco.api_key.clone())
            },
            api_endpoint: None,
        });
    }

    if smart.provider == HybridProvider::Anthropic || eco.provider == HybridProvider::Anthropic {
        profile.anthropic = Some(AnthropicConfig {
            api_key: if smart.provider == HybridProvider::Anthropic {
                Some(smart.api_key.clone())
            } else {
                Some(eco.api_key.clone())
            },
            api_endpoint: None,
            access_token: None,
        });
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

    if let Some(ref openai) = profile.openai {
        toml.push_str("\n[profiles.default.openai]\n");
        if let Some(ref endpoint) = openai.api_endpoint {
            toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
        }
        if let Some(ref key) = openai.api_key {
            toml.push_str(&format!(
                "api_key = \"{}\"\n",
                if key.is_empty() { "" } else { "***" }
            ));
        }
    }

    if let Some(ref gemini) = profile.gemini {
        toml.push_str("\n[profiles.default.gemini]\n");
        if let Some(ref key) = gemini.api_key {
            toml.push_str(&format!(
                "api_key = \"{}\"\n",
                if key.is_empty() { "" } else { "***" }
            ));
        }
    }

    if let Some(ref anthropic) = profile.anthropic {
        toml.push_str("\n[profiles.default.anthropic]\n");
        if let Some(ref key) = anthropic.api_key {
            toml.push_str(&format!(
                "api_key = \"{}\"\n",
                if key.is_empty() { "" } else { "***" }
            ));
        }
    }

    if let Some(ref custom_providers) = profile.custom_providers {
        for cp in custom_providers {
            toml.push_str("\n[[profiles.default.custom_providers]]\n");
            toml.push_str(&format!("name = \"{}\"\n", cp.name));
            toml.push_str(&format!("api_endpoint = \"{}\"\n", cp.api_endpoint));
            if let Some(ref key) = cp.api_key {
                toml.push_str(&format!(
                    "api_key = \"{}\"\n",
                    if key.is_empty() { "" } else { "***" }
                ));
            }
        }
    }

    toml
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CustomProvider;

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

        let custom_providers = profile
            .custom_providers
            .expect("custom_providers should be set");
        assert_eq!(custom_providers.len(), 1);
        assert_eq!(custom_providers[0].name, "litellm");
        assert_eq!(custom_providers[0].api_endpoint, "http://localhost:4000");
        assert_eq!(custom_providers[0].api_key, Some("sk-1234".to_string()));
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

        let custom_providers = profile
            .custom_providers
            .expect("custom_providers should be set");
        assert_eq!(custom_providers.len(), 1);
        assert_eq!(custom_providers[0].name, "ollama");
        assert_eq!(custom_providers[0].api_key, None);
    }

    #[test]
    fn test_config_to_toml_preview_with_custom_providers() {
        let profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("litellm/claude-opus".to_string()),
            eco_model: Some("litellm/claude-haiku".to_string()),
            custom_providers: Some(vec![CustomProvider {
                name: "litellm".to_string(),
                api_endpoint: "http://localhost:4000".to_string(),
                api_key: Some("sk-1234".to_string()),
            }]),
            ..ProfileConfig::default()
        };

        let toml = config_to_toml_preview(&profile);

        assert!(toml.contains("provider = \"local\""));
        assert!(toml.contains("smart_model = \"litellm/claude-opus\""));
        assert!(toml.contains("eco_model = \"litellm/claude-haiku\""));
        assert!(toml.contains("[[profiles.default.custom_providers]]"));
        assert!(toml.contains("name = \"litellm\""));
        assert!(toml.contains("api_endpoint = \"http://localhost:4000\""));
        assert!(toml.contains("api_key = \"***\"")); // Should be masked
    }

    #[test]
    fn test_config_to_toml_preview_custom_provider_no_api_key() {
        let profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("ollama/llama3".to_string()),
            eco_model: Some("ollama/llama3".to_string()),
            custom_providers: Some(vec![CustomProvider {
                name: "ollama".to_string(),
                api_endpoint: "http://localhost:11434/v1".to_string(),
                api_key: None,
            }]),
            ..ProfileConfig::default()
        };

        let toml = config_to_toml_preview(&profile);

        assert!(toml.contains("[[profiles.default.custom_providers]]"));
        assert!(toml.contains("name = \"ollama\""));
        assert!(toml.contains("api_endpoint = \"http://localhost:11434/v1\""));
        assert!(!toml.contains("api_key")); // Should not have api_key line
    }

    #[test]
    fn test_migrate_byom_to_custom_provider() {
        // Old BYOM config using openai.api_endpoint
        let old_byom_profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("qwen/qwen-2.5-coder-32b".to_string()),
            eco_model: Some("qwen/qwen-2.5-coder-32b".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("sk-old-key".to_string()),
                api_endpoint: Some("http://localhost:4000".to_string()),
            }),
            custom_providers: None,
            ..ProfileConfig::default()
        };

        let migrated = migrate_byom_to_custom_provider(old_byom_profile, "default");

        // Should have custom_providers now
        let custom_providers = migrated
            .custom_providers
            .expect("custom_providers should be set after migration");
        assert_eq!(custom_providers.len(), 1);

        // Provider name should be extracted from smart_model prefix
        assert_eq!(custom_providers[0].name, "qwen");
        assert_eq!(custom_providers[0].api_endpoint, "http://localhost:4000");
        assert_eq!(custom_providers[0].api_key, Some("sk-old-key".to_string()));

        // openai config should be cleared (endpoint was for BYOM, not actual OpenAI)
        assert!(migrated.openai.is_none());
    }

    #[test]
    fn test_migrate_byom_preserves_real_openai_config() {
        // A profile with actual OpenAI config (no custom api_endpoint)
        let openai_profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("gpt-4".to_string()),
            eco_model: Some("gpt-4o-mini".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("sk-openai-key".to_string()),
                api_endpoint: None, // No custom endpoint = real OpenAI
            }),
            custom_providers: None,
            ..ProfileConfig::default()
        };

        let migrated = migrate_byom_to_custom_provider(openai_profile.clone(), "default");

        // Should NOT migrate - openai config should remain
        assert!(migrated.openai.is_some());
        assert!(migrated.custom_providers.is_none());
        assert_eq!(
            migrated.openai.unwrap().api_key,
            Some("sk-openai-key".to_string())
        );
    }

    #[test]
    fn test_migrate_byom_no_provider_prefix_in_model() {
        // BYOM config where model doesn't have a prefix (edge case)
        // Should use the profile name as the provider name
        let old_byom_profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("some-model".to_string()), // No "/" prefix
            eco_model: Some("some-model".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("sk-key".to_string()),
                api_endpoint: Some("http://localhost:4000".to_string()),
            }),
            custom_providers: None,
            ..ProfileConfig::default()
        };

        let migrated = migrate_byom_to_custom_provider(old_byom_profile, "myprofile");

        // Should still migrate, using the profile name as provider name
        let custom_providers = migrated
            .custom_providers
            .expect("custom_providers should be set");
        assert_eq!(custom_providers[0].name, "myprofile");
        assert_eq!(
            migrated.smart_model,
            Some("myprofile/some-model".to_string())
        );
        assert_eq!(migrated.eco_model, Some("myprofile/some-model".to_string()));
    }

    #[test]
    fn test_migrate_byom_already_has_custom_providers() {
        // Profile that already has custom_providers should not be modified
        let profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            smart_model: Some("litellm/claude".to_string()),
            eco_model: Some("litellm/claude".to_string()),
            openai: Some(OpenAIConfig {
                api_key: Some("old-key".to_string()),
                api_endpoint: Some("http://old-endpoint".to_string()),
            }),
            custom_providers: Some(vec![CustomProvider {
                name: "litellm".to_string(),
                api_endpoint: "http://localhost:4000".to_string(),
                api_key: Some("new-key".to_string()),
            }]),
            ..ProfileConfig::default()
        };

        let migrated = migrate_byom_to_custom_provider(profile.clone(), "default");

        // Should not change custom_providers
        let custom_providers = migrated
            .custom_providers
            .expect("custom_providers should exist");
        assert_eq!(custom_providers.len(), 1);
        assert_eq!(custom_providers[0].api_key, Some("new-key".to_string()));
    }
}
