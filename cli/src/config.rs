use config::ConfigError;
use serde::{Deserialize, Serialize};
use stakpak_api::{ClientConfig, ListRuleBook};
use std::collections::HashMap;
use std::fs::{create_dir_all, write};
use std::io;
use std::path::{Path, PathBuf};

const STAKPAK_API_ENDPOINT: &str = "https://apiv2.stakpak.dev";
const STAKPAK_CONFIG_PATH: &str = ".stakpak/config.toml";

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
    /// Allowed tools (empty = all tools allowed)
    pub allowed_tools: Option<Vec<String>>,
    /// Tools that auto-approve without asking
    pub auto_approve: Option<Vec<String>>,
    /// Rulebook filtering configuration
    pub rulebooks: Option<RulebookConfig>,
    /// Warden (runtime security) configuration
    pub warden: Option<WardenConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
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
}

#[derive(Deserialize, Clone)]
struct OldAppConfig {
    pub api_endpoint: String,
    pub api_key: Option<String>,
    pub machine_name: Option<String>,
    pub auto_append_gitignore: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ProfileInfo {
    pub name: String,
    pub has_api_key: bool,
    pub allowed_tools_count: usize,
    pub auto_approve_count: usize,
    pub is_restricted: bool,
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

impl From<OldAppConfig> for Settings {
    fn from(old_config: OldAppConfig) -> Self {
        Settings {
            machine_name: old_config.machine_name,
            auto_append_gitignore: old_config.auto_append_gitignore,
        }
    }
}

impl From<OldAppConfig> for ConfigFile {
    // OldAppConfigConfig will always create a 'default' ConfigFile
    fn from(old_config: OldAppConfig) -> Self {
        ConfigFile {
            profiles: HashMap::from([("default".to_string(), old_config.clone().into())]),
            settings: old_config.into(),
        }
    }
}

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            profiles: HashMap::from([(
                "default".into(),
                ProfileConfig::with_api_endpoint(STAKPAK_API_ENDPOINT),
            )]),
            settings: Settings {
                machine_name: None,
                auto_append_gitignore: Some(true),
            },
        }
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

fn create_readonly_profile(default_profile: Option<&ProfileConfig>) -> ProfileConfig {
    ProfileConfig {
        api_endpoint: default_profile.and_then(|p| p.api_endpoint.clone()),
        api_key: default_profile.and_then(|p| p.api_key.clone()),
        warden: Some(WardenConfig::readonly_profile()),
        ..ProfileConfig::default()
    }
}

impl ProfileConfig {
    fn with_api_endpoint(api_endpoint: &str) -> Self {
        ProfileConfig {
            api_endpoint: Some(api_endpoint.into()),
            ..ProfileConfig::default()
        }
    }
}

impl AppConfig {
    fn get_config_path<P: AsRef<Path>>(path: Option<P>) -> PathBuf {
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
        let old_config = toml::from_str::<OldAppConfig>(content).map_err(|_| {
            ConfigError::Message("Failed to parse config file in both old and new formats".into())
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
            Ok(content) => toml::from_str::<ConfigFile>(&content)
                .or_else(|_| Self::migrate_old_config(config_path, &content)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ConfigFile::default()),
            Err(e) => Err(ConfigError::Message(format!(
                "Failed to read config file: {}",
                e
            ))),
        }
    }

    pub fn load<P: AsRef<Path>>(
        profile_name: &str,
        custom_config_path: Option<P>,
    ) -> Result<Self, ConfigError> {
        // Don't allow "all" as a profile to be loaded directly
        if profile_name == "all" {
            return Err(ConfigError::Message(
                "Cannot use 'all' as a profile name. It's reserved for defaults.".to_string(),
            ));
        }

        let config_path = Self::get_config_path(custom_config_path);

        // Try to load existing config file
        let mut config_file = Self::load_config_file(&config_path)?;

        let mut is_config_file_dirty = false;
        if !config_file.profiles.contains_key("readonly") {
            let base_profile = config_file.profiles.get("default");
            let readonly_profile = create_readonly_profile(base_profile);
            config_file
                .profiles
                .insert("readonly".to_string(), readonly_profile);
            is_config_file_dirty = true;
        }

        // Get the specified profile
        let profile = config_file
            .profiles
            .get(profile_name)
            .cloned()
            .ok_or_else(|| {
                ConfigError::Message(format!(
                    "Profile '{}' not found in configuration",
                    profile_name
                ))
            })?;

        // Get defaults from "all" profile if it exists
        let all_profile = config_file.profiles.get("all");

        // Apply inheritance: profile values override "all" profile values
        let api_endpoint = profile
            .api_endpoint
            .or_else(|| all_profile.and_then(|all| all.api_endpoint.clone()))
            .unwrap_or_else(|| STAKPAK_API_ENDPOINT.into());

        let api_key = profile
            .api_key
            .or_else(|| all_profile.and_then(|all| all.api_key.clone()));

        // Apply inheritance for tool settings
        let allowed_tools = profile
            .allowed_tools
            .or_else(|| all_profile.and_then(|all| all.allowed_tools.clone()));

        let auto_approve = profile
            .auto_approve
            .or_else(|| all_profile.and_then(|all| all.auto_approve.clone()));

        let rulebooks = profile
            .rulebooks
            .or_else(|| all_profile.and_then(|all| all.rulebooks.clone()));

        let warden = profile
            .warden
            .or_else(|| all_profile.and_then(|all| all.warden.clone()));

        // Override with environment variables if present
        let api_key = std::env::var("STAKPAK_API_KEY").ok().or(api_key);
        let api_endpoint = std::env::var("STAKPAK_API_ENDPOINT").unwrap_or(api_endpoint);

        let app_config = AppConfig {
            api_endpoint,
            api_key,
            mcp_server_host: None, // This can be added to profiles later if needed
            machine_name: config_file.settings.machine_name,
            auto_append_gitignore: config_file.settings.auto_append_gitignore,
            profile_name: profile_name.to_string(),
            config_path: config_path.display().to_string(),
            allowed_tools,
            auto_approve,
            rulebooks,
            warden,
        };

        if is_config_file_dirty {
            // fail without crashing, because it's not critical
            if let Err(e) = app_config.save() {
                eprintln!("Warning: Failed to update config on load: {}", e);
            }
        }

        Ok(app_config)
    }

    /// List all available profiles from config file
    pub fn list_available_profiles(
        custom_config_path: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let config_path = Self::get_config_path(custom_config_path);

        if !Path::new(&config_path).exists() {
            return Err("Config file not found".to_string());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config_file: ConfigFile =
            toml::from_str(&content).map_err(|e| format!("Failed to parse config file: {}", e))?;

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

    /// Get profile display info
    pub fn get_profile_info(
        profile_name: &str,
        custom_config_path: Option<&str>,
    ) -> Result<ProfileInfo, String> {
        let config = Self::load(profile_name, custom_config_path).map_err(|e| e.to_string())?;

        Ok(ProfileInfo {
            name: profile_name.to_string(),
            has_api_key: config.api_key.is_some(),
            allowed_tools_count: config.allowed_tools.as_ref().map(|t| t.len()).unwrap_or(0),
            auto_approve_count: config.auto_approve.as_ref().map(|t| t.len()).unwrap_or(0),
            is_restricted: config
                .allowed_tools
                .as_ref()
                .map(|t| t.len() < 5)
                .unwrap_or(false),
        })
    }

    pub fn save(&self) -> Result<(), String> {
        // Load existing config or create new one
        let mut config_file = if Path::new(&self.config_path).exists() {
            let content = std::fs::read_to_string(&self.config_path)
                .map_err(|e| format!("Failed to read config file: {}", e))?;
            toml::from_str::<ConfigFile>(&content)
                .map_err(|e| format!("Failed to parse config file: {}", e))?
        } else {
            ConfigFile {
                profiles: HashMap::new(),
                settings: Settings {
                    machine_name: None,
                    auto_append_gitignore: Some(true),
                },
            }
        };

        // Update the current profile
        config_file.profiles.insert(
            self.profile_name.clone(),
            ProfileConfig {
                api_endpoint: Some(self.api_endpoint.clone()),
                api_key: self.api_key.clone(),
                allowed_tools: self.allowed_tools.clone(),
                auto_approve: self.auto_approve.clone(),
                rulebooks: self.rulebooks.clone(),
                warden: self.warden.clone(),
            },
        );

        // Update settings
        config_file.settings = Settings {
            machine_name: self.machine_name.clone(),
            auto_append_gitignore: self.auto_append_gitignore,
        };

        if let Some(parent) = Path::new(&self.config_path).parent() {
            create_dir_all(parent).map_err(|e| format!("{}", e))?;
        }

        let config_str = toml::to_string_pretty(&config_file).map_err(|e| format!("{}", e))?;
        write(&self.config_path, config_str).map_err(|e| format!("{}", e))
    }
}

impl RulebookConfig {
    /// Filter rulebooks based on the configuration rules
    pub fn filter_rulebooks(&self, rulebooks: Vec<ListRuleBook>) -> Vec<ListRuleBook> {
        let mut filtered = rulebooks;

        // Apply include patterns first (if specified)
        if let Some(include_patterns) = &self.include
            && !include_patterns.is_empty()
        {
            filtered.retain(|rulebook| {
                include_patterns
                    .iter()
                    .any(|pattern| Self::matches_pattern(&rulebook.uri, pattern))
            });
        }

        // Apply exclude patterns (if specified)
        if let Some(exclude_patterns) = &self.exclude
            && !exclude_patterns.is_empty()
        {
            filtered.retain(|rulebook| {
                !exclude_patterns
                    .iter()
                    .any(|pattern| Self::matches_pattern(&rulebook.uri, pattern))
            });
        }

        // Apply include tags (if specified)
        if let Some(include_tags) = &self.include_tags
            && !include_tags.is_empty()
        {
            filtered.retain(|rulebook| include_tags.iter().any(|tag| rulebook.tags.contains(tag)));
        }

        // Apply exclude tags (if specified)
        if let Some(exclude_tags) = &self.exclude_tags
            && !exclude_tags.is_empty()
        {
            filtered.retain(|rulebook| !exclude_tags.iter().any(|tag| rulebook.tags.contains(tag)));
        }

        filtered
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

[settings]
machine_name = "dev-machine"
auto_append_gitignore = true
"#;

    fn get_a_config_path(dir: &TempDir) -> PathBuf {
        dir.path().join("config.toml")
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use stakpak_api::RuleBookVisibility;

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
            include_tags: Some(vec!["security".to_string(), "performance".to_string()]),
            exclude_tags: None,
        };

        let rulebooks = vec![
            create_test_rulebook(
                "https://rules.stakpak.dev/rule1",
                vec!["security".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/rule2",
                vec!["performance".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/rule3",
                vec!["experimental".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/rule4",
                vec!["security".to_string(), "production".to_string()],
            ),
        ];

        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), 3);
        assert!(filtered.iter().any(|r| r.uri.contains("rule1")));
        assert!(filtered.iter().any(|r| r.uri.contains("rule2")));
        assert!(filtered.iter().any(|r| r.uri.contains("rule4")));
    }

    #[test]
    fn test_rulebook_filtering_exclude_tags() {
        let config = RulebookConfig {
            include: None,
            exclude: None,
            include_tags: None,
            exclude_tags: Some(vec!["experimental".to_string(), "dev-only".to_string()]),
        };

        let rulebooks = vec![
            create_test_rulebook(
                "https://rules.stakpak.dev/rule1",
                vec!["security".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/rule2",
                vec!["experimental".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/rule3",
                vec!["production".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/rule4",
                vec!["dev-only".to_string(), "security".to_string()],
            ),
        ];

        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|r| r.uri.contains("rule1")));
        assert!(filtered.iter().any(|r| r.uri.contains("rule3")));
    }

    #[test]
    fn test_rulebook_filtering_combined() {
        let config = RulebookConfig {
            include: Some(vec!["https://rules.stakpak.dev/*".to_string()]),
            exclude: Some(vec!["https://rules.stakpak.dev/*/beta".to_string()]),
            include_tags: Some(vec!["security".to_string()]),
            exclude_tags: Some(vec!["experimental".to_string()]),
        };

        let rulebooks = vec![
            create_test_rulebook(
                "https://rules.stakpak.dev/security/stable",
                vec!["security".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/security/beta",
                vec!["security".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/performance/stable",
                vec!["performance".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/security/experimental",
                vec!["security".to_string(), "experimental".to_string()],
            ),
            create_test_rulebook(
                "https://external.rules.dev/security",
                vec!["security".to_string()],
            ),
        ];

        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), 1);
        assert!(
            filtered
                .iter()
                .any(|r| r.uri == "https://rules.stakpak.dev/security/stable")
        );
    }

    #[test]
    fn test_config_file_parsing() {
        let config_content = r#"
[profiles]

[profiles.test]
api_endpoint = "https://test-api.stakpak.dev"
api_key = "test_key"
allowed_tools = ["read", "create"]
auto_approve = ["read"]

[profiles.test.rulebooks]
include = ["https://rules.stakpak.dev/security/*"]
exclude = ["https://rules.stakpak.dev/*/beta"]
include_tags = ["security", "performance"]
exclude_tags = ["experimental"]

[profiles.test.warden]
enabled = true
volumes = ["~/.stakpak/config.toml:/home/agent/.stakpak/config.toml:ro", "./:/agent:ro", "./.stakpak:/agent/.stakpak"]

[settings]
machine_name = "test-machine"
auto_append_gitignore = true
"#;

        let config: ConfigFile = toml::from_str(config_content).expect("Failed to parse config");

        let test_profile = config.profiles.get("test").expect("Test profile not found");
        assert_eq!(
            test_profile.api_endpoint,
            Some("https://test-api.stakpak.dev".to_string())
        );
        assert_eq!(test_profile.api_key, Some("test_key".to_string()));
        assert_eq!(
            test_profile.allowed_tools,
            Some(vec!["read".to_string(), "create".to_string()])
        );
        assert_eq!(test_profile.auto_approve, Some(vec!["read".to_string()]));

        let rulebooks = test_profile
            .rulebooks
            .as_ref()
            .expect("Rulebooks config not found");
        assert_eq!(
            rulebooks.include,
            Some(vec!["https://rules.stakpak.dev/security/*".to_string()])
        );
        assert_eq!(
            rulebooks.exclude,
            Some(vec!["https://rules.stakpak.dev/*/beta".to_string()])
        );
        assert_eq!(
            rulebooks.include_tags,
            Some(vec!["security".to_string(), "performance".to_string()])
        );
        assert_eq!(
            rulebooks.exclude_tags,
            Some(vec!["experimental".to_string()])
        );

        let warden = test_profile
            .warden
            .as_ref()
            .expect("Warden config not found");
        assert!(warden.enabled);
        assert_eq!(warden.volumes.len(), 3);
        assert_eq!(
            warden.volumes[0],
            "~/.stakpak/config.toml:/home/agent/.stakpak/config.toml:ro"
        );
        assert_eq!(warden.volumes[1], "./:/agent:ro");
        assert_eq!(warden.volumes[2], "./.stakpak:/agent/.stakpak");

        assert_eq!(
            config.settings.machine_name,
            Some("test-machine".to_string())
        );
        assert_eq!(config.settings.auto_append_gitignore, Some(true));
    }

    #[test]
    fn test_empty_filters_allow_all() {
        let config = RulebookConfig {
            include: None,
            exclude: None,
            include_tags: None,
            exclude_tags: None,
        };

        let rulebooks = vec![
            create_test_rulebook(
                "https://rules.stakpak.dev/rule1",
                vec!["security".to_string()],
            ),
            create_test_rulebook(
                "https://rules.stakpak.dev/rule2",
                vec!["performance".to_string()],
            ),
            create_test_rulebook(
                "https://experimental.rules.dev/rule3",
                vec!["experimental".to_string()],
            ),
        ];

        let original_count = rulebooks.len();
        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), original_count);
    }

    #[test]
    fn test_empty_include_lists_allow_all() {
        let config = RulebookConfig {
            include: Some(vec![]),
            exclude: None,
            include_tags: Some(vec![]),
            exclude_tags: None,
        };

        let rulebooks = vec![
            create_test_rulebook(
                "https://rules.stakpak.dev/rule1",
                vec!["security".to_string()],
            ),
            create_test_rulebook(
                "https://experimental.rules.dev/rule2",
                vec!["experimental".to_string()],
            ),
        ];

        let original_count = rulebooks.len();
        let filtered = config.filter_rulebooks(rulebooks);
        assert_eq!(filtered.len(), original_count);
    }
}
