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

    /// Recently used models (most recent first, max 5)
    /// Stores model IDs which may include provider prefix for Stakpak API routing
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_models: Vec<String>,

    /// System prompt override for sessions using this profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

    /// Maximum agent turns per session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<usize>,

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
                // Copy unified model field only (legacy fields are not copied)
                model: default.model.clone(),
                // Copy recent models + runtime overrides
                recent_models: default.recent_models.clone(),
                system_prompt: default.system_prompt.clone(),
                max_turns: default.max_turns,
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
                    auth: None,
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
                        auth: None,
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
                        auth: None,
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

    /// Check if this profile has legacy model fields that need migration.
    pub fn needs_model_migration(&self) -> bool {
        // If we have legacy fields but no unified model field, migration is needed
        self.model.is_none()
            && (self.smart_model.is_some()
                || self.eco_model.is_some()
                || self.recovery_model.is_some())
    }

    /// Migrate legacy model fields (smart_model, eco_model, recovery_model) to unified 'model' field.
    ///
    /// Priority: smart_model > eco_model > recovery_model
    ///
    /// After migration, legacy fields are cleared so they won't be serialized.
    /// Returns true if migration was performed.
    pub fn migrate_model_fields(&mut self) -> bool {
        if !self.needs_model_migration() {
            return false;
        }

        // Take ownership of legacy fields and pick the best one
        let smart = self.smart_model.take();
        let eco = self.eco_model.take();
        let recovery = self.recovery_model.take();

        // Priority: smart > eco > recovery
        // (At least one is Some due to needs_model_migration() guard)
        self.model = smart.or(eco).or(recovery);

        true
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
            // Unified model field
            model: self
                .model
                .clone()
                .or_else(|| other.and_then(|config| config.model.clone())),
            // Recent models - use self's if non-empty, otherwise other's
            recent_models: if !self.recent_models.is_empty() {
                self.recent_models.clone()
            } else {
                other
                    .map(|config| config.recent_models.clone())
                    .unwrap_or_default()
            },
            system_prompt: self
                .system_prompt
                .clone()
                .or_else(|| other.and_then(|config| config.system_prompt.clone())),
            max_turns: self
                .max_turns
                .or_else(|| other.and_then(|config| config.max_turns)),
            // Legacy fields - kept for reading only, not merged
            eco_model: None,
            smart_model: None,
            recovery_model: None,
        }
    }

    /// Validate profile-specific constraints.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(max_turns) = self.max_turns
            && !(1..=256).contains(&max_turns)
        {
            return Err(format!("max_turns must be 1-256, got {max_turns}"));
        }

        if let Some(system_prompt) = self.system_prompt.as_ref()
            && system_prompt.chars().count() > 32 * 1024
        {
            return Err("system_prompt exceeds 32KB character limit".to_string());
        }

        Ok(())
    }

    /// Maximum number of recent models to store
    const MAX_RECENT_MODELS: usize = 5;

    /// Add a model to the recent models list.
    ///
    /// The model is added to the front of the list. If the model already exists,
    /// it's moved to the front. The list is truncated to MAX_RECENT_MODELS entries.
    ///
    /// The `recent_id` should already be in normalized `"provider/short_name"` format
    /// (see [`format_recent_model_id`]).
    pub fn add_recent_model(&mut self, recent_id: &str) {
        // Remove if already exists (we'll re-add at front)
        self.recent_models.retain(|id| id != recent_id);

        // Add to front
        self.recent_models.insert(0, recent_id.to_string());

        // Truncate to max size
        self.recent_models.truncate(Self::MAX_RECENT_MODELS);
    }

    /// Migrate old-format `recent_models` entries to normalized `"provider/short_name"` format.
    ///
    /// Old entries may be bare model names like `"glm-4.6"` or `"claude-sonnet-4-6"` without
    /// a provider prefix. Bare entries whose short name already appears in a prefixed entry
    /// are dropped (they're duplicates). Remaining bare entries are prefixed with the
    /// config `model` field's provider, or `"stakpak"` as a fallback.
    ///
    /// Also ensures the config `model` field is represented in `recent_models`.
    /// Returns true if any entries were modified.
    pub fn migrate_recent_models(&mut self) -> bool {
        let mut changed = false;

        // Determine default provider from the config model field (e.g., "z/glm-4.6" -> "z")
        let default_provider = self
            .model
            .as_deref()
            .and_then(|m| m.find('/').map(|idx| &m[..idx]))
            .unwrap_or("stakpak");

        // Collect the short names that already have a prefixed entry
        let prefixed_short_names: Vec<String> = self
            .recent_models
            .iter()
            .filter(|id| id.contains('/'))
            .filter_map(|id| id.rsplit('/').next().map(|s| s.to_string()))
            .collect();

        // Normalize: drop bare entries that duplicate a prefixed one,
        // prefix remaining bare entries with the default provider
        let migrated: Vec<String> = self
            .recent_models
            .iter()
            .filter_map(|id| {
                if id.contains('/') {
                    Some(id.clone())
                } else if prefixed_short_names.iter().any(|s| s == id) {
                    // Bare entry duplicates an existing prefixed entry — drop it
                    changed = true;
                    None
                } else {
                    changed = true;
                    Some(format!("{}/{}", default_provider, id))
                }
            })
            .collect();

        if changed {
            self.recent_models = migrated;
        }

        // Ensure the config model is in recent_models (at the end, not overriding order).
        // Make room first so the config model isn't immediately truncated away.
        if let Some(ref model_str) = self.model {
            let (provider, model_id) = model_str
                .split_once('/')
                .unwrap_or((default_provider, model_str));
            let recent_id = format_recent_model_id(provider, model_id);
            if !self.recent_models.contains(&recent_id) {
                // Truncate to MAX-1 so there's guaranteed room for the config model
                self.recent_models
                    .truncate(Self::MAX_RECENT_MODELS.saturating_sub(1));
                self.recent_models.push(recent_id);
                changed = true;
            }
        }

        changed
    }
}

/// Format a model's provider and ID into the normalized `"provider/short_name"`
/// format used for `recent_models` storage.
///
/// The short name is the last segment of the model ID (after the last `/`),
/// which strips long upstream paths like `"fireworks-ai/accounts/fireworks/models/glm-5"`
/// down to just `"glm-5"`. Combined with the provider, this produces clean entries
/// like `"stakpak/glm-5"`, `"anthropic/claude-sonnet-4-5"`, or `"z.ai/glm-4.6"`.
pub fn format_recent_model_id(provider: &str, model_id: &str) -> String {
    // Take only the last segment after "/" to get the short model name
    let short_name = model_id.rsplit('/').next().unwrap_or(model_id);
    format!("{}/{}", provider, short_name)
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
