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

    /// User's preferred model (replaces smart_model/eco_model/recovery_model)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    // =========================================================================
    // Legacy model fields - kept for backward compatibility during migration
    // These are read but deprecated (will migrate to 'model' field)
    // =========================================================================
    /// Eco (fast/cheap) model name (deprecated - use 'model')
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eco_model: Option<String>,
    /// Smart (capable) model name (deprecated - use 'model')
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart_model: Option<String>,
    /// Recovery model name (deprecated - use 'model')
    #[serde(skip_serializing_if = "Option::is_none")]
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
    /// This creates a replica of the default profile with warden enabled for sandboxed execution.
    pub(crate) fn readonly_profile(default_profile: Option<&ProfileConfig>) -> Self {
        match default_profile {
            Some(default) => ProfileConfig {
                // Copy all provider-related fields from default
                api_endpoint: default.api_endpoint.clone(),
                api_key: default.api_key.clone(),
                provider: default.provider,
                providers: default.providers.clone(),
                // Copy model fields
                model: default.model.clone(),
                smart_model: default.smart_model.clone(),
                eco_model: default.eco_model.clone(),
                recovery_model: default.recovery_model.clone(),
                // Enable warden for readonly sandboxed execution
                warden: Some(WardenConfig::readonly_profile()),
                // Don't copy allowed_tools/auto_approve - readonly has its own restrictions
                ..ProfileConfig::default()
            },
            None => ProfileConfig {
                warden: Some(WardenConfig::readonly_profile()),
                ..ProfileConfig::default()
            },
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

    /// Clean an API endpoint by removing `/chat/completions` suffix if present.
    /// This suffix is appended at runtime by the provider, not stored in config.
    fn clean_api_endpoint(endpoint: Option<String>) -> Option<String> {
        endpoint.map(|ep| {
            ep.trim_end_matches("/chat/completions")
                .trim_end_matches("/chat/completions/")
                .to_string()
        })
    }

    /// Migrate legacy provider fields to the new unified `providers` HashMap.
    ///
    /// This converts:
    /// - `openai`, `anthropic`, `gemini` fields -> `providers["openai"]`, etc.
    /// - Strips `/chat/completions` from endpoints (added at runtime)
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
                    api_endpoint: Self::clean_api_endpoint(openai.api_endpoint),
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
                        api_endpoint: Self::clean_api_endpoint(anthropic.api_endpoint),
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
                        api_endpoint: Self::clean_api_endpoint(gemini.api_endpoint),
                    },
                );
                migrated = true;
            }
        }

        // Also clean existing providers in HashMap
        migrated = self.clean_provider_endpoints() || migrated;

        migrated
    }

    /// Clean `/chat/completions` suffix from all provider endpoints.
    /// Returns true if any endpoint was modified.
    fn clean_provider_endpoints(&mut self) -> bool {
        let mut cleaned = false;

        for (_name, provider) in self.providers.iter_mut() {
            match provider {
                ProviderConfig::OpenAI { api_endpoint, .. } => {
                    if let Some(ep) = api_endpoint {
                        let clean = Self::clean_api_endpoint(Some(ep.clone()));
                        if clean.as_ref() != Some(ep) {
                            *api_endpoint = clean;
                            cleaned = true;
                        }
                    }
                }
                ProviderConfig::Anthropic { api_endpoint, .. } => {
                    if let Some(ep) = api_endpoint {
                        let clean = Self::clean_api_endpoint(Some(ep.clone()));
                        if clean.as_ref() != Some(ep) {
                            *api_endpoint = clean;
                            cleaned = true;
                        }
                    }
                }
                ProviderConfig::Gemini { api_endpoint, .. } => {
                    if let Some(ep) = api_endpoint {
                        let clean = Self::clean_api_endpoint(Some(ep.clone()));
                        if clean.as_ref() != Some(ep) {
                            *api_endpoint = clean;
                            cleaned = true;
                        }
                    }
                }
                ProviderConfig::Custom { api_endpoint, .. } => {
                    let clean = Self::clean_api_endpoint(Some(api_endpoint.clone()));
                    if let Some(clean_ep) = clean
                        && &clean_ep != api_endpoint
                    {
                        *api_endpoint = clean_ep;
                        cleaned = true;
                    }
                }
                ProviderConfig::Stakpak { api_endpoint, .. } => {
                    if let Some(ep) = api_endpoint {
                        let clean = Self::clean_api_endpoint(Some(ep.clone()));
                        if clean.as_ref() != Some(ep) {
                            *api_endpoint = clean;
                            cleaned = true;
                        }
                    }
                }
                ProviderConfig::Bedrock { .. } => {
                    // Bedrock has no API endpoint to clean
                }
            }
        }

        cleaned
    }

    /// Check if this profile has legacy provider fields or endpoints that need migration/cleaning.
    pub fn needs_provider_migration(&self) -> bool {
        // Check for legacy provider fields
        if self.openai.is_some() || self.anthropic.is_some() || self.gemini.is_some() {
            return true;
        }

        // Check for endpoints with /chat/completions that need cleaning
        for provider in self.providers.values() {
            if let Some(ep) = provider.api_endpoint()
                && ep.contains("/chat/completions")
            {
                return true;
            }
        }

        false
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
                .or_else(|| other.and_then(|config| config.provider)),
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
            // New unified model field
            model: self
                .model
                .clone()
                .or_else(|| other.and_then(|config| config.model.clone())),
            // Legacy fields - merge for backward compatibility during transition
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
