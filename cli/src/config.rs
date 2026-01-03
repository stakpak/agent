use config::ConfigError;
use serde::{Deserialize, Serialize};
use stakpak_api::{models::ListRuleBook, remote::ClientConfig};
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::models::integrations::anthropic::AnthropicConfig;
use stakpak_shared::models::integrations::gemini::GeminiConfig;
use stakpak_shared::models::integrations::openai::OpenAIConfig;
use std::collections::HashMap;
use std::fs::{create_dir_all, write};
use std::io;
use std::path::{Path, PathBuf};

const STAKPAK_API_ENDPOINT: &str = "https://apiv2.stakpak.dev";
const STAKPAK_CONFIG_PATH: &str = ".stakpak/config.toml";

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    #[default]
    Remote,
    Local,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RulebookConfig {
    /// Include only these rulebooks by URI (supports wildcards, empty = all allowed)
    pub include: Option<Vec<String>>,
    /// Exclude specific rulebooks (supports wildcards, empty = none excluded)
    pub exclude: Option<Vec<String>>,
    /// Filter by tags to include
    pub include_tags: Option<Vec<String>>,
    /// Filter by tags to exclude
    pub exclude_tags: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WardenConfig {
    pub enabled: bool,
    #[serde(default)]
    pub volumes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProfileConfig {
    pub api_endpoint: Option<String>,
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
    /// OpenAI configuration
    pub openai: Option<OpenAIConfig>,
    /// Gemini configuration
    pub gemini: Option<GeminiConfig>,
    /// Anthropic configuration
    pub anthropic: Option<AnthropicConfig>,
    pub eco_model: Option<String>,
    pub smart_model: Option<String>,
    pub recovery_model: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
    /// Unique ID for anonymous telemetry (formerly user_id)
    #[serde(alias = "user_id")]
    pub anonymous_id: Option<String>,
    pub collect_telemetry: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigFile {
    pub profiles: HashMap<String, ProfileConfig>,
    pub settings: Settings,
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub api_endpoint: String,
    pub api_key: Option<String>,
    pub provider: ProviderType,
    pub mcp_server_host: Option<String>,
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
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
    pub openai: Option<OpenAIConfig>,
    pub anthropic: Option<AnthropicConfig>,
    pub gemini: Option<GeminiConfig>,
    pub smart_model: Option<String>,
    pub eco_model: Option<String>,
    pub recovery_model: Option<String>,
    pub anonymous_id: Option<String>,
    pub collect_telemetry: Option<bool>,
}

#[derive(Deserialize, Clone)]
struct OldAppConfig {
    pub api_endpoint: String,
    pub api_key: Option<String>,
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
}

impl From<AppConfig> for ClientConfig {
    fn from(config: AppConfig) -> Self {
        ClientConfig {
            api_key: config.api_key.clone(),
            api_endpoint: config.api_endpoint.clone(),
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

impl From<AppConfig> for Settings {
    fn from(config: AppConfig) -> Self {
        Settings {
            machine_name: config.machine_name,
            auto_append_gitignore: config.auto_append_gitignore,
            anonymous_id: config.anonymous_id,
            collect_telemetry: config.collect_telemetry,
        }
    }
}

impl From<OldAppConfig> for Settings {
    fn from(old_config: OldAppConfig) -> Self {
        Settings {
            machine_name: old_config.machine_name,
            auto_append_gitignore: old_config.auto_append_gitignore,
            anonymous_id: Some(uuid::Uuid::new_v4().to_string()),
            collect_telemetry: Some(true),
        }
    }
}

impl From<OldAppConfig> for ConfigFile {
    // OldAppConfigConfig will always create a 'default' ConfigFile
    fn from(old_config: OldAppConfig) -> Self {
        let settings: Settings = old_config.clone().into();
        ConfigFile {
            profiles: HashMap::from([(
                "default".to_string(),
                ProfileConfig::migrated_from_old_config(old_config),
            )]),
            settings,
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
            openai: config.openai,
            anthropic: config.anthropic,
            gemini: config.gemini,
            eco_model: config.eco_model,
            smart_model: config.smart_model,
            recovery_model: config.recovery_model,
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

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            profiles: HashMap::new(),
            settings: Settings {
                machine_name: None,
                auto_append_gitignore: Some(true),
                anonymous_id: Some(uuid::Uuid::new_v4().to_string()),
                collect_telemetry: Some(true),
            },
        }
    }
}

impl ConfigFile {
    fn with_default_profile() -> Self {
        ConfigFile {
            profiles: HashMap::from([(
                "default".into(),
                ProfileConfig::with_api_endpoint(STAKPAK_API_ENDPOINT),
            )]),
            settings: Settings {
                machine_name: None,
                auto_append_gitignore: Some(true),
                anonymous_id: Some(uuid::Uuid::new_v4().to_string()),
                collect_telemetry: Some(true),
            },
        }
    }

    fn profile_config(&self, profile_name: &str) -> Option<&ProfileConfig> {
        self.profiles.get(profile_name)
    }

    fn profile_config_ok_or(&self, profile_name: &str) -> Result<ProfileConfig, ConfigError> {
        self.profile_config(profile_name).cloned().ok_or_else(|| {
            ConfigError::Message(format!(
                "Profile '{}' not found in configuration",
                profile_name
            ))
        })
    }

    // Get the specified profile
    // Get defaults from "all" profile if it exists
    // Apply inheritance: profile values override "all" profile values
    fn resolved_profile_config(&self, profile_name: &str) -> Result<ProfileConfig, ConfigError> {
        let profile = self.profile_config_ok_or(profile_name)?;
        Ok(profile.merge(self.profile_config("all")))
    }

    fn insert_app_config(&mut self, config: AppConfig) {
        self.profiles
            .insert(config.profile_name.clone(), config.into());
    }

    fn set_app_config_settings(&mut self, config: AppConfig) {
        // Preserve existing anonymous_id and collect_telemetry if AppConfig values are None
        let existing_anonymous_id = self.settings.anonymous_id.clone();
        let existing_collect_telemetry = self.settings.collect_telemetry;

        self.settings = Settings {
            machine_name: config.machine_name,
            auto_append_gitignore: config.auto_append_gitignore,
            anonymous_id: config.anonymous_id.or(existing_anonymous_id),
            collect_telemetry: config.collect_telemetry.or(existing_collect_telemetry),
        };
    }

    fn contains_readonly(&self) -> bool {
        self.profiles.contains_key("readonly")
    }

    fn ensure_readonly(&mut self) -> bool {
        if self.contains_readonly() {
            false
        } else {
            self.profiles.insert(
                "readonly".into(),
                ProfileConfig::readonly_profile(self.profile_config("default")),
            );
            true
        }
    }

    fn save_to<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        if let Some(parent) = path.as_ref().parent() {
            create_dir_all(parent).map_err(|e| {
                ConfigError::Message(format!("Failed to create config directory: {}", e))
            })?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Message(format!("Failed to serialize config file: {}", e)))?;
        write(path, body)
            .map_err(|e| ConfigError::Message(format!("Failed to write config file: {}", e)))
    }
}

impl WardenConfig {
    fn readonly_profile() -> Self {
        WardenConfig {
            enabled: true,
            volumes: vec![
                "~/.stakpak/config.toml:/home/agent/.stakpak/config.toml:ro".to_string(),
                "./:/agent:ro".to_string(),
                "./.stakpak:/agent/.stakpak".to_string(),
                "~/.aws:/home/agent/.aws:ro".to_string(),
                "~/.config/gcloud:/home/agent/.config/gcloud:ro".to_string(),
                "~/.digitalocean:/home/agent/.digitalocean:ro".to_string(),
                "~/.azure:/home/agent/.azure:ro".to_string(),
                "~/.kube:/home/agent/.kube:ro".to_string(),
            ],
        }
    }
}

impl ProfileConfig {
    fn with_api_endpoint(api_endpoint: &str) -> Self {
        ProfileConfig {
            api_endpoint: Some(api_endpoint.into()),
            ..ProfileConfig::default()
        }
    }

    fn readonly_profile(default_profile: Option<&ProfileConfig>) -> Self {
        ProfileConfig {
            api_endpoint: default_profile.and_then(|p| p.api_endpoint.clone()),
            api_key: default_profile.and_then(|p| p.api_key.clone()),
            warden: Some(WardenConfig::readonly_profile()),
            ..ProfileConfig::default()
        }
    }

    fn migrated_from_old_config(old_config: OldAppConfig) -> Self {
        ProfileConfig {
            api_endpoint: Some(old_config.api_endpoint),
            api_key: old_config.api_key,
            ..ProfileConfig::default()
        }
    }

    fn merge(&self, other: Option<&ProfileConfig>) -> ProfileConfig {
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

impl AppConfig {
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

    /// List all available profiles from config file
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

    pub fn save(&self) -> Result<(), String> {
        // Load existing config or create new one
        let config_path = PathBuf::from(&self.config_path);
        let mut config_file = Self::load_config_file(&config_path).unwrap_or_default();
        config_file.insert_app_config(self.clone()); // Update the current profile
        config_file.set_app_config_settings(self.clone()); // Update settings

        if let Some(parent) = config_path.parent() {
            create_dir_all(parent).map_err(|e| format!("{}", e))?;
        }

        let config_str = toml::to_string_pretty(&config_file).map_err(|e| format!("{}", e))?;
        write(&self.config_path, config_str).map_err(|e| format!("{}", e))
    }

    fn build(
        profile_name: &str,
        path: PathBuf,
        settings: Settings,
        profile_config: ProfileConfig,
    ) -> Self {
        AppConfig {
            api_endpoint: std::env::var("STAKPAK_API_ENDPOINT").unwrap_or(
                profile_config
                    .api_endpoint
                    .unwrap_or_else(|| STAKPAK_API_ENDPOINT.into()),
            ),
            api_key: std::env::var("STAKPAK_API_KEY")
                .ok()
                .or(profile_config.api_key),
            mcp_server_host: None, // This can be added to profiles later if needed
            machine_name: settings.machine_name,
            auto_append_gitignore: settings.auto_append_gitignore,
            profile_name: profile_name.to_string(),
            config_path: path.display().to_string(),
            allowed_tools: profile_config.allowed_tools,
            auto_approve: profile_config.auto_approve,
            rulebooks: profile_config.rulebooks,
            warden: profile_config.warden,
            provider: profile_config.provider.unwrap_or(ProviderType::Remote),
            openai: profile_config.openai,
            anthropic: profile_config.anthropic,
            gemini: profile_config.gemini,
            smart_model: profile_config.smart_model,
            eco_model: profile_config.eco_model,
            recovery_model: profile_config.recovery_model,
            anonymous_id: settings.anonymous_id,
            collect_telemetry: settings.collect_telemetry,
        }
    }

    pub fn get_config_path<P: AsRef<Path>>(path: Option<P>) -> PathBuf {
        match path {
            Some(p) => p.as_ref().to_path_buf(),
            None => std::env::home_dir()
                .unwrap_or_default()
                .join(STAKPAK_CONFIG_PATH),
        }
    }

    fn migrate_old_config<P: AsRef<Path>>(
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

    fn load_config_file<P: AsRef<Path>>(config_path: P) -> Result<ConfigFile, ConfigError> {
        match std::fs::read_to_string(config_path.as_ref()) {
            Ok(content) => toml::from_str::<ConfigFile>(&content).or_else(|e| {
                println!("Failed to parse config file in new format: {}", e);
                Self::migrate_old_config(config_path, &content)
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ConfigFile::with_default_profile()),
            Err(e) => Err(ConfigError::Message(format!(
                "Failed to read config file: {}",
                e
            ))),
        }
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

    /// Get the config directory from the config path
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

    /// Resolve provider credentials with fallback chain
    ///
    /// Resolution order:
    /// 1. auth.toml → [{profile}.{provider}] (profile-specific)
    /// 2. auth.toml → [all.{provider}] (shared fallback)
    /// 3. config.toml → [profiles.{profile}.{provider}].api_key (legacy)
    /// 4. Environment variable (e.g., ANTHROPIC_API_KEY)
    pub fn resolve_provider_auth(&self, provider: &str) -> Option<ProviderAuth> {
        let config_dir = self.get_config_dir();

        // 1 & 2: Check auth.toml (handles profile inheritance internally)
        if let Ok(auth_manager) = AuthManager::new(&config_dir)
            && let Some(auth) = auth_manager.get(&self.profile_name, provider)
        {
            return Some(auth.clone());
        }

        // 3: Check config.toml legacy API key
        match provider {
            "anthropic" => {
                if let Some(ref anthropic_config) = self.anthropic
                    && let Some(ref key) = anthropic_config.api_key
                    && !key.is_empty()
                {
                    return Some(ProviderAuth::api_key(key));
                }
            }
            "openai" => {
                if let Some(ref openai_config) = self.openai
                    && let Some(ref key) = openai_config.api_key
                    && !key.is_empty()
                {
                    return Some(ProviderAuth::api_key(key));
                }
            }
            "gemini" => {
                if let Some(ref gemini_config) = self.gemini
                    && let Some(ref key) = gemini_config.api_key
                    && !key.is_empty()
                {
                    return Some(ProviderAuth::api_key(key));
                }
            }
            _ => {}
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

    /// Check if OAuth tokens for a provider need refresh and refresh them if needed
    ///
    /// Returns the updated auth if refresh was successful, or the original auth if no refresh was needed.
    /// Returns None if the auth is expired and refresh failed.
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

    /// Get Anthropic config with resolved credentials from auth.toml fallback chain
    ///
    /// This method resolves credentials in the following order:
    /// 1. auth.toml → [{profile}.anthropic]
    /// 2. auth.toml → [all.anthropic]
    /// 3. config.toml → existing anthropic config
    /// 4. ANTHROPIC_API_KEY environment variable
    pub fn get_anthropic_config_with_auth(&self) -> Option<AnthropicConfig> {
        // Try to resolve credentials from auth.toml first
        if let Some(auth) = self.resolve_provider_auth("anthropic") {
            // If we have existing anthropic config, merge credentials into it
            if let Some(existing) = &self.anthropic {
                return Some(existing.clone().with_provider_auth(&auth));
            }
            // Otherwise create new config from auth
            return Some(AnthropicConfig::from_provider_auth(&auth));
        }

        // Fall back to existing config.toml config
        self.anthropic.clone()
    }

    /// Get OpenAI config with resolved credentials from auth.toml fallback chain
    pub fn get_openai_config_with_auth(&self) -> Option<OpenAIConfig> {
        if let Some(auth) = self.resolve_provider_auth("openai") {
            if let Some(existing) = &self.openai {
                return existing.clone().with_provider_auth(&auth);
            }
            return OpenAIConfig::from_provider_auth(&auth);
        }
        self.openai.clone()
    }

    /// Get Gemini config with resolved credentials from auth.toml fallback chain
    pub fn get_gemini_config_with_auth(&self) -> Option<GeminiConfig> {
        if let Some(auth) = self.resolve_provider_auth("gemini") {
            if let Some(existing) = &self.gemini {
                return existing.clone().with_provider_auth(&auth);
            }
            return GeminiConfig::from_provider_auth(&auth);
        }
        self.gemini.clone()
    }

    /// Get Stakpak API key with resolved credentials from auth.toml fallback chain
    ///
    /// Resolution order:
    /// 1. STAKPAK_API_KEY environment variable (already in self.api_key)
    /// 2. config.toml → [profiles.{profile}].api_key (already in self.api_key)
    /// 3. auth.toml → [{profile}.stakpak]
    /// 4. auth.toml → [all.stakpak]
    pub fn get_stakpak_api_key(&self) -> Option<String> {
        // First check if we already have an API key from env or config.toml
        if self.api_key.is_some() {
            return self.api_key.clone();
        }

        // Try to resolve from auth.toml
        if let Some(ProviderAuth::Api { key }) = self.resolve_provider_auth("stakpak") {
            return Some(key);
        }

        None
    }
}

impl RulebookConfig {
    /// Filter rulebooks based on the configuration rules
    pub fn filter_rulebooks(&self, rulebooks: Vec<ListRuleBook>) -> Vec<ListRuleBook> {
        rulebooks
            .into_iter()
            .filter(|rulebook| self.should_keep(rulebook))
            .collect()
    }

    fn should_keep(&self, rulebook: &ListRuleBook) -> bool {
        self.matches_uri_filters(rulebook) && self.matches_tag_filters(rulebook)
    }

    fn matches_uri_filters(&self, rulebook: &ListRuleBook) -> bool {
        self.matches_include_patterns(rulebook) && self.matches_exclude_patterns(rulebook)
    }

    fn matches_include_patterns(&self, rulebook: &ListRuleBook) -> bool {
        match &self.include {
            Some(patterns) if !patterns.is_empty() => patterns
                .iter()
                .any(|pattern| Self::matches_pattern(&rulebook.uri, pattern)),
            _ => true,
        }
    }

    fn matches_exclude_patterns(&self, rulebook: &ListRuleBook) -> bool {
        match &self.exclude {
            Some(patterns) if !patterns.is_empty() => !patterns
                .iter()
                .any(|pattern| Self::matches_pattern(&rulebook.uri, pattern)),
            _ => true,
        }
    }

    fn matches_tag_filters(&self, rulebook: &ListRuleBook) -> bool {
        self.matches_include_tags(rulebook) && self.matches_exclude_tags(rulebook)
    }

    fn matches_include_tags(&self, rulebook: &ListRuleBook) -> bool {
        match &self.include_tags {
            Some(tags) if !tags.is_empty() => tags.iter().any(|tag| rulebook.tags.contains(tag)),
            _ => true,
        }
    }

    fn matches_exclude_tags(&self, rulebook: &ListRuleBook) -> bool {
        match &self.exclude_tags {
            Some(tags) if !tags.is_empty() => !tags.iter().any(|tag| rulebook.tags.contains(tag)),
            _ => true,
        }
    }

    /// Check if a URI matches a pattern (supports wildcards)
    fn matches_pattern(uri: &str, pattern: &str) -> bool {
        // Use glob pattern matching for better wildcard support
        if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
            glob_pattern.matches(uri)
        } else {
            // Fallback to exact match if glob pattern is invalid
            uri == pattern
        }
    }
}

#[cfg(test)]
mod app_config_tests {
    use super::*;
    use tempfile::TempDir;

    const OLD_CONFIG: &str = r#"
api_endpoint = "https://legacy"
api_key = "old-key"
machine_name = "legacy-machine"
auto_append_gitignore = false
"#;

    const NEW_CONFIG: &str = r#"
[profiles]

[profiles.dev]
api_endpoint = "https://new-api.stakpak.dev"
api_key = "dev-key"
allowed_tools = ["read"]

[profiles.a]
api_endpoint = "https://new-api.stakpak.a"
api_key = "a-key"

[settings]
machine_name = "dev-machine"
auto_append_gitignore = true
"#;

    fn get_a_config_path(dir: &TempDir) -> PathBuf {
        dir.path().join("config.toml")
    }

    fn sample_app_config(profile_name: &str) -> AppConfig {
        AppConfig {
            api_endpoint: "https://custom-api.stakpak.dev".into(),
            api_key: Some("custom-key".into()),
            mcp_server_host: Some("localhost:9000".into()),
            machine_name: Some("workstation-1".into()),
            auto_append_gitignore: Some(false),
            profile_name: profile_name.into(),
            config_path: "/tmp/stakpak/config.toml".into(),
            allowed_tools: Some(vec!["git".into(), "curl".into()]),
            auto_approve: Some(vec!["git status".into()]),
            rulebooks: Some(RulebookConfig {
                include: Some(vec!["https://rules.stakpak.dev/security/*".into()]),
                exclude: Some(vec!["https://rules.stakpak.dev/internal/*".into()]),
                include_tags: Some(vec!["security".into()]),
                exclude_tags: Some(vec!["beta".into()]),
            }),
            warden: Some(WardenConfig {
                enabled: true,
                volumes: vec!["/tmp:/tmp:ro".into()],
            }),
            provider: ProviderType::Remote,
            openai: None,
            anthropic: None,
            gemini: None,
            smart_model: None,
            eco_model: None,
            recovery_model: None,
            anonymous_id: Some("test-user-id".into()),
            collect_telemetry: Some(true),
        }
    }

    #[test]
    fn get_config_path_returns_custom_path_when_provided() {
        let custom_path = PathBuf::from("/tmp/stakpak/custom.toml");
        let resolved = AppConfig::get_config_path(Some(&custom_path));
        assert_eq!(custom_path, resolved);
    }

    #[test]
    fn get_config_path_defaults_to_home_directory() {
        let home_dir = std::env::home_dir().unwrap();
        let resolved = AppConfig::get_config_path::<&str>(None);
        let expected = home_dir.join(STAKPAK_CONFIG_PATH);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn old_config_into_profile_config() {
        let old_config: OldAppConfig = toml::from_str(OLD_CONFIG).unwrap();
        let resolved: ProfileConfig = old_config.clone().into();
        let expected = ProfileConfig {
            api_endpoint: Some(old_config.api_endpoint),
            api_key: old_config.api_key,
            ..ProfileConfig::default()
        };

        assert!(resolved.api_endpoint.is_some());
        assert!(expected.api_endpoint.is_some());

        assert_eq!(resolved.api_endpoint, expected.api_endpoint);
        assert_eq!(resolved.api_key, expected.api_key);

        assert!(resolved.allowed_tools.is_none());
        assert!(expected.allowed_tools.is_none());

        assert_eq!(resolved.api_endpoint.as_deref(), Some("https://legacy"));
        assert_eq!(resolved.api_key.as_deref(), Some("old-key"));
    }

    #[test]
    fn old_config_into_setting() {
        let old_config: OldAppConfig = toml::from_str(OLD_CONFIG).unwrap();
        let resolved: Settings = old_config.clone().into();

        assert_eq!(resolved.machine_name, old_config.machine_name);
        assert_eq!(
            resolved.auto_append_gitignore,
            old_config.auto_append_gitignore
        );

        assert_eq!(resolved.machine_name.as_deref(), Some("legacy-machine"));
        assert_eq!(resolved.auto_append_gitignore, Some(false));
    }

    #[test]
    fn old_config_into_config_file() {
        let old_config: OldAppConfig = toml::from_str(OLD_CONFIG).unwrap();
        let resolved: ConfigFile = old_config.clone().into();

        assert_eq!(resolved.profiles.len(), 1);
        assert!(resolved.profiles.contains_key("default"));

        let profile_config = resolved.profiles.get("default").unwrap();

        assert_eq!(
            profile_config.api_endpoint.clone().unwrap(),
            old_config.api_endpoint
        );
        assert_eq!(profile_config.api_key, old_config.api_key);

        assert_eq!(resolved.settings.machine_name, old_config.machine_name);
        assert_eq!(
            resolved.settings.auto_append_gitignore,
            old_config.auto_append_gitignore
        );
    }

    #[test]
    fn config_file_default_has_no_profiles() {
        let config = ConfigFile::default();
        assert!(config.profiles.is_empty());
        assert!(config.profile_config("default").is_none());
        assert_eq!(config.settings.machine_name, None);
        assert_eq!(config.settings.auto_append_gitignore, Some(true));
    }

    #[test]
    fn config_file_with_default_profile_contains_built_in_profile() {
        let config = ConfigFile::with_default_profile();
        let default = config.profile_config("default").expect("default profile");
        assert_eq!(default.api_endpoint.as_deref(), Some(STAKPAK_API_ENDPOINT));
        assert!(config.profile_config("readonly").is_none());
    }

    #[test]
    fn profile_config_ok_or_errors_on_missing_profile() {
        let config = ConfigFile::with_default_profile();
        assert!(config.profile_config_ok_or("default").is_ok());
        let err = config.profile_config_ok_or("missing").unwrap_err();
        match err {
            ConfigError::Message(msg) => {
                assert!(msg.contains("missing"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn resolved_profile_config_merges_all_profile_defaults() {
        let mut config = ConfigFile {
            profiles: HashMap::new(),
            settings: Settings {
                machine_name: None,
                auto_append_gitignore: Some(true),
                anonymous_id: Some("test-user-id".into()),
                collect_telemetry: Some(true),
            },
        };

        config.profiles.insert(
            "all".into(),
            ProfileConfig {
                api_endpoint: Some("https://shared-api.stakpak.dev".into()),
                api_key: Some("shared-key".into()),
                allowed_tools: Some(vec!["git".into()]),
                auto_approve: Some(vec!["git status".into()]),
                rulebooks: Some(RulebookConfig {
                    include: Some(vec!["https://rules.stakpak.dev/shared/*".into()]),
                    exclude: None,
                    include_tags: None,
                    exclude_tags: None,
                }),
                warden: Some(WardenConfig {
                    enabled: true,
                    volumes: vec!["/tmp:/tmp:ro".into()],
                }),
                provider: None,
                openai: None,
                anthropic: None,
                gemini: None,
                smart_model: None,
                eco_model: None,
                recovery_model: None,
            },
        );

        config.profiles.insert(
            "dev".into(),
            ProfileConfig {
                api_endpoint: Some("https://dev-api.stakpak.dev".into()),
                api_key: None,
                allowed_tools: None,
                auto_approve: Some(vec!["dev override".into()]),
                rulebooks: None,
                warden: None,
                provider: None,
                openai: None,
                anthropic: None,
                gemini: None,
                smart_model: None,
                eco_model: None,
                recovery_model: None,
            },
        );

        let resolved = config
            .resolved_profile_config("dev")
            .expect("profile resolves");
        assert_eq!(
            resolved.api_endpoint.as_deref(),
            Some("https://dev-api.stakpak.dev")
        );
        assert_eq!(resolved.api_key.as_deref(), Some("shared-key"));
        assert_eq!(resolved.allowed_tools, Some(vec!["git".into()]));
        assert_eq!(resolved.auto_approve, Some(vec!["dev override".into()]));
        assert!(resolved.rulebooks.is_some());
        assert!(resolved.warden.as_ref().expect("warden merged").enabled);
    }

    #[test]
    fn insert_and_set_app_config_update_profiles_and_settings() {
        let mut config = ConfigFile::default();
        let app_config = sample_app_config("custom");

        config.insert_app_config(app_config.clone());
        config.set_app_config_settings(app_config.clone());

        let stored = config.profile_config("custom").expect("profile stored");
        assert_eq!(
            stored.api_endpoint.as_deref(),
            Some("https://custom-api.stakpak.dev")
        );
        assert_eq!(stored.api_key.as_deref(), Some("custom-key"));
        assert_eq!(
            stored.allowed_tools,
            Some(vec!["git".into(), "curl".into()])
        );
        assert_eq!(stored.auto_approve, Some(vec!["git status".into()]));
        assert!(stored.rulebooks.is_some());
        assert!(stored.warden.is_some());

        assert_eq!(
            config.settings.machine_name.as_deref(),
            Some("workstation-1")
        );
        assert_eq!(config.settings.auto_append_gitignore, Some(false));
    }

    #[test]
    fn ensure_readonly_inserts_profile_once() {
        let mut config = ConfigFile::with_default_profile();
        assert!(!config.profiles.contains_key("readonly"));
        assert!(config.ensure_readonly());
        assert!(config.profiles.contains_key("readonly"));
        assert!(!config.ensure_readonly(), "second call should be a no-op");

        let readonly = config.profile_config("readonly").expect("readonly present");
        let default = config.profile_config("default").expect("default present");
        assert_eq!(readonly.api_endpoint, default.api_endpoint);
        assert!(readonly.warden.as_ref().expect("readonly warden").enabled);
    }

    #[test]
    fn save_to_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let nested_path = dir.path().join("nested/config/config.toml");
        let config = ConfigFile::with_default_profile();

        config.save_to(&nested_path).unwrap();

        assert!(nested_path.exists());
        let saved = std::fs::read_to_string(&nested_path).unwrap();
        assert!(saved.contains("[profiles.default]"));
        assert!(saved.contains("[settings]"));
    }

    #[test]
    fn migrate_old_config() {
        let dir = TempDir::new().unwrap();
        let path = get_a_config_path(&dir);
        let config = AppConfig::migrate_old_config(&path, OLD_CONFIG).unwrap();
        let default = config.profiles.get("default").unwrap();

        assert_eq!(default.api_endpoint.as_deref(), Some("https://legacy"));
        assert_eq!(default.api_key.as_deref(), Some("old-key"));
        assert_eq!(
            config.settings.machine_name.as_deref(),
            Some("legacy-machine")
        );
        assert_eq!(config.settings.auto_append_gitignore, Some(false));

        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("[profiles.default]"));
        assert!(saved.contains("[settings]"));
    }

    #[test]
    fn profile_config_with_api_endpoint() {
        let p1 = ProfileConfig::with_api_endpoint("url1");
        let p2 = ProfileConfig::with_api_endpoint("url2");

        assert_eq!(p1.api_endpoint.as_deref(), Some("url1"));
        assert_eq!(p2.api_endpoint.as_deref(), Some("url2"));

        let default = ProfileConfig::default();

        assert!(default.api_endpoint.is_none());
        assert!(default.api_key.is_none());

        assert_ne!(p1.api_endpoint, default.api_endpoint);
        assert_ne!(p2.api_endpoint, default.api_endpoint);

        assert_eq!(p1.api_key, default.api_key);
        assert_eq!(p2.api_key, default.api_key);
    }

    #[test]
    fn load_config_file_for_missing_path() {
        let dir = TempDir::new().unwrap();
        let path = get_a_config_path(&dir);
        let config = AppConfig::load_config_file(&path).unwrap();

        assert!(config.profiles.contains_key("default"));
        assert!(!path.exists());
    }

    #[test]
    fn load_config_file_for_old_formats() {
        let dir = TempDir::new().unwrap();
        let path = get_a_config_path(&dir);

        std::fs::write(&path, OLD_CONFIG).unwrap();

        let config = AppConfig::load_config_file(&path).unwrap();
        assert_eq!(
            config.settings.machine_name.as_deref(),
            Some("legacy-machine")
        );
        assert_eq!(config.settings.auto_append_gitignore, Some(false));

        let default = config.profiles.get("default").unwrap();
        assert_eq!(default.api_endpoint.as_deref(), Some("https://legacy"));
        assert_eq!(default.api_key.as_deref(), Some("old-key"));

        let overriden = std::fs::read_to_string(&path).unwrap();
        assert!(overriden.contains("[profiles.default]"));
        assert!(overriden.contains("[settings]"));
    }

    #[test]
    fn load_config_file_for_new_formats() {
        let dir = TempDir::new().unwrap();
        let path = get_a_config_path(&dir);

        std::fs::write(&path, NEW_CONFIG).unwrap();

        let config = AppConfig::load_config_file(&path).unwrap();
        assert!(config.profiles.contains_key("dev"));

        let dev = config.profiles.get("dev").unwrap();
        assert_eq!(
            dev.api_endpoint.as_deref(),
            Some("https://new-api.stakpak.dev")
        );
        assert_eq!(dev.api_key.as_deref(), Some("dev-key"));
        assert_eq!(dev.allowed_tools, Some(vec!["read".to_string()]));

        assert_eq!(config.settings.machine_name.as_deref(), Some("dev-machine"));
        assert_eq!(config.settings.auto_append_gitignore, Some(true));
    }

    #[test]
    fn save_writes_profile_and_settings() {
        let dir = TempDir::new().unwrap();
        let path = get_a_config_path(&dir);
        let config = AppConfig {
            api_endpoint: "https://custom-api.stakpak.dev".into(),
            api_key: Some("custom-key".into()),
            mcp_server_host: Some("localhost:9000".into()),
            machine_name: Some("workstation-1".into()),
            auto_append_gitignore: Some(false),
            profile_name: "dev".into(),
            config_path: path.to_string_lossy().into_owned(),
            allowed_tools: Some(vec!["git".into(), "curl".into()]),
            auto_approve: Some(vec!["git status".into()]),
            rulebooks: Some(RulebookConfig {
                include: Some(vec!["https://rules.stakpak.dev/security/*".into()]),
                exclude: Some(vec!["https://rules.stakpak.dev/internal/*".into()]),
                include_tags: Some(vec!["security".into()]),
                exclude_tags: Some(vec!["beta".into()]),
            }),
            warden: Some(WardenConfig {
                enabled: true,
                volumes: vec!["/tmp:/tmp:ro".into()],
            }),
            provider: ProviderType::Remote,
            openai: None,
            anthropic: None,
            gemini: None,
            smart_model: None,
            eco_model: None,
            recovery_model: None,
            anonymous_id: Some("test-user-id".into()),
            collect_telemetry: Some(true),
        };

        config.save().unwrap();

        let saved: ConfigFile = AppConfig::load_config_file(&path).unwrap();

        let profile = saved.profiles.get("dev").expect("profile saved");
        assert_eq!(
            profile.api_endpoint.as_deref(),
            Some("https://custom-api.stakpak.dev")
        );
        assert_eq!(profile.api_key.as_deref(), Some("custom-key"));
        assert_eq!(
            profile.allowed_tools,
            Some(vec!["git".to_string(), "curl".to_string()])
        );
        assert_eq!(profile.auto_approve, Some(vec!["git status".to_string()]));

        let rulebooks = profile.rulebooks.as_ref().expect("rulebooks persisted");
        assert_eq!(
            rulebooks.include.as_ref().unwrap(),
            &vec!["https://rules.stakpak.dev/security/*".to_string()]
        );
        assert_eq!(
            rulebooks.exclude.as_ref().unwrap(),
            &vec!["https://rules.stakpak.dev/internal/*".to_string()]
        );
        assert_eq!(
            rulebooks.include_tags.as_ref().unwrap(),
            &vec!["security".to_string()]
        );
        assert_eq!(
            rulebooks.exclude_tags.as_ref().unwrap(),
            &vec!["beta".to_string()]
        );

        let warden = profile.warden.as_ref().expect("warden persisted");
        assert!(warden.enabled);
        assert_eq!(&warden.volumes, &vec!["/tmp:/tmp:ro".to_string()]);

        assert_eq!(
            saved.settings.machine_name.as_deref(),
            Some("workstation-1")
        );
        assert_eq!(saved.settings.auto_append_gitignore, Some(false));
    }

    #[test]
    fn list_available_profiles_returns_default_when_missing_config() {
        let dir = TempDir::new().unwrap();
        let path = get_a_config_path(&dir);

        let profiles = AppConfig::list_available_profiles(Some(&path)).unwrap();

        assert_eq!(profiles, vec!["default".to_string()]);
    }

    #[test]
    fn list_available_profiles_reads_existing_config() {
        let dir = TempDir::new().unwrap();
        let path = get_a_config_path(&dir);

        std::fs::write(&path, NEW_CONFIG).unwrap();

        let profiles = AppConfig::list_available_profiles(Some(&path)).unwrap();

        assert_eq!(profiles, vec!["a".to_string(), "dev".to_string()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use stakpak_api::models::RuleBookVisibility;

    fn create_test_rulebook(uri: &str, tags: Vec<String>) -> ListRuleBook {
        ListRuleBook {
            id: "test-id".to_string(),
            uri: uri.to_string(),
            description: "Test rulebook".to_string(),
            visibility: RuleBookVisibility::Public,
            tags,
            created_at: Some(Utc::now()),
            updated_at: Some(Utc::now()),
        }
    }

    #[test]
    fn test_glob_pattern_matching() {
        // Test wildcard patterns
        assert!(RulebookConfig::matches_pattern(
            "https://rules.stakpak.dev/security/auth",
            "https://rules.stakpak.dev/security/*"
        ));

        assert!(RulebookConfig::matches_pattern(
            "https://rules.stakpak.dev/security/network",
            "https://rules.stakpak.dev/security/*"
        ));

        assert!(!RulebookConfig::matches_pattern(
            "https://rules.stakpak.dev/performance/v1",
            "https://rules.stakpak.dev/security/*"
        ));

        // Test exact match
        assert!(RulebookConfig::matches_pattern(
            "https://rules.stakpak.dev/performance/v2",
            "https://rules.stakpak.dev/performance/v2"
        ));

        // Test multiple wildcards
        assert!(RulebookConfig::matches_pattern(
            "https://internal.company.com/team1/stable",
            "https://internal.company.com/*/stable"
        ));

        assert!(!RulebookConfig::matches_pattern(
            "https://internal.company.com/team1/beta",
            "https://internal.company.com/*/stable"
        ));

        // Test question mark wildcard
        assert!(RulebookConfig::matches_pattern(
            "https://rules.stakpak.dev/performance/v1",
            "https://rules.stakpak.dev/performance/v?"
        ));

        assert!(RulebookConfig::matches_pattern(
            "https://rules.stakpak.dev/performance/v2",
            "https://rules.stakpak.dev/performance/v?"
        ));
    }

    #[test]
    fn test_rulebook_filtering_include_patterns() {
        let config = RulebookConfig {
            include: Some(vec![
                "https://rules.stakpak.dev/security/*".to_string(),
                "https://internal.company.com/*/stable".to_string(),
            ]),
            exclude: None,
            include_tags: None,
            exclude_tags: None,
        };

        let rulebooks = vec![
            create_test_rulebook("https://rules.stakpak.dev/security/auth", vec![]),
            create_test_rulebook("https://rules.stakpak.dev/performance/v1", vec![]),
            create_test_rulebook("https://internal.company.com/team1/stable", vec![]),
            create_test_rulebook("https://internal.company.com/team1/beta", vec![]),
            create_test_rulebook("https://experimental.rules.dev/test", vec![]),
        ];

        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://rules.stakpak.dev/security/auth")
        );
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://internal.company.com/team1/stable")
        );
    }

    #[test]
    fn test_rulebook_filtering_exclude_patterns() {
        let config = RulebookConfig {
            include: None,
            exclude: Some(vec![
                "https://rules.stakpak.dev/*/beta".to_string(),
                "https://experimental.rules.dev/*".to_string(),
            ]),
            include_tags: None,
            exclude_tags: None,
        };

        let rulebooks = vec![
            create_test_rulebook("https://rules.stakpak.dev/security/stable", vec![]),
            create_test_rulebook("https://rules.stakpak.dev/security/beta", vec![]),
            create_test_rulebook("https://internal.company.com/team1/stable", vec![]),
            create_test_rulebook("https://experimental.rules.dev/test", vec![]),
        ];

        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://rules.stakpak.dev/security/stable")
        );
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://internal.company.com/team1/stable")
        );
    }

    #[test]
    fn test_rulebook_filtering_include_tags() {
        let config = RulebookConfig {
            include: None,
            exclude: None,
            include_tags: Some(vec!["security".to_string(), "stable".to_string()]),
            exclude_tags: None,
        };

        let rulebooks = vec![
            create_test_rulebook("https://rules.stakpak.dev/r1", vec!["security".to_string()]),
            create_test_rulebook(
                "https://rules.stakpak.dev/r2",
                vec!["performance".to_string()],
            ),
            create_test_rulebook("https://rules.stakpak.dev/r3", vec!["stable".to_string()]),
            create_test_rulebook("https://rules.stakpak.dev/r4", vec!["beta".to_string()]),
        ];

        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://rules.stakpak.dev/r1")
        );
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://rules.stakpak.dev/r3")
        );
    }

    #[test]
    fn test_rulebook_filtering_exclude_tags() {
        let config = RulebookConfig {
            include: None,
            exclude: None,
            include_tags: None,
            exclude_tags: Some(vec!["beta".to_string(), "deprecated".to_string()]),
        };

        let rulebooks = vec![
            create_test_rulebook("https://rules.stakpak.dev/r1", vec!["security".to_string()]),
            create_test_rulebook("https://rules.stakpak.dev/r2", vec!["beta".to_string()]),
            create_test_rulebook("https://rules.stakpak.dev/r3", vec!["stable".to_string()]),
            create_test_rulebook(
                "https://rules.stakpak.dev/r4",
                vec!["deprecated".to_string()],
            ),
        ];

        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://rules.stakpak.dev/r1")
        );
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://rules.stakpak.dev/r3")
        );
    }
}
