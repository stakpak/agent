//! Profile configuration for per-environment settings.

use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::anthropic::AnthropicConfig;
use stakpak_shared::models::integrations::gemini::GeminiConfig;
use stakpak_shared::models::integrations::openai::OpenAIConfig;
use stakpak_shared::models::llm::ProviderConfig;
use std::collections::HashMap;

use super::rulebook::RulebookConfig;
use super::types::{OldAppConfig, ProviderType};
use super::warden::WardenConfig;

/// Configuration for a specific profile (environment).
///
/// # New Config Format (v2)
/// ```toml
/// [profiles.myprofile.providers.openai]
/// type = "openai"
/// api_key = "sk-..."
///
/// [profiles.myprofile.providers.anthropic]
/// type = "anthropic"
/// api_key = "sk-ant-..."
///
/// [profiles.myprofile.providers.litellm]
/// type = "custom"
/// api_endpoint = "http://localhost:4000"
/// api_key = "sk-litellm"
/// ```
///
/// # Legacy Config Format (v1) - still supported for reading
/// ```toml
/// [profiles.myprofile]
/// openai.api_key = "sk-..."
/// anthropic.api_key = "sk-ant-..."
/// ```
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProfileConfig {
    /// API endpoint URL
    pub api_endpoint: Option<String>,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Provider type (remote or local)
    pub provider: Option<ProviderType>,
    /// Allowed tools (empty = all tools allowed)
    pub allowed_tools: Option<Vec<String>>,
    /// Tools that auto-approve without asking
    pub auto_approve: Option<Vec<String>>,
    /// Rulebook filtering configuration
    pub rulebooks: Option<RulebookConfig>,
    /// Warden (runtime security) configuration
    pub warden: Option<WardenConfig>,

    /// Unified providers configuration (new format)
    /// Key is provider name (e.g., "openai", "anthropic", "litellm")
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub providers: HashMap<String, ProviderConfig>,

    // =========================================================================
    // Legacy fields - kept for backward compatibility during config migration
    // These are read but not written (skip_serializing)
    // =========================================================================
    /// OpenAI configuration (legacy - use providers instead)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<OpenAIConfig>,
    /// Gemini configuration (legacy - use providers instead)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<GeminiConfig>,
    /// Anthropic configuration (legacy - use providers instead)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<AnthropicConfig>,

    /// Eco (fast/cheap) model name
    pub eco_model: Option<String>,
    /// Smart (capable) model name
    pub smart_model: Option<String>,
    /// Recovery model name
    pub recovery_model: Option<String>,
}

impl ProfileConfig {
    /// Create a profile with only the API endpoint set.
    pub(crate) fn with_api_endpoint(api_endpoint: &str) -> Self {
        ProfileConfig {
            api_endpoint: Some(api_endpoint.into()),
            ..ProfileConfig::default()
        }
    }

    /// Create a readonly profile based on the default profile.
    pub(crate) fn readonly_profile(default_profile: Option<&ProfileConfig>) -> Self {
        ProfileConfig {
            api_endpoint: default_profile.and_then(|p| p.api_endpoint.clone()),
            api_key: default_profile.and_then(|p| p.api_key.clone()),
            warden: Some(WardenConfig::readonly_profile()),
            ..ProfileConfig::default()
        }
    }

    /// Create a profile migrated from the old config format.
    pub(crate) fn migrated_from_old_config(old_config: OldAppConfig) -> Self {
        ProfileConfig {
            api_endpoint: Some(old_config.api_endpoint),
            api_key: old_config.api_key,
            ..ProfileConfig::default()
        }
    }

    /// Migrate legacy provider fields to the new unified `providers` HashMap.
    ///
    /// This converts:
    /// - `openai`, `anthropic`, `gemini` fields -> `providers["openai"]`, etc.
    ///
    /// Returns true if any migration was performed.
    #[allow(clippy::collapsible_if)]
    pub fn migrate_legacy_providers(&mut self) -> bool {
        let mut migrated = false;

        // Migrate openai
        if let Some(openai) = self.openai.take() {
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.providers.entry("openai".to_string())
            {
                e.insert(ProviderConfig::OpenAI {
                    api_key: openai.api_key,
                    api_endpoint: openai.api_endpoint,
                });
                migrated = true;
            }
        }

        // Migrate anthropic
        if let Some(anthropic) = self.anthropic.take() {
            if !self.providers.contains_key("anthropic") {
                self.providers.insert(
                    "anthropic".to_string(),
                    ProviderConfig::Anthropic {
                        api_key: anthropic.api_key,
                        api_endpoint: anthropic.api_endpoint,
                        access_token: anthropic.access_token,
                    },
                );
                migrated = true;
            }
        }

        // Migrate gemini
        if let Some(gemini) = self.gemini.take() {
            if !self.providers.contains_key("gemini") {
                self.providers.insert(
                    "gemini".to_string(),
                    ProviderConfig::Gemini {
                        api_key: gemini.api_key,
                        api_endpoint: gemini.api_endpoint,
                    },
                );
                migrated = true;
            }
        }

        migrated
    }

    /// Check if this profile has legacy provider fields that need migration.
    pub fn needs_provider_migration(&self) -> bool {
        self.openai.is_some() || self.anthropic.is_some() || self.gemini.is_some()
    }

    /// Merge this profile with another, using self's values if present.
    pub(crate) fn merge(&self, other: Option<&ProfileConfig>) -> ProfileConfig {
        // Merge providers: start with other's providers, then overlay self's
        let mut merged_providers = other.map(|o| o.providers.clone()).unwrap_or_default();
        for (name, config) in &self.providers {
            merged_providers.insert(name.clone(), config.clone());
        }

        ProfileConfig {
            api_endpoint: self
                .api_endpoint
                .clone()
                .or_else(|| other.and_then(|config| config.api_endpoint.clone())),
            api_key: self
                .api_key
                .clone()
                .or_else(|| other.and_then(|config| config.api_key.clone())),
            allowed_tools: self
                .allowed_tools
                .clone()
                .or_else(|| other.and_then(|config| config.allowed_tools.clone())),
            auto_approve: self
                .auto_approve
                .clone()
                .or_else(|| other.and_then(|config| config.auto_approve.clone())),
            rulebooks: self
                .rulebooks
                .clone()
                .or_else(|| other.and_then(|config| config.rulebooks.clone())),
            warden: self
                .warden
                .clone()
                .or_else(|| other.and_then(|config| config.warden.clone())),
            provider: self
                .provider
                .clone()
                .or_else(|| other.and_then(|config| config.provider.clone())),
            providers: merged_providers,
            // Legacy fields - merge for backward compatibility during transition
            openai: self
                .openai
                .clone()
                .or_else(|| other.and_then(|config| config.openai.clone())),
            anthropic: self
                .anthropic
                .clone()
                .or_else(|| other.and_then(|config| config.anthropic.clone())),
            gemini: self
                .gemini
                .clone()
                .or_else(|| other.and_then(|config| config.gemini.clone())),
            eco_model: self
                .eco_model
                .clone()
                .or_else(|| other.and_then(|config| config.eco_model.clone())),
            smart_model: self
                .smart_model
                .clone()
                .or_else(|| other.and_then(|config| config.smart_model.clone())),
            recovery_model: self
                .recovery_model
                .clone()
                .or_else(|| other.and_then(|config| config.recovery_model.clone())),
        }
    }
}

impl From<OldAppConfig> for ProfileConfig {
    fn from(old_config: OldAppConfig) -> Self {
        ProfileConfig {
            api_endpoint: Some(old_config.api_endpoint),
            api_key: old_config.api_key,
            ..ProfileConfig::default()
        }
    }
}
