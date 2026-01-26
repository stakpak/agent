//! Tests for configuration module.

use super::*;
use chrono::Utc;
use stakpak_api::models::RuleBookVisibility;
use stakpak_shared::models::llm::ProviderConfig;
use std::collections::HashMap;
use std::path::PathBuf;
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
        providers: HashMap::new(),
        smart_model: None,
        eco_model: None,
        recovery_model: None,
        model: None,
        anonymous_id: Some("test-user-id".into()),
        collect_telemetry: Some(true),
        editor: Some("nano".into()),
    }
}

fn create_test_rulebook(uri: &str, tags: Vec<String>) -> stakpak_api::models::ListRuleBook {
    stakpak_api::models::ListRuleBook {
        id: "test-id".to_string(),
        uri: uri.to_string(),
        description: "Test rulebook".to_string(),
        visibility: RuleBookVisibility::Public,
        tags,
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
    }
}

// =============================================================================
// AppConfig Tests
// =============================================================================

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
    let old_config: types::OldAppConfig = toml::from_str(OLD_CONFIG).unwrap();
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
    let old_config: types::OldAppConfig = toml::from_str(OLD_CONFIG).unwrap();
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
    let old_config: types::OldAppConfig = toml::from_str(OLD_CONFIG).unwrap();
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
        config::ConfigError::Message(msg) => {
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
            editor: Some("nano".into()),
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
            ..ProfileConfig::default()
        },
    );

    config.profiles.insert(
        "dev".into(),
        ProfileConfig {
            api_endpoint: Some("https://dev-api.stakpak.dev".into()),
            api_key: None,
            allowed_tools: None,
            auto_approve: Some(vec!["dev override".into()]),
            ..ProfileConfig::default()
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
        providers: HashMap::new(),
        smart_model: None,
        eco_model: None,
        recovery_model: None,
        model: None,
        anonymous_id: Some("test-user-id".into()),
        collect_telemetry: Some(true),
        editor: Some("nano".into()),
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

// =============================================================================
// Rulebook Tests
// =============================================================================

#[test]
fn test_glob_pattern_matching() {
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

    assert!(RulebookConfig::matches_pattern(
        "https://rules.stakpak.dev/performance/v2",
        "https://rules.stakpak.dev/performance/v2"
    ));

    assert!(RulebookConfig::matches_pattern(
        "https://internal.company.com/team1/stable",
        "https://internal.company.com/*/stable"
    ));

    assert!(!RulebookConfig::matches_pattern(
        "https://internal.company.com/team1/beta",
        "https://internal.company.com/*/stable"
    ));

    assert!(RulebookConfig::matches_pattern(
        "https://rules.stakpak.dev/performance/v1",
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

// =============================================================================
// Providers Tests (New Unified Format)
// =============================================================================

#[test]
fn test_providers_toml_parsing_new_format() {
    let toml_str = r#"
[settings]

[profiles.litellm]
provider = "local"
eco_model = "litellm/claude-opus-4-5"
smart_model = "litellm/claude-opus-4-5"

[profiles.litellm.providers.litellm]
type = "custom"
api_endpoint = "http://localhost:4000"
api_key = "sk-1234"

[profiles.litellm.providers.ollama]
type = "custom"
api_endpoint = "http://localhost:11434/v1"
"#;

    let config: ConfigFile = toml::from_str(toml_str).expect("Failed to parse toml");

    let profile = config
        .profiles
        .get("litellm")
        .expect("litellm profile not found");
    assert!(matches!(profile.provider, Some(ProviderType::Local)));
    assert_eq!(
        profile.eco_model.as_deref(),
        Some("litellm/claude-opus-4-5")
    );
    assert_eq!(
        profile.smart_model.as_deref(),
        Some("litellm/claude-opus-4-5")
    );

    // Check providers HashMap
    assert_eq!(profile.providers.len(), 2);

    let litellm_provider = profile
        .providers
        .get("litellm")
        .expect("litellm provider not found");
    match litellm_provider {
        ProviderConfig::Custom {
            api_endpoint,
            api_key,
        } => {
            assert_eq!(api_endpoint, "http://localhost:4000");
            assert_eq!(api_key, &Some("sk-1234".to_string()));
        }
        _ => panic!("Expected Custom provider"),
    }

    let ollama_provider = profile
        .providers
        .get("ollama")
        .expect("ollama provider not found");
    match ollama_provider {
        ProviderConfig::Custom {
            api_endpoint,
            api_key,
        } => {
            assert_eq!(api_endpoint, "http://localhost:11434/v1");
            assert!(api_key.is_none());
        }
        _ => panic!("Expected Custom provider"),
    }
}

#[test]
fn test_providers_toml_parsing_builtin_types() {
    let toml_str = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.providers.openai]
type = "openai"
api_key = "sk-openai"

[profiles.default.providers.anthropic]
type = "anthropic"
api_key = "sk-ant"
access_token = "oauth-token"

[profiles.default.providers.gemini]
type = "gemini"
api_key = "gemini-key"
"#;

    let config: ConfigFile = toml::from_str(toml_str).expect("Failed to parse toml");

    let profile = config
        .profiles
        .get("default")
        .expect("default profile not found");
    assert_eq!(profile.providers.len(), 3);

    // Check OpenAI
    let openai = profile.providers.get("openai").expect("openai not found");
    assert!(matches!(openai, ProviderConfig::OpenAI { .. }));
    assert_eq!(openai.api_key(), Some("sk-openai"));

    // Check Anthropic
    let anthropic = profile
        .providers
        .get("anthropic")
        .expect("anthropic not found");
    match anthropic {
        ProviderConfig::Anthropic {
            api_key,
            access_token,
            ..
        } => {
            assert_eq!(api_key, &Some("sk-ant".to_string()));
            assert_eq!(access_token, &Some("oauth-token".to_string()));
        }
        _ => panic!("Expected Anthropic provider"),
    }

    // Check Gemini
    let gemini = profile.providers.get("gemini").expect("gemini not found");
    assert!(matches!(gemini, ProviderConfig::Gemini { .. }));
    assert_eq!(gemini.api_key(), Some("gemini-key"));
}

// =============================================================================
// Legacy Provider Migration Tests
// =============================================================================

#[test]
fn test_legacy_provider_migration_on_load() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    // Old format with separate openai/anthropic fields
    let old_config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "gpt-4"
eco_model = "gpt-4o-mini"

[profiles.default.openai]
api_key = "sk-openai-key"

[profiles.default.anthropic]
api_key = "sk-ant-key"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();

    let profile = config_file.profiles.get("default").unwrap();

    // Should have migrated to providers HashMap
    assert!(profile.providers.contains_key("openai"));
    assert!(profile.providers.contains_key("anthropic"));

    // Legacy fields should be cleared
    assert!(profile.openai.is_none());
    assert!(profile.anthropic.is_none());

    // Saved file should use new format
    let saved_content = std::fs::read_to_string(&config_path).unwrap();
    assert!(saved_content.contains("[profiles.default.providers.openai]"));
    assert!(saved_content.contains("[profiles.default.providers.anthropic]"));
}

#[test]
fn test_new_format_preserved_on_load() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    // New format should be preserved as-is
    let new_config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "gpt-4"
eco_model = "gpt-4o-mini"

[profiles.default.providers.openai]
type = "openai"
api_key = "sk-openai-key"
"#;
    std::fs::write(&config_path, new_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    // Should have providers
    let openai = profile
        .providers
        .get("openai")
        .expect("openai provider should exist");
    assert_eq!(openai.api_key(), Some("sk-openai-key"));
}

#[test]
fn test_legacy_migration_with_gemini() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let old_config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "gemini-2.5-pro"

[profiles.default.gemini]
api_key = "gemini-api-key"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    assert!(profile.providers.contains_key("gemini"));
    assert!(profile.gemini.is_none());

    let gemini = profile.providers.get("gemini").unwrap();
    assert_eq!(gemini.api_key(), Some("gemini-api-key"));
}

#[test]
fn test_legacy_migration_with_custom_endpoints() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let old_config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "gpt-4"

[profiles.default.openai]
api_key = "sk-openai"
api_endpoint = "https://custom-openai.example.com/v1"

[profiles.default.anthropic]
api_key = "sk-ant"
api_endpoint = "https://custom-anthropic.example.com"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let openai = profile.providers.get("openai").unwrap();
    assert_eq!(openai.api_key(), Some("sk-openai"));
    assert_eq!(
        openai.api_endpoint(),
        Some("https://custom-openai.example.com/v1")
    );

    let anthropic = profile.providers.get("anthropic").unwrap();
    assert_eq!(anthropic.api_key(), Some("sk-ant"));
    assert_eq!(
        anthropic.api_endpoint(),
        Some("https://custom-anthropic.example.com")
    );
}

#[test]
fn test_legacy_migration_with_anthropic_access_token() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let old_config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "claude-sonnet-4-5"

[profiles.default.anthropic]
access_token = "oauth-token-here"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let anthropic = profile.providers.get("anthropic").unwrap();
    assert_eq!(anthropic.access_token(), Some("oauth-token-here"));
    assert!(anthropic.api_key().is_none());
}

#[test]
fn test_legacy_migration_preserves_existing_providers() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    // Mixed format: new providers HashMap + old legacy field
    let old_config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "gpt-4"

[profiles.default.providers.openai]
type = "openai"
api_key = "new-format-key"

[profiles.default.anthropic]
api_key = "legacy-anthropic-key"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    // OpenAI should keep the new format key (not overwritten)
    let openai = profile.providers.get("openai").unwrap();
    assert_eq!(openai.api_key(), Some("new-format-key"));

    // Anthropic should be migrated from legacy
    let anthropic = profile.providers.get("anthropic").unwrap();
    assert_eq!(anthropic.api_key(), Some("legacy-anthropic-key"));
}

#[test]
fn test_legacy_migration_does_not_overwrite_existing() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    // Both new and legacy format for same provider - new should win
    let old_config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.providers.openai]
type = "openai"
api_key = "new-key"

[profiles.default.openai]
api_key = "legacy-key"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    // New format should be preserved, legacy ignored
    let openai = profile.providers.get("openai").unwrap();
    assert_eq!(openai.api_key(), Some("new-key"));
}

#[test]
fn test_custom_provider_config_parsing() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "litellm/anthropic/claude-opus"
eco_model = "litellm/openai/gpt-4-turbo"

[profiles.default.providers.litellm]
type = "custom"
api_endpoint = "http://localhost:4000"
api_key = "sk-litellm"
"#;
    std::fs::write(&config_path, config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    assert!(profile.providers.contains_key("litellm"));

    let litellm = profile.providers.get("litellm").unwrap();
    assert_eq!(litellm.api_key(), Some("sk-litellm"));
    assert_eq!(litellm.api_endpoint(), Some("http://localhost:4000"));
    assert_eq!(litellm.provider_type(), "custom");

    assert_eq!(
        profile.smart_model,
        Some("litellm/anthropic/claude-opus".to_string())
    );
    assert_eq!(
        profile.eco_model,
        Some("litellm/openai/gpt-4-turbo".to_string())
    );
}

#[test]
fn test_custom_provider_without_api_key() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "ollama/llama3"

[profiles.default.providers.ollama]
type = "custom"
api_endpoint = "http://localhost:11434/v1"
"#;
    std::fs::write(&config_path, config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let ollama = profile.providers.get("ollama").unwrap();
    assert!(ollama.api_key().is_none());
    assert_eq!(ollama.api_endpoint(), Some("http://localhost:11434/v1"));
}

#[test]
fn test_multiple_custom_providers() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "litellm/claude-opus"
eco_model = "ollama/llama3"

[profiles.default.providers.litellm]
type = "custom"
api_endpoint = "http://localhost:4000"
api_key = "sk-litellm"

[profiles.default.providers.ollama]
type = "custom"
api_endpoint = "http://localhost:11434/v1"
"#;
    std::fs::write(&config_path, config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    assert!(profile.providers.contains_key("litellm"));
    assert!(profile.providers.contains_key("ollama"));

    let litellm = profile.providers.get("litellm").unwrap();
    assert_eq!(litellm.api_endpoint(), Some("http://localhost:4000"));

    let ollama = profile.providers.get("ollama").unwrap();
    assert_eq!(ollama.api_endpoint(), Some("http://localhost:11434/v1"));
}

#[test]
fn test_mixed_builtin_and_custom_providers() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let config = r#"
[settings]

[profiles.default]
provider = "local"
smart_model = "claude-sonnet-4-5"
eco_model = "ollama/llama3"

[profiles.default.providers.anthropic]
type = "anthropic"
api_key = "sk-ant-key"

[profiles.default.providers.ollama]
type = "custom"
api_endpoint = "http://localhost:11434/v1"
"#;
    std::fs::write(&config_path, config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    // Built-in provider
    let anthropic = profile.providers.get("anthropic").unwrap();
    assert_eq!(anthropic.provider_type(), "anthropic");
    assert_eq!(anthropic.api_key(), Some("sk-ant-key"));

    // Custom provider
    let ollama = profile.providers.get("ollama").unwrap();
    assert_eq!(ollama.provider_type(), "custom");
    assert_eq!(ollama.api_endpoint(), Some("http://localhost:11434/v1"));
}

#[test]
fn test_legacy_migration_all_providers() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let old_config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.openai]
api_key = "sk-openai"

[profiles.default.anthropic]
api_key = "sk-ant"
access_token = "oauth-token"

[profiles.default.gemini]
api_key = "gemini-key"
api_endpoint = "https://custom-gemini.example.com"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    // All legacy fields should be cleared
    assert!(profile.openai.is_none());
    assert!(profile.anthropic.is_none());
    assert!(profile.gemini.is_none());

    // All should be migrated to providers HashMap
    assert_eq!(profile.providers.len(), 3);

    let openai = profile.providers.get("openai").unwrap();
    assert_eq!(openai.api_key(), Some("sk-openai"));

    let anthropic = profile.providers.get("anthropic").unwrap();
    assert_eq!(anthropic.api_key(), Some("sk-ant"));
    assert_eq!(anthropic.access_token(), Some("oauth-token"));

    let gemini = profile.providers.get("gemini").unwrap();
    assert_eq!(gemini.api_key(), Some("gemini-key"));
    assert_eq!(
        gemini.api_endpoint(),
        Some("https://custom-gemini.example.com")
    );
}

#[test]
fn test_legacy_migration_strips_chat_completions_from_endpoint() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    // Old config format with /chat/completions in endpoint
    let old_config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.openai]
api_key = "sk-openai"
api_endpoint = "http://localhost:4000/v1/chat/completions"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let openai = profile.providers.get("openai").unwrap();
    // /chat/completions should be stripped during migration
    assert_eq!(openai.api_endpoint(), Some("http://localhost:4000/v1"));
}

#[test]
fn test_legacy_migration_strips_chat_completions_with_trailing_slash() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let old_config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.anthropic]
api_key = "sk-ant"
api_endpoint = "http://localhost:4000/chat/completions/"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let anthropic = profile.providers.get("anthropic").unwrap();
    assert_eq!(anthropic.api_endpoint(), Some("http://localhost:4000"));
}

#[test]
fn test_legacy_endpoint_without_chat_completions_unchanged() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let old_config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.openai]
api_key = "sk-openai"
api_endpoint = "https://api.openai.com/v1"

[profiles.default.gemini]
api_key = "gemini-key"
api_endpoint = "https://custom-gemini.example.com"
"#;
    std::fs::write(&config_path, old_config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    // Endpoints without /chat/completions should remain unchanged
    let openai = profile.providers.get("openai").unwrap();
    assert_eq!(openai.api_endpoint(), Some("https://api.openai.com/v1"));

    let gemini = profile.providers.get("gemini").unwrap();
    assert_eq!(
        gemini.api_endpoint(),
        Some("https://custom-gemini.example.com")
    );
}

#[test]
fn test_new_format_strips_chat_completions_from_builtin_provider() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    // New format with /chat/completions in endpoint
    let config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.providers.openai]
type = "openai"
api_key = "sk-openai"
api_endpoint = "https://api.example.com/v1/chat/completions"
"#;
    std::fs::write(&config_path, config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let openai = profile.providers.get("openai").unwrap();
    assert_eq!(openai.api_endpoint(), Some("https://api.example.com/v1"));
}

#[test]
fn test_new_format_strips_chat_completions_from_custom_provider() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.providers.litellm]
type = "custom"
api_endpoint = "http://localhost:4000/chat/completions"
api_key = "sk-litellm"
"#;
    std::fs::write(&config_path, config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let litellm = profile.providers.get("litellm").unwrap();
    assert_eq!(litellm.api_endpoint(), Some("http://localhost:4000"));
}

#[test]
fn test_new_format_strips_chat_completions_from_multiple_providers() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");

    let config = r#"
[settings]

[profiles.default]
provider = "local"

[profiles.default.providers.openai]
type = "openai"
api_key = "sk-openai"
api_endpoint = "https://custom-openai.com/v1/chat/completions"

[profiles.default.providers.anthropic]
type = "anthropic"
api_key = "sk-ant"
api_endpoint = "https://custom-anthropic.com/chat/completions/"

[profiles.default.providers.litellm]
type = "custom"
api_endpoint = "http://localhost:4000/v1/chat/completions"
api_key = "sk-litellm"
"#;
    std::fs::write(&config_path, config).unwrap();

    let config_file = AppConfig::load_config_file(&config_path).unwrap();
    let profile = config_file.profiles.get("default").unwrap();

    let openai = profile.providers.get("openai").unwrap();
    assert_eq!(openai.api_endpoint(), Some("https://custom-openai.com/v1"));

    let anthropic = profile.providers.get("anthropic").unwrap();
    assert_eq!(
        anthropic.api_endpoint(),
        Some("https://custom-anthropic.com")
    );

    let litellm = profile.providers.get("litellm").unwrap();
    assert_eq!(litellm.api_endpoint(), Some("http://localhost:4000/v1"));
}
