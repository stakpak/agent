//! Main application configuration.

use config::ConfigError;
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::models::integrations::anthropic::AnthropicConfig;
use stakpak_shared::models::integrations::gemini::GeminiConfig;
use stakpak_shared::models::integrations::openai::OpenAIConfig;
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
    /// Smart (capable) model name
    pub smart_model: Option<String>,
    /// Eco (fast/cheap) model name
    pub eco_model: Option<String>,
    /// Recovery model name
    pub recovery_model: Option<String>,
    /// New unified model field (replaces smart/eco/recovery model selection)
    pub model: Option<String>,
    /// Unique ID for anonymous telemetry
    pub anonymous_id: Option<String>,
    /// Whether to collect telemetry data
    pub collect_telemetry: Option<bool>,
    /// Editor command
    pub editor: Option<String>,
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
            smart_model: profile_config.smart_model,
            eco_model: profile_config.eco_model,
            recovery_model: profile_config.recovery_model,
            model: profile_config.model,
            anonymous_id: settings.anonymous_id,
            collect_telemetry: settings.collect_telemetry,
            editor: settings.editor,
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

    /// Migrate legacy provider configs (openai, anthropic, gemini)
    /// to the new unified `providers` HashMap format.
    /// Also ensures settings have default values (e.g., editor).
    fn migrate_legacy_provider_configs<P: AsRef<Path>>(
        config_path: P,
        mut config_file: ConfigFile,
    ) -> Result<ConfigFile, ConfigError> {
        let mut any_migrated = false;

        for (_profile_name, profile) in config_file.profiles.iter_mut() {
            if profile.needs_provider_migration() {
                profile.migrate_legacy_providers();
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
            toml::to_string_pretty(&config_file)
                .map_err(|e| {
                    ConfigError::Message(format!(
                        "Failed to serialize config after migration: {}",
                        e
                    ))
                })
                .and_then(|config_str| {
                    write(config_path, config_str).map_err(|e| {
                        ConfigError::Message(format!(
                            "Failed to save config after migration: {}",
                            e
                        ))
                    })
                })?;
        }

        Ok(config_file)
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
    /// 1. auth.toml -> [{profile}.{provider}] (profile-specific)
    /// 2. auth.toml -> [all.{provider}] (shared fallback)
    /// 3. config.toml -> [profiles.{profile}.providers.{provider}].api_key
    /// 4. Environment variable (e.g., ANTHROPIC_API_KEY)
    pub fn resolve_provider_auth(&self, provider: &str) -> Option<ProviderAuth> {
        let config_dir = self.get_config_dir();

        // 1 & 2: Check auth.toml (handles profile inheritance internally)
        if let Ok(auth_manager) = AuthManager::new(&config_dir)
            && let Some(auth) = auth_manager.get(&self.profile_name, provider)
        {
            return Some(auth.clone());
        }

        // 3: Check config.toml providers HashMap
        if let Some(provider_config) = self.providers.get(provider) {
            // Check for API key in provider config
            if let Some(key) = provider_config.api_key()
                && !key.is_empty()
            {
                return Some(ProviderAuth::api_key(key));
            }
            // Check for access token (Anthropic OAuth)
            if let Some(token) = provider_config.access_token()
                && !token.is_empty()
            {
                // For OAuth tokens, we'd need more info, but for now just treat as API key
                return Some(ProviderAuth::api_key(token));
            }
        }

        // 4: Check environment variable
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

        // Get OAuth config (use claude-max as default for Anthropic)
        let method_id = match provider {
            "anthropic" => "claude-max",
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
        let new_auth =
            ProviderAuth::oauth(&tokens.access_token, &tokens.refresh_token, new_expires);

        // Save the updated tokens
        let config_dir = self.get_config_dir();
        if let Ok(mut auth_manager) = AuthManager::new(&config_dir)
            && let Err(e) = auth_manager.set(&self.profile_name, provider, new_auth.clone())
        {
            // Log but don't fail - the tokens are still valid for this session
            tracing::warn!("Failed to save refreshed tokens: {}", e);
        }

        Ok(new_auth)
    }

    /// Get Anthropic config with resolved credentials from auth.toml fallback chain.
    pub fn get_anthropic_config_with_auth(&self) -> Option<AnthropicConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Anthropic {
            api_key,
            api_endpoint,
            access_token,
        }) = self.providers.get("anthropic")
        {
            let mut config = AnthropicConfig {
                api_key: api_key.clone(),
                api_endpoint: api_endpoint.clone(),
                access_token: access_token.clone(),
            };
            // Override with auth.toml if available
            if let Some(auth) = self.resolve_provider_auth("anthropic") {
                config = config.with_provider_auth(&auth);
            }
            return Some(config);
        }

        // Fall back to auth.toml only
        if let Some(auth) = self.resolve_provider_auth("anthropic") {
            return Some(AnthropicConfig::from_provider_auth(&auth));
        }

        None
    }

    /// Get Anthropic config with resolved credentials, refreshing OAuth tokens if needed.
    pub async fn get_anthropic_config_with_auth_async(&self) -> Option<AnthropicConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Anthropic {
            api_key,
            api_endpoint,
            access_token,
        }) = self.providers.get("anthropic")
        {
            let mut config = AnthropicConfig {
                api_key: api_key.clone(),
                api_endpoint: api_endpoint.clone(),
                access_token: access_token.clone(),
            };
            // Override with auth.toml if available (with refresh)
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
                config = config.with_provider_auth(&auth);
            }
            return Some(config);
        }

        // Fall back to auth.toml only (with refresh)
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

    /// Get OpenAI config with resolved credentials from auth.toml fallback chain.
    pub fn get_openai_config_with_auth(&self) -> Option<OpenAIConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::OpenAI {
            api_key,
            api_endpoint,
        }) = self.providers.get("openai")
        {
            let config = OpenAIConfig {
                api_key: api_key.clone(),
                api_endpoint: api_endpoint.clone(),
            };
            // Override with auth.toml if available
            if let Some(auth) = self.resolve_provider_auth("openai") {
                return config.clone().with_provider_auth(&auth).or(Some(config));
            }
            return Some(config);
        }

        // Fall back to auth.toml only
        if let Some(auth) = self.resolve_provider_auth("openai") {
            return OpenAIConfig::from_provider_auth(&auth);
        }

        None
    }

    /// Get OpenAI config with resolved credentials, refreshing OAuth tokens if needed.
    pub async fn get_openai_config_with_auth_async(&self) -> Option<OpenAIConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::OpenAI {
            api_key,
            api_endpoint,
        }) = self.providers.get("openai")
        {
            let config = OpenAIConfig {
                api_key: api_key.clone(),
                api_endpoint: api_endpoint.clone(),
            };
            // Override with auth.toml if available (with refresh)
            if let Some(auth) = self.resolve_provider_auth("openai") {
                let auth = match self.refresh_provider_auth_if_needed("openai", &auth).await {
                    Ok(refreshed_auth) => refreshed_auth,
                    Err(e) => {
                        eprintln!(
                            "\x1b[33mWarning: Failed to refresh OpenAI token: {}\x1b[0m",
                            e
                        );
                        auth
                    }
                };
                return config.clone().with_provider_auth(&auth).or(Some(config));
            }
            return Some(config);
        }

        // Fall back to auth.toml only (with refresh)
        if let Some(auth) = self.resolve_provider_auth("openai") {
            let auth = match self.refresh_provider_auth_if_needed("openai", &auth).await {
                Ok(refreshed_auth) => refreshed_auth,
                Err(e) => {
                    eprintln!(
                        "\x1b[33mWarning: Failed to refresh OpenAI token: {}\x1b[0m",
                        e
                    );
                    auth
                }
            };
            return OpenAIConfig::from_provider_auth(&auth);
        }

        None
    }

    /// Get Gemini config with resolved credentials from auth.toml fallback chain.
    pub fn get_gemini_config_with_auth(&self) -> Option<GeminiConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Gemini {
            api_key,
            api_endpoint,
        }) = self.providers.get("gemini")
        {
            let config = GeminiConfig {
                api_key: api_key.clone(),
                api_endpoint: api_endpoint.clone(),
            };
            // Override with auth.toml if available
            if let Some(auth) = self.resolve_provider_auth("gemini") {
                return config.clone().with_provider_auth(&auth).or(Some(config));
            }
            return Some(config);
        }

        // Fall back to auth.toml only
        if let Some(auth) = self.resolve_provider_auth("gemini") {
            return GeminiConfig::from_provider_auth(&auth);
        }

        None
    }

    /// Get Gemini config with resolved credentials, refreshing OAuth tokens if needed.
    pub async fn get_gemini_config_with_auth_async(&self) -> Option<GeminiConfig> {
        // First check providers HashMap
        if let Some(ProviderConfig::Gemini {
            api_key,
            api_endpoint,
        }) = self.providers.get("gemini")
        {
            let config = GeminiConfig {
                api_key: api_key.clone(),
                api_endpoint: api_endpoint.clone(),
            };
            // Override with auth.toml if available (with refresh)
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
                return config.clone().with_provider_auth(&auth).or(Some(config));
            }
            return Some(config);
        }

        // Fall back to auth.toml only (with refresh)
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
        openai: Option<OpenAIConfig>,
        anthropic: Option<AnthropicConfig>,
        gemini: Option<GeminiConfig>,
    ) {
        if let Some(openai) = openai {
            config.add_provider(
                "openai",
                ProviderConfig::OpenAI {
                    api_key: openai.api_key,
                    api_endpoint: openai.api_endpoint,
                },
            );
        }
        if let Some(anthropic) = anthropic {
            config.add_provider(
                "anthropic",
                ProviderConfig::Anthropic {
                    api_key: anthropic.api_key,
                    api_endpoint: anthropic.api_endpoint,
                    access_token: anthropic.access_token,
                },
            );
        }
        if let Some(gemini) = gemini {
            config.add_provider(
                "gemini",
                ProviderConfig::Gemini {
                    api_key: gemini.api_key,
                    api_endpoint: gemini.api_endpoint,
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
            self.get_openai_config_with_auth(),
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
            self.get_openai_config_with_auth_async().await,
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
    /// Uses the `model` field if set, otherwise falls back to `smart_model`,
    /// and finally to a default Claude Opus model.
    ///
    /// If `cli_override` is provided, it takes highest priority over all config values.
    ///
    /// Searches the model catalog by ID. If the model string has a provider
    /// prefix (e.g., "anthropic/claude-opus-4-5"), it searches within that
    /// provider first. Otherwise, it searches all providers.
    pub fn get_default_model(&self, cli_override: Option<&str>) -> stakpak_api::Model {
        let use_stakpak = self.api_key.is_some();

        // Priority: cli_override > model > smart_model > default
        let model_str = cli_override
            .or(self.model.as_deref())
            .or(self.smart_model.as_deref())
            .unwrap_or("claude-opus-4-5");

        // Extract explicit provider prefix if present (e.g., "amazon-bedrock/claude-sonnet-4-5")
        let explicit_provider = model_str.find('/').map(|idx| &model_str[..idx]);

        // Search the model catalog
        let model = stakpak_api::find_model(model_str, use_stakpak).unwrap_or_else(|| {
            // Model not found in catalog - create a custom model
            // Extract provider from prefix if present
            let (provider, model_id) = if let Some(idx) = model_str.find('/') {
                let (prefix, rest) = model_str.split_at(idx);
                (prefix, &rest[1..])
            } else {
                ("anthropic", model_str) // Default to anthropic
            };

            let final_provider = if use_stakpak { "stakpak" } else { provider };
            let final_id = if use_stakpak {
                format!("{}/{}", provider, model_id)
            } else {
                model_id.to_string()
            };

            stakpak_api::Model::custom(final_id, final_provider)
        });

        // If the user specified an explicit provider prefix (e.g., "amazon-bedrock/..."),
        // ensure the resolved model uses that provider — the catalog may have returned
        // the model under a different provider (e.g., "anthropic" instead of "amazon-bedrock").
        if let Some(prefix) = explicit_provider
            && !use_stakpak
            && model.provider != prefix
        {
            return stakpak_api::Model {
                provider: prefix.to_string(),
                ..model
            };
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
            provider: None,
            providers: config.providers,
            // Legacy fields - not used in new format
            openai: None,
            anthropic: None,
            gemini: None,
            eco_model: config.eco_model,
            smart_model: config.smart_model,
            recovery_model: config.recovery_model,
            model: config.model,
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
