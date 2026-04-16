//! Main application configuration.

use config::ConfigError;
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::models::integrations::anthropic::AnthropicConfig;
use stakpak_shared::models::integrations::gemini::GeminiConfig;
use stakpak_shared::models::llm::{LLMProviderConfig, ProviderConfig};
use std::collections::HashMap;
use std::fs::{create_dir_all, write};
use std::io;
use std::path::{Path, PathBuf};

use super::file::ConfigFile;
use super::profile::ProfileConfig;
use super::rulebook::RulebookConfig;
use super::types::{OldAppConfig, ProviderType, Settings};
use super::warden::WardenConfig;
use super::{STAKPAK_API_ENDPOINT, STAKPAK_CONFIG_PATH};

/// The main application configuration, built from config file and environment.
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// API endpoint URL
    pub api_endpoint: String,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Provider type (remote or local)
    pub provider: ProviderType,
    /// MCP server host
    pub mcp_server_host: Option<String>,
    /// Machine name for identification
    pub machine_name: Option<String>,
    /// Whether to auto-append .stakpak to .gitignore
    pub auto_append_gitignore: Option<bool>,
    /// Current profile name
    pub profile_name: String,
    /// Path to the config file (used for saving)
    pub config_path: String,
    /// Allowed tools (empty = all tools allowed)
    pub allowed_tools: Option<Vec<String>>,
    /// Tools that auto-approve without asking
    pub auto_approve: Option<Vec<String>>,
    /// Rulebook filtering configuration
    pub rulebooks: Option<RulebookConfig>,
    /// Warden (runtime security) configuration
    pub warden: Option<WardenConfig>,
    /// Unified provider configurations (key = provider name)
    pub providers: HashMap<String, ProviderConfig>,
    /// User's preferred model (unified field, replaces smart/eco/recovery)
    pub model: Option<String>,
    /// Optional system prompt override for sessions using this profile.
    pub system_prompt: Option<String>,
    /// Optional max turn override for sessions using this profile.
    pub max_turns: Option<usize>,
    /// Unique ID for anonymous telemetry
    pub anonymous_id: Option<String>,
    /// Whether to collect telemetry data
    pub collect_telemetry: Option<bool>,
    /// Editor command
    pub editor: Option<String>,
    /// Recently used model IDs (most recent first)
    pub recent_models: Vec<String>,
}

impl AppConfig {
    /// Load configuration from file.
    pub fn load<P: AsRef<Path>>(
        profile_name: &str,
        custom_config_path: Option<P>,
    ) -> Result<Self, ConfigError> {
        // Don't allow "all" as a profile to be loaded directly
        Self::validate_profile_name(profile_name)?;

        let config_path = Self::get_config_path(custom_config_path);
        // Try to load existing config file
        let mut config_file = Self::load_config_file(&config_path)?;
        let is_config_dirty = config_file.ensure_readonly();
        let profile = config_file.resolved_profile_config(profile_name)?;

        if is_config_dirty {
            // fail without crashing, because it's not critical
            if let Err(e) = config_file.save_to(&config_path) {
                eprintln!("Warning: Failed to update config on load: {}", e);
            }
        }

        Ok(Self::build(
            profile_name,
            config_path,
            config_file.settings,
            profile,
        ))
    }

    /// List all available profiles from config file.
    pub fn list_available_profiles<P: AsRef<Path>>(
        custom_config_path: Option<P>,
    ) -> Result<Vec<String>, String> {
        let config_path = Self::get_config_path(custom_config_path);
        let config_file = Self::load_config_file(&config_path).map_err(|e| format!("{}", e))?;
        let mut profiles: Vec<String> = config_file
            .profiles
            .keys()
            .filter(|name| name.as_str() != "all") // Skip the "all" meta-profile
            .cloned()
            .collect();

        if profiles.is_empty() {
            return Err("No profiles found in config file".to_string());
        }

        profiles.sort();
        Ok(profiles)
    }

    /// Save the current configuration to file.
    pub fn save(&self) -> Result<(), String> {
        // Load existing config or create new one
        let config_path = PathBuf::from(&self.config_path);
        let mut config_file = Self::load_config_file(&config_path).unwrap_or_default();
        config_file.insert_app_config(self.clone());
        config_file.set_app_config_settings(self.clone());

        if let Some(parent) = config_path.parent() {
            create_dir_all(parent).map_err(|e| format!("{}", e))?;
        }

        let config_str = toml::to_string_pretty(&config_file).map_err(|e| format!("{}", e))?;
        write(&self.config_path, config_str).map_err(|e| format!("{}", e))
    }

    /// Build an AppConfig from its components.
    pub(crate) fn build(
        profile_name: &str,
        path: PathBuf,
        settings: Settings,
        mut profile_config: ProfileConfig,
    ) -> Self {
        // Migrate any legacy provider fields to the unified providers HashMap
        profile_config.migrate_legacy_providers();
        // Migrate any legacy model fields to unified 'model' field
        profile_config.migrate_model_fields();
        // Normalize old-format recent_models entries and ensure config model is included
        profile_config.migrate_recent_models();

        AppConfig {
            api_endpoint: std::env::var("STAKPAK_API_ENDPOINT").unwrap_or(
                profile_config
                    .api_endpoint
                    .unwrap_or_else(|| STAKPAK_API_ENDPOINT.into()),
            ),
            api_key: std::env::var("STAKPAK_API_KEY")
                .ok()
                .or(profile_config.api_key),
            mcp_server_host: None,
            machine_name: settings.machine_name,
            auto_append_gitignore: settings.auto_append_gitignore,
            profile_name: profile_name.to_string(),
            config_path: path.display().to_string(),
            allowed_tools: profile_config.allowed_tools,
            auto_approve: profile_config.auto_approve,
            rulebooks: profile_config.rulebooks,
            warden: profile_config.warden,
            provider: profile_config.provider.unwrap_or(ProviderType::Remote),
            providers: profile_config.providers,
            model: profile_config.model,
            system_prompt: profile_config.system_prompt,
            max_turns: profile_config.max_turns,
            anonymous_id: settings.anonymous_id,
            collect_telemetry: settings.collect_telemetry,
            editor: settings.editor,
            recent_models: profile_config.recent_models,
        }
    }

    /// Get the config file path, using custom path or default.
    pub fn get_config_path<P: AsRef<Path>>(path: Option<P>) -> PathBuf {
        match path {
            Some(p) => p.as_ref().to_path_buf(),
            None => std::env::home_dir()
                .unwrap_or_default()
                .join(STAKPAK_CONFIG_PATH),
        }
    }

    /// Migrate old config format to new format.
    pub(crate) fn migrate_old_config<P: AsRef<Path>>(
        config_path: P,
        content: &str,
    ) -> Result<ConfigFile, ConfigError> {
        let old_config = toml::from_str::<OldAppConfig>(content).map_err(|e| {
            ConfigError::Message(format!(
                "Failed to parse config file in both old and new formats: {}",
                e
            ))
        })?;
        let config_file = old_config.into();

        toml::to_string_pretty(&config_file)
            .map_err(|e| {
                ConfigError::Message(format!("Failed to serialize migrated config: {}", e))
            })
            .and_then(|config_str| {
                write(config_path, config_str).map_err(|e| {
                    ConfigError::Message(format!("Failed to save migrated config: {}", e))
                })
            })?;

        Ok(config_file)
    }

    /// Load config file from disk.
    pub(crate) fn load_config_file<P: AsRef<Path>>(
        config_path: P,
    ) -> Result<ConfigFile, ConfigError> {
        match std::fs::read_to_string(config_path.as_ref()) {
            Ok(content) => {
                Self::validate_removed_openai_provider_fields(&content)?;

                let config_file = toml::from_str::<ConfigFile>(&content).or_else(|e| {
                    println!("Failed to parse config file in new format: {}", e);
                    Self::migrate_old_config(config_path.as_ref(), &content)
                })?;

                // Migrate any legacy provider configs to new unified providers format
                Self::migrate_legacy_provider_configs(config_path.as_ref(), config_file)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ConfigFile::with_default_profile()),
            Err(e) => Err(ConfigError::Message(format!(
                "Failed to read config file: {}",
                e
            ))),
        }
    }

    fn validate_removed_openai_provider_fields(content: &str) -> Result<(), ConfigError> {
        let value = match content.parse::<toml::Value>() {
            Ok(value) => value,
            Err(_) => return Ok(()),
        };

        let Some(profiles) = value.get("profiles").and_then(toml::Value::as_table) else {
            return Ok(());
        };

        for (profile_name, profile) in profiles {
            if let Some(openai) = profile.get("openai").and_then(toml::Value::as_table) {
                for removed_field in ["custom_headers", "use_responses_api"] {
                    if openai.contains_key(removed_field) {
                        return Err(ConfigError::Message(format!(
                            "profiles.{profile_name}.openai.{removed_field} has been removed; update the OpenAI provider config"
                        )));
                    }
                }
            }

            if let Some(openai) = profile
                .get("providers")
                .and_then(toml::Value::as_table)
                .and_then(|providers| providers.get("openai"))
                .and_then(toml::Value::as_table)
            {
                for removed_field in ["custom_headers", "use_responses_api"] {
                    if openai.contains_key(removed_field) {
                        return Err(ConfigError::Message(format!(
                            "profiles.{profile_name}.providers.openai.{removed_field} has been removed; update the OpenAI provider config"
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Migrate legacy provider configs (openai, anthropic, gemini)
    /// to the new unified `providers` HashMap format.
    /// Also migrates auth.toml to config.toml, model fields, and ensures settings have default values.
    fn migrate_legacy_provider_configs<P: AsRef<Path>>(
        config_path: P,
        mut config_file: ConfigFile,
    ) -> Result<ConfigFile, ConfigError> {
        let mut any_migrated = false;

        // Migrate auth.toml to config.toml
        if Self::migrate_auth_file(config_path.as_ref(), &mut config_file)? {
            any_migrated = true;
        }

        for (_profile_name, profile) in config_file.profiles.iter_mut() {
            // Migrate legacy provider fields
            if profile.needs_provider_migration() {
                profile.migrate_legacy_providers();
                any_migrated = true;
            }
            // Migrate legacy model fields (smart_model, eco_model, recovery_model -> model)
            if profile.needs_model_migration() {
                profile.migrate_model_fields();
                any_migrated = true;
            }
            // Normalize old-format recent_models entries and persist to disk
            if profile.migrate_recent_models() {
                any_migrated = true;
            }
        }

        // Ensure editor setting has a default value
        if config_file.settings.editor.is_none() {
            config_file.settings.editor = Some("nano".to_string());
            any_migrated = true;
        }

        // Save if any setting was migrated or added
        if any_migrated {
            config_file.save_to(config_path.as_ref())?;
        }

        Ok(config_file)
    }

    /// Migrate auth.toml credentials into config.toml.
    ///
    /// This merges all credentials from auth.toml into the config file's
    /// provider configurations, then backs up auth.toml to auth.toml.bak.
    ///
    /// Returns true if migration was performed.
    fn migrate_auth_file<P: AsRef<Path>>(
        config_path: P,
        config_file: &mut ConfigFile,
    ) -> Result<bool, ConfigError> {
        let config_dir = config_path
            .as_ref()
            .parent()
            .ok_or_else(|| ConfigError::Message("Invalid config path".into()))?;
        let auth_path = config_dir.join("auth.toml");

        // Skip if auth.toml doesn't exist or isn't a file
        if !auth_path.is_file() {
            return Ok(false);
        }

        // Load auth.toml
        let auth_manager = AuthManager::new(config_dir).map_err(|e| {
            ConfigError::Message(format!("Failed to load auth.toml for migration: {}", e))
        })?;

        // Skip if no credentials to migrate
        if !auth_manager.has_credentials() {
            return Ok(false);
        }

        // Merge credentials into config file
        for (profile_name, providers) in auth_manager.list() {
            for (provider_name, auth) in providers {
                // Get or create profile
                let profile = config_file
                    .profiles
                    .entry(profile_name.clone())
                    .or_default();

                // Get or create provider config
                let provider_config = profile
                    .providers
                    .entry(provider_name.clone())
                    .or_insert_with(|| {
                        // Create empty provider config for this provider type
                        ProviderConfig::empty_for_provider(provider_name).unwrap_or_else(|| {
                            // For unknown providers, create a custom provider with empty endpoint
                            // This shouldn't happen in practice since auth.toml only has known providers
                            ProviderConfig::Custom {
                                api_key: None,
                                api_endpoint: String::new(),
                                auth: None,
                            }
                        })
                    });

                // Set auth on provider config
                provider_config.set_auth(auth.clone());
            }
        }

        // Backup auth.toml — credentials are now in config.toml
        let backup_path = auth_path.with_extension("toml.bak");
        if let Err(e) = std::fs::rename(&auth_path, &backup_path) {
            // Log warning but don't fail migration
            eprintln!(
                "Warning: Failed to backup auth.toml to auth.toml.bak: {}",
                e
            );
        } else {
            eprintln!(
                "Migrated credentials from auth.toml to config.toml. \
                 Backup saved to auth.toml.bak — you can safely delete it."
            );
        }

        Ok(true)
    }

    fn validate_profile_name(profile_name: &str) -> Result<(), ConfigError> {
        if profile_name == "all" {
            Err(ConfigError::Message(
                "Cannot use 'all' as a profile name. It's reserved for defaults.".into(),
            ))
        } else {
            Ok(())
        }
    }

    /// Get the config directory from the config path.
    pub fn get_config_dir(&self) -> PathBuf {
        if !self.config_path.is_empty() {
            let path = PathBuf::from(&self.config_path);
            if let Some(parent) = path.parent() {
                return parent.to_path_buf();
            }
        }
        // Default to ~/.stakpak/
        std::env::home_dir().unwrap_or_default().join(".stakpak")
    }

    /// Resolve provider credentials with fallback chain.
    ///
    /// Resolution order:
    /// 1. config.toml -> [profiles.{profile}.providers.{provider}.auth] (new format)
    /// 2. config.toml -> [profiles.{profile}.providers.{provider}].api_key (legacy)
    /// 3. auth.toml -> [{profile}.{provider}] (legacy, for migration period)
    /// 4. auth.toml -> [all.{provider}] (legacy fallback)
    /// 5. Environment variable (e.g., ANTHROPIC_API_KEY)
    pub fn resolve_provider_auth(&self, provider: &str) -> Option<ProviderAuth> {
        // 1 & 2: Check config.toml providers HashMap (get_auth checks auth field then legacy fields)
        if let Some(provider_config) = self.providers.get(provider)
            && let Some(auth) = provider_config.get_auth()
        {
            return Some(auth);
        }

        // 3 & 4: Check auth.toml (legacy, for users who haven't migrated yet)
        // This fallback will be removed in a future version
        let config_dir = self.get_config_dir();
        if let Ok(auth_manager) = AuthManager::new(&config_dir)
            && let Some(auth) = auth_manager.get(&self.profile_name, provider)
        {
            return Some(auth.clone());
        }

        // 5: Check environment variable
        let env_var = match provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "gemini" => "GEMINI_API_KEY",
            _ => return None,
        };

        if let Ok(key) = std::env::var(env_var)
            && !key.is_empty()
        {
            return Some(ProviderAuth::api_key(key));
        }

        None
    }

    /// Check if OAuth tokens need refresh and refresh them if needed.
    pub async fn refresh_provider_auth_if_needed(
        &self,
        provider: &str,
        auth: &ProviderAuth,
    ) -> Result<ProviderAuth, String> {
        if !auth.needs_refresh() {
            return Ok(auth.clone());
        }

        // Only OAuth tokens need refresh
        let refresh_token = match auth.refresh_token() {
            Some(token) => token,
            None => return Ok(auth.clone()), // API keys don't need refresh
        };

        // Get OAuth provider for refresh
        use stakpak_shared::oauth::{OAuthFlow, ProviderRegistry};

        let registry = ProviderRegistry::new();
        let oauth_provider = registry
            .get(provider)
            .ok_or_else(|| format!("Unknown provider: {}", provider))?;

        // Get OAuth config for the provider's default subscription flow.
        let method_id = match provider {
            "anthropic" => "claude-max",
            "openai" => "chatgpt-plus-pro",
            _ => return Err(format!("OAuth refresh not implemented for {}", provider)),
        };

        let oauth_config = oauth_provider
            .oauth_config(method_id)
            .ok_or("OAuth not supported for this method")?;

        // Refresh the token
        let flow = OAuthFlow::new(oauth_config);
        let tokens = flow.refresh_token(refresh_token).await.map_err(|e| {
            format!(
                "Token refresh failed: {}. Please re-authenticate with 'stakpak auth login'.",
                e
            )
        })?;

        // Create new auth with updated tokens
        let new_expires = chrono::Utc::now().timestamp_millis() + (tokens.expires_in * 1000);
        let new_auth = if let Some(name) = auth.subscription_name() {
            ProviderAuth::oauth_with_name(
                &tokens.access_token,
                &tokens.refresh_token,
                new_expires,
                name,
            )
        } else {
            ProviderAuth::oauth(&tokens.access_token, &tokens.refresh_token, new_expires)
        };

        // Save the updated tokens to config.toml
        if let Err(e) = self.save_provider_auth(provider, new_auth.clone()) {
            // Log but don't fail - the tokens are still valid for this session
            tracing::warn!("Failed to save refreshed tokens: {}", e);
        }

        Ok(new_auth)
    }

    /// Save provider auth credentials to config.toml.
    ///
    /// This updates the provider's auth field in the current profile and saves
    /// the config file with 0600 permissions.
    pub fn save_provider_auth(&self, provider: &str, auth: ProviderAuth) -> Result<(), String> {
        let config_path = PathBuf::from(&self.config_path);
        let mut config_file = Self::load_config_file(&config_path).map_err(|e| format!("{}", e))?;

        // Get or create the profile
        let profile = config_file
            .profiles
            .entry(self.profile_name.clone())
            .or_default();

        // Get or create the provider config
        let provider_config = profile
            .providers
            .entry(provider.to_string())
            .or_insert_with(|| {
                ProviderConfig::empty_for_provider(provider).unwrap_or_else(|| {
                    ProviderConfig::Custom {
                        api_key: None,
                        api_endpoint: String::new(),
                        auth: None,
                    }
                })
            });

        // Set the auth
        provider_config.set_auth(auth);

        // Save config file (this sets 0600 permissions)
        config_file
            .save_to(&config_path)
            .map_err(|e| format!("{}", e))
    }

    /// Get Anthropic config with resolved credentials.
    pub fn get_anthropic_config_with_auth(&self) -> Option<AnthropicConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Anthropic { api_endpoint, .. }) =
            self.providers.get("anthropic")
        {
            // Use resolve_provider_auth which checks auth field, legacy fields, auth.toml, env vars
            if let Some(auth) = self.resolve_provider_auth("anthropic") {
                let mut config = AnthropicConfig::from_provider_auth(&auth);
                config.api_endpoint = api_endpoint.clone();
                return Some(config);
            }
            return None;
        }

        // Fall back to resolve_provider_auth only (handles auth.toml, env vars)
        self.resolve_provider_auth("anthropic")
            .map(|auth| AnthropicConfig::from_provider_auth(&auth))
    }

    /// Get Anthropic config with resolved credentials, refreshing OAuth tokens if needed.
    pub async fn get_anthropic_config_with_auth_async(&self) -> Option<AnthropicConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Anthropic { api_endpoint, .. }) =
            self.providers.get("anthropic")
        {
            if let Some(auth) = self.resolve_provider_auth("anthropic") {
                let auth = match self
                    .refresh_provider_auth_if_needed("anthropic", &auth)
                    .await
                {
                    Ok(refreshed_auth) => refreshed_auth,
                    Err(e) => {
                        eprintln!(
                            "\x1b[33mWarning: Failed to refresh Anthropic token: {}\x1b[0m",
                            e
                        );
                        auth
                    }
                };
                let mut config = AnthropicConfig::from_provider_auth(&auth);
                config.api_endpoint = api_endpoint.clone();
                return Some(config);
            }
            return None;
        }

        // Fall back to resolve_provider_auth only (with refresh)
        if let Some(auth) = self.resolve_provider_auth("anthropic") {
            let auth = match self
                .refresh_provider_auth_if_needed("anthropic", &auth)
                .await
            {
                Ok(refreshed_auth) => refreshed_auth,
                Err(e) => {
                    eprintln!(
                        "\x1b[33mWarning: Failed to refresh Anthropic token: {}\x1b[0m",
                        e
                    );
                    auth
                }
            };
            return Some(AnthropicConfig::from_provider_auth(&auth));
        }

        None
    }

    fn build_openai_provider_config_with_auth(&self, auth: ProviderAuth) -> ProviderConfig {
        let api_endpoint = match self.providers.get("openai") {
            Some(ProviderConfig::OpenAI { api_endpoint, .. }) => api_endpoint.clone(),
            _ => None,
        };

        ProviderConfig::OpenAI {
            api_key: None,
            api_endpoint,
            auth: Some(auth),
        }
    }

    fn get_openai_provider_config_with_auth(&self) -> Option<ProviderConfig> {
        self.resolve_provider_auth("openai")
            .map(|auth| self.build_openai_provider_config_with_auth(auth))
    }

    async fn get_openai_provider_config_with_auth_async(&self) -> Option<ProviderConfig> {
        let auth = if let Some(auth) = self.resolve_provider_auth("openai") {
            match self.refresh_provider_auth_if_needed("openai", &auth).await {
                Ok(refreshed_auth) => Some(refreshed_auth),
                Err(e) => {
                    eprintln!(
                        "\x1b[33mWarning: Failed to refresh OpenAI token: {}\x1b[0m",
                        e
                    );
                    Some(auth)
                }
            }
        } else {
            None
        }?;

        Some(self.build_openai_provider_config_with_auth(auth))
    }

    /// Get Gemini config with resolved credentials.
    pub fn get_gemini_config_with_auth(&self) -> Option<GeminiConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Gemini { api_endpoint, .. }) = self.providers.get("gemini") {
            if let Some(auth) = self.resolve_provider_auth("gemini") {
                let mut config = GeminiConfig::from_provider_auth(&auth).unwrap_or(GeminiConfig {
                    api_key: None,
                    api_endpoint: None,
                });
                config.api_endpoint = api_endpoint.clone();
                return Some(config);
            }
            return None;
        }

        // Fall back to resolve_provider_auth only
        self.resolve_provider_auth("gemini")
            .and_then(|auth| GeminiConfig::from_provider_auth(&auth))
    }

    /// Get Gemini config with resolved credentials, refreshing OAuth tokens if needed.
    pub async fn get_gemini_config_with_auth_async(&self) -> Option<GeminiConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Gemini { api_endpoint, .. }) = self.providers.get("gemini") {
            if let Some(auth) = self.resolve_provider_auth("gemini") {
                let auth = match self.refresh_provider_auth_if_needed("gemini", &auth).await {
                    Ok(refreshed_auth) => refreshed_auth,
                    Err(e) => {
                        eprintln!(
                            "\x1b[33mWarning: Failed to refresh Gemini token: {}\x1b[0m",
                            e
                        );
                        auth
                    }
                };
                let mut config = GeminiConfig::from_provider_auth(&auth).unwrap_or(GeminiConfig {
                    api_key: None,
                    api_endpoint: None,
                });
                config.api_endpoint = api_endpoint.clone();
                return Some(config);
            }
            return None;
        }

        // Fall back to resolve_provider_auth only (with refresh)
        if let Some(auth) = self.resolve_provider_auth("gemini") {
            let auth = match self.refresh_provider_auth_if_needed("gemini", &auth).await {
                Ok(refreshed_auth) => refreshed_auth,
                Err(e) => {
                    eprintln!(
                        "\x1b[33mWarning: Failed to refresh Gemini token: {}\x1b[0m",
                        e
                    );
                    auth
                }
            };
            return GeminiConfig::from_provider_auth(&auth);
        }

        None
    }

    /// Add custom providers (non-built-in) from the providers HashMap.
    fn add_custom_providers(&self, config: &mut LLMProviderConfig) {
        for (name, provider_config) in &self.providers {
            if !matches!(
                name.as_str(),
                "openai" | "anthropic" | "gemini" | "amazon-bedrock"
            ) {
                config.add_provider(name, provider_config.clone());
            }
        }
    }

    /// Add built-in providers to config if credentials are available.
    fn add_builtin_providers(
        &self,
        config: &mut LLMProviderConfig,
        openai: Option<ProviderConfig>,
        anthropic: Option<AnthropicConfig>,
        gemini: Option<GeminiConfig>,
    ) {
        if let Some(openai) = openai {
            config.add_provider("openai", openai);
        }
        if let Some(anthropic) = anthropic {
            config.add_provider(
                "anthropic",
                ProviderConfig::Anthropic {
                    api_key: anthropic.api_key,
                    api_endpoint: anthropic.api_endpoint,
                    access_token: anthropic.access_token,
                    auth: None, // Auth is already resolved into api_key/access_token
                },
            );
        }
        if let Some(gemini) = gemini {
            config.add_provider(
                "gemini",
                ProviderConfig::Gemini {
                    api_key: gemini.api_key,
                    api_endpoint: gemini.api_endpoint,
                    auth: None, // Auth is already resolved into api_key
                },
            );
        }
        // Bedrock uses AWS credential chain — no API key resolution needed.
        // Just pass through the config if present.
        if let Some(bedrock) = self.get_bedrock_config() {
            config.add_provider("amazon-bedrock", bedrock);
        }
    }

    /// Get Bedrock provider config if configured.
    ///
    /// Unlike other providers, Bedrock does not need credential resolution —
    /// authentication is handled by the AWS credential chain (env vars, shared
    /// credentials, SSO, instance roles).
    pub fn get_bedrock_config(&self) -> Option<ProviderConfig> {
        self.providers
            .get("amazon-bedrock")
            .filter(|p| matches!(p, ProviderConfig::Bedrock { .. }))
            .cloned()
    }

    /// Build LLMProviderConfig from the app configuration.
    pub fn get_llm_provider_config(&self) -> LLMProviderConfig {
        let mut config = LLMProviderConfig::new();

        self.add_custom_providers(&mut config);
        self.add_builtin_providers(
            &mut config,
            self.get_openai_provider_config_with_auth(),
            self.get_anthropic_config_with_auth(),
            self.get_gemini_config_with_auth(),
        );

        config
    }

    /// Build LLMProviderConfig from the app configuration (async version with OAuth refresh).
    pub async fn get_llm_provider_config_async(&self) -> LLMProviderConfig {
        let mut config = LLMProviderConfig::new();

        self.add_custom_providers(&mut config);
        self.add_builtin_providers(
            &mut config,
            self.get_openai_provider_config_with_auth_async().await,
            self.get_anthropic_config_with_auth_async().await,
            self.get_gemini_config_with_auth_async().await,
        );

        config
    }

    /// Get Stakpak API key with resolved credentials from auth.toml fallback chain.
    /// Returns None if the API key is empty or not set.
    pub fn get_stakpak_api_key(&self) -> Option<String> {
        if let Some(ref key) = self.api_key
            && !key.is_empty()
        {
            return Some(key.clone());
        }

        if let Some(ProviderAuth::Api { key }) = self.resolve_provider_auth("stakpak")
            && !key.is_empty()
        {
            return Some(key);
        }

        None
    }

    /// Get auth display info for the TUI.
    pub fn get_auth_display_info(&self) -> (Option<String>, Option<String>, Option<String>) {
        if matches!(self.provider, ProviderType::Remote) {
            return (None, None, None);
        }

        let config_provider = Some("Local".to_string());
        let builtin_providers = ["anthropic", "openai", "gemini"];

        for provider_name in builtin_providers {
            if let Some(auth) = self.resolve_provider_auth(provider_name) {
                let base_name = match provider_name {
                    "anthropic" => "Anthropic",
                    "openai" => "OpenAI",
                    "gemini" => "Gemini",
                    _ => provider_name,
                };

                // Check if provider has a custom endpoint
                let has_custom_endpoint = self
                    .providers
                    .get(provider_name)
                    .map(|p| p.api_endpoint().is_some())
                    .unwrap_or(false);

                let auth_provider = if has_custom_endpoint {
                    format!("{} BYOM", base_name)
                } else {
                    base_name.to_string()
                };

                let subscription_name = auth.subscription_name().map(|s| s.to_string());

                return (config_provider, Some(auth_provider), subscription_name);
            }
        }

        // Check custom providers
        for name in self.providers.keys() {
            if !builtin_providers.contains(&name.as_str()) {
                return (config_provider, Some(name.clone()), None);
            }
        }

        (config_provider, None, None)
    }

    /// Get the default Model from config
    ///
    /// Uses the `model` field if set, otherwise falls back to a default Claude Opus model.
    ///
    /// If `cli_override` is provided, it takes highest priority over config values.
    ///
    /// Searches the model catalog by ID. If the model string has a provider
    /// prefix (e.g., "anthropic/claude-opus-4-5"), it searches within that
    /// provider first. Otherwise, it searches all providers.
    pub fn get_default_model(&self, cli_override: Option<&str>) -> stakpak_api::Model {
        let has_stakpak_key = self.get_stakpak_api_key().is_some();

        // Priority: cli_override > recent_models[0] > model > default
        // The most recently used model takes precedence over the static config model,
        // so re-opening stakpak continues with the last model you were using.
        let most_recent = self.recent_models.first().map(|s| s.as_str());
        let model_str = cli_override
            .or(most_recent)
            .or(self.model.as_deref())
            .unwrap_or("claude-opus-4-6");

        // Extract explicit provider prefix if present (e.g., "amazon-bedrock/claude-sonnet-4-5")
        let explicit_provider = model_str.find('/').map(|idx| &model_str[..idx]);

        // First, find the model without Stakpak transform to determine its native provider
        let model = stakpak_api::find_model(model_str, false).unwrap_or_else(|| {
            // Model not found in catalog - create a custom model
            // Extract provider from prefix if present
            let (provider, model_id) = if let Some(idx) = model_str.find('/') {
                let (prefix, rest) = model_str.split_at(idx);
                (prefix, &rest[1..])
            } else {
                // Default to stakpak (which can resolve arbitrary model names)
                // if the user has a Stakpak key; otherwise fall back to anthropic
                let default_provider = if has_stakpak_key {
                    "stakpak"
                } else {
                    "anthropic"
                };
                (default_provider, model_str)
            };

            stakpak_api::Model::custom(model_id.to_string(), provider)
        });

        // If the user specified an explicit provider prefix (e.g., "amazon-bedrock/..."),
        // ensure the resolved model uses that provider — the catalog may have returned
        // the model under a different provider (e.g., "anthropic" instead of "amazon-bedrock").
        let model = if let Some(prefix) = explicit_provider
            && model.provider != prefix
        {
            stakpak_api::Model {
                provider: prefix.to_string(),
                ..model
            }
        } else {
            model
        };

        // Transform for Stakpak routing only if:
        // 1. User has a Stakpak API key
        // 2. The model is from a known cloud provider (not custom/ollama/litellm)
        // 3. User does NOT have a direct API key for this model's provider
        // If the user has a direct provider key, use it instead of routing through Stakpak.
        // NOTE: keep in sync with known_cloud_providers in mode_interactive.rs SwitchToModel handler
        let known_cloud_providers = ["anthropic", "openai", "google", "gemini", "amazon-bedrock"];
        let has_direct_provider_key = self.resolve_provider_auth(&model.provider).is_some();
        if has_stakpak_key
            && !has_direct_provider_key
            && model.provider != "stakpak"
            && known_cloud_providers.contains(&model.provider.as_str())
        {
            return stakpak_api::transform_for_stakpak(model);
        }

        model
    }
}

// Conversions

impl From<AppConfig> for Settings {
    fn from(config: AppConfig) -> Self {
        Settings {
            machine_name: config.machine_name,
            auto_append_gitignore: config.auto_append_gitignore,
            anonymous_id: config.anonymous_id,
            collect_telemetry: config.collect_telemetry,
            editor: config.editor,
        }
    }
}

impl From<AppConfig> for ProfileConfig {
    fn from(config: AppConfig) -> Self {
        ProfileConfig {
            api_endpoint: Some(config.api_endpoint),
            api_key: config.api_key,
            allowed_tools: config.allowed_tools,
            auto_approve: config.auto_approve,
            rulebooks: config.rulebooks,
            warden: config.warden,
            provider: Some(config.provider),
            providers: config.providers,
            model: config.model,
            recent_models: config.recent_models,
            system_prompt: config.system_prompt,
            max_turns: config.max_turns,
            // Legacy fields - not used in new format
            openai: None,
            anthropic: None,
            gemini: None,
            eco_model: None,
            smart_model: None,
            recovery_model: None,
        }
    }
}

impl From<ConfigFile> for AppConfig {
    fn from(file: ConfigFile) -> Self {
        let profile_name = "default";
        let profile = file.profiles.get(profile_name).cloned().unwrap_or_default();
        Self::build(
            "default",
            PathBuf::from(STAKPAK_CONFIG_PATH),
            file.settings,
            profile,
        )
    }
}
