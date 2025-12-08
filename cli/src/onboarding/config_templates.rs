//! Configuration templates for different provider setups

use crate::config::ProfileConfig;
use crate::config::ProviderType;
use stakpak_shared::models::integrations::anthropic::{AnthropicConfig, AnthropicModel};
use stakpak_shared::models::integrations::gemini::{GeminiConfig, GeminiModel};
use stakpak_shared::models::integrations::openai::{OpenAIConfig, OpenAIModel};

/// Generate OpenAI configuration template
pub fn generate_openai_config(api_key: String) -> ProfileConfig {
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(OpenAIModel::default_smart_model()),
        eco_model: Some(OpenAIModel::default_eco_model()),
        recovery_model: Some(OpenAIModel::default_recovery_model()),
        openai: Some(OpenAIConfig {
            api_key: Some(api_key),
            api_endpoint: None,
        }),
        ..ProfileConfig::default()
    }
}

/// Generate Gemini configuration template
pub fn generate_gemini_config(api_key: String) -> ProfileConfig {
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(GeminiModel::default_smart_model()),
        eco_model: Some(GeminiModel::default_eco_model()),
        recovery_model: Some(GeminiModel::default_recovery_model()),
        gemini: Some(GeminiConfig {
            api_key: Some(api_key),
            api_endpoint: None,
        }),
        ..ProfileConfig::default()
    }
}

/// Generate Anthropic configuration template
pub fn generate_anthropic_config(api_key: String) -> ProfileConfig {
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(AnthropicModel::default_smart_model()),
        eco_model: Some(AnthropicModel::default_eco_model()),
        recovery_model: Some(AnthropicModel::default_recovery_model()),
        anthropic: Some(AnthropicConfig {
            api_key: Some(api_key),
            api_endpoint: None,
        }),
        ..ProfileConfig::default()
    }
}

/// Generate BYOM (Bring Your Own Model) single model configuration
pub fn generate_byom_single_config(
    endpoint: String,
    model: String,
    api_key: Option<String>,
) -> ProfileConfig {
    ProfileConfig {
        provider: Some(ProviderType::Local),
        smart_model: Some(model.clone()),
        eco_model: Some(model),
        recovery_model: None,
        openai: Some(OpenAIConfig {
            api_key,
            api_endpoint: Some(endpoint),
        }),
        ..ProfileConfig::default()
    }
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
        recovery_model: None,
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

    if let Some(ref recovery_model) = profile.recovery_model {
        toml.push_str(&format!("recovery_model = \"{}\"\n", recovery_model));
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

    toml
}
