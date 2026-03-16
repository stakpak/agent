//! Configuration templates for different provider setups
//!
//! # Provider Configuration
//!
//! Providers are configured in a `providers` HashMap where:
//! - The key becomes the model prefix for custom providers
//! - Built-in types: `openai`, `anthropic`, `gemini`
//! - Custom type: `custom` for OpenAI-compatible providers
//!
//! # Built-in Providers
//!
//! For built-in providers, you can use model names directly without a prefix:
//!
//! ```toml
//! [profiles.default]
//! smart_model = "claude-sonnet-4-5"  # auto-detected as anthropic
//! eco_model = "gpt-4-turbo"          # auto-detected as openai
//!
//! [profiles.default.providers.openai]
//! type = "openai"
//! api_key = "sk-..."  # or use auth.toml / OPENAI_API_KEY env var
//!
//! [profiles.default.providers.anthropic]
//! type = "anthropic"
//! api_key = "sk-ant-..."  # or use auth.toml / ANTHROPIC_API_KEY env var
//! ```
//!
//! # Custom Providers
//!
//! For OpenAI-compatible providers (Ollama, vLLM, etc.):
//!
//! ```toml
//! [profiles.default]
//! smart_model = "claude-sonnet-4-5"  # built-in, auto-detected
//! eco_model = "offline/llama3"       # custom provider
//!
//! [profiles.default.providers.offline]
//! type = "custom"
//! api_endpoint = "http://localhost:11434/v1"
//! # api_key is optional for local providers
//! ```
//!
//! # Model Routing
//!
//! - Built-in models: `claude-sonnet-4-5`, `gpt-4`, `gemini-2.5-pro` → auto-detected
//! - Custom providers: `offline/llama3` → routes to `offline` provider, sends `llama3` to API
//!
//! # Credential Resolution Order
//!
//! For built-in providers, credentials are resolved in order:
//! 1. `auth.toml` (profile-specific)
//! 2. `auth.toml` (shared/all)
//! 3. `config.toml` providers section
//! 4. Environment variable (e.g., `ANTHROPIC_API_KEY`)

use crate::config::ProfileConfig;
use crate::config::ProviderType;
use stakpak_shared::models::llm::ProviderConfig;

/// Default model for all new profiles
pub const DEFAULT_MODEL: &str = "claude-opus-4-6";

/// Generate OpenAI profile configuration (credentials stored separately in config.toml auth field)
pub fn generate_openai_profile() -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some("gpt-4.1".to_string()),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        "openai".to_string(),
        ProviderConfig::OpenAI {
            api_key: None,
            api_endpoint: None,
            auth: None,
        },
    );
    profile
}

/// Generate Gemini profile configuration (credentials stored separately in config.toml auth field)
pub fn generate_gemini_profile() -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some("gemini-2.5-pro".to_string()),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        "gemini".to_string(),
        ProviderConfig::Gemini {
            api_key: None,
            api_endpoint: None,
            auth: None,
        },
    );
    profile
}

/// Generate GitHub Copilot profile configuration (credentials stored via OAuth device flow)
pub fn generate_github_copilot_profile() -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some("github-copilot/gpt-4o".to_string()),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        "github-copilot".to_string(),
        ProviderConfig::GitHubCopilot {
            api_endpoint: None,
            auth: None,
        },
    );
    profile
}

/// Generate Anthropic profile configuration (credentials stored separately in config.toml auth field)
pub fn generate_anthropic_profile() -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some(DEFAULT_MODEL.to_string()),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        "anthropic".to_string(),
        ProviderConfig::Anthropic {
            api_key: None,
            api_endpoint: None,
            access_token: None,
            auth: None,
        },
    );
    profile
}

/// Generate custom provider profile configuration
///
/// This creates a profile with a custom OpenAI-compatible provider (e.g., LiteLLM, Ollama).
/// Model names are automatically prefixed with the provider name.
///
/// # Arguments
/// * `provider_name` - Name of the provider (e.g., "litellm", "ollama") - becomes the model prefix
/// * `api_endpoint` - API endpoint URL as required by the provider (e.g., "http://localhost:4000")
/// * `api_key` - Optional API key (some providers like Ollama don't require auth)
/// * `model_name` - Model name/path (e.g., "claude-opus" or "anthropic/claude-opus" for LiteLLM)
///
/// # Example
/// For LiteLLM with Anthropic models:
/// ```ignore
/// generate_custom_provider_profile(
///     "litellm".to_string(),
///     "http://localhost:4000".to_string(),
///     Some("sk-litellm".to_string()),
///     "anthropic/claude-opus".to_string(),  // Will become "litellm/anthropic/claude-opus"
/// )
/// ```
pub fn generate_custom_provider_profile(
    provider_name: String,
    api_endpoint: String,
    api_key: Option<String>,
    model_name: String,
) -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some(format!("{}/{}", provider_name, model_name)),
        ..ProfileConfig::default()
    };
    profile.providers.insert(
        provider_name,
        ProviderConfig::Custom {
            api_key,
            api_endpoint,
            auth: None,
        },
    );
    profile
}

/// Built-in provider types for multi-provider configuration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BuiltinProvider {
    OpenAI,
    Gemini,
    Anthropic,
}

impl BuiltinProvider {
    pub fn display_name(&self) -> &'static str {
        match self {
            BuiltinProvider::OpenAI => "OpenAI",
            BuiltinProvider::Gemini => "Gemini",
            BuiltinProvider::Anthropic => "Anthropic",
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            BuiltinProvider::OpenAI => "gpt-4.1",
            BuiltinProvider::Gemini => "gemini-2.5-pro",
            BuiltinProvider::Anthropic => DEFAULT_MODEL,
        }
    }
}

/// Configuration for a provider in multi-provider setup
#[derive(Debug, Clone)]
pub struct ProviderSetup {
    pub provider: BuiltinProvider,
    pub api_key: String,
}

/// Generate a multi-provider profile configuration.
///
/// This creates a profile with multiple providers configured, allowing the user
/// to switch between them using the `/model` command at runtime.
///
/// # Arguments
/// * `providers` - List of providers to configure with their API keys
/// * `default_model` - The model to use by default (e.g., "claude-opus-4-5")
pub fn generate_multi_provider_profile(
    providers: Vec<ProviderSetup>,
    default_model: String,
) -> ProfileConfig {
    let mut profile = ProfileConfig {
        provider: Some(ProviderType::Local),
        model: Some(default_model),
        ..ProfileConfig::default()
    };

    for setup in providers {
        match setup.provider {
            BuiltinProvider::OpenAI => {
                profile.providers.insert(
                    "openai".to_string(),
                    ProviderConfig::OpenAI {
                        api_key: Some(setup.api_key),
                        api_endpoint: None,
                        auth: None,
                    },
                );
            }
            BuiltinProvider::Gemini => {
                profile.providers.insert(
                    "gemini".to_string(),
                    ProviderConfig::Gemini {
                        api_key: Some(setup.api_key),
                        api_endpoint: None,
                        auth: None,
                    },
                );
            }
            BuiltinProvider::Anthropic => {
                profile.providers.insert(
                    "anthropic".to_string(),
                    ProviderConfig::Anthropic {
                        api_key: Some(setup.api_key),
                        api_endpoint: None,
                        access_token: None,
                        auth: None,
                    },
                );
            }
        }
    }

    profile
}

/// Convert profile config to TOML string for preview
pub fn config_to_toml_preview(profile: &ProfileConfig, profile_name: &str) -> String {
    let mut toml = format!("[profiles.{}]\n", profile_name);

    if let Some(provider) = &profile.provider {
        toml.push_str(&format!(
            "provider = \"{}\"\n",
            match provider {
                ProviderType::Remote => "remote",
                ProviderType::Local => "local",
            }
        ));
    }

    if let Some(ref model) = profile.model {
        toml.push_str(&format!("model = \"{}\"\n", model));
    }

    // Output providers in the new unified format
    for (name, config) in &profile.providers {
        toml.push_str(&format!(
            "\n[profiles.{}.providers.{}]\n",
            profile_name, name
        ));

        match config {
            ProviderConfig::OpenAI {
                api_key,
                api_endpoint,
                ..
            } => {
                toml.push_str("type = \"openai\"\n");
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Anthropic {
                api_key,
                api_endpoint,
                access_token,
                ..
            } => {
                toml.push_str("type = \"anthropic\"\n");
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
                if let Some(token) = access_token {
                    toml.push_str(&format!(
                        "access_token = \"{}\"\n",
                        if token.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Gemini {
                api_key,
                api_endpoint,
                ..
            } => {
                toml.push_str("type = \"gemini\"\n");
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
                ..
            } => {
                toml.push_str("type = \"custom\"\n");
                toml.push_str(&format!("api_endpoint = \"{}\"\n", api_endpoint));
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
            }
            ProviderConfig::Stakpak {
                api_key,
                api_endpoint,
                ..
            } => {
                toml.push_str("type = \"stakpak\"\n");
                if let Some(key) = api_key {
                    toml.push_str(&format!(
                        "api_key = \"{}\"\n",
                        if key.is_empty() { "" } else { "***" }
                    ));
                }
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
            }
            ProviderConfig::Bedrock {
                region,
                profile_name,
            } => {
                toml.push_str("type = \"amazon-bedrock\"\n");
                toml.push_str(&format!("region = \"{}\"\n", region));
                if let Some(profile) = profile_name {
                    toml.push_str(&format!("profile_name = \"{}\"\n", profile));
                }
            }
            ProviderConfig::GitHubCopilot { api_endpoint, auth } => {
                toml.push_str("type = \"github-copilot\"\n");
                if let Some(endpoint) = api_endpoint {
                    toml.push_str(&format!("api_endpoint = \"{}\"\n", endpoint));
                }
                if let Some(a) = auth {
                    toml.push_str(&format!("# auth: {} (set)\n", a.auth_type_display()));
                }
            }
        }
    }

    toml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_custom_provider_profile() {
        let profile = generate_custom_provider_profile(
            "litellm".to_string(),
            "http://localhost:4000".to_string(),
            Some("sk-1234".to_string()),
            "claude-opus".to_string(),
        );

        assert!(matches!(profile.provider, Some(ProviderType::Local)));
        assert_eq!(profile.model, Some("litellm/claude-opus".to_string()));

        // Check providers HashMap
        let provider = profile
            .providers
            .get("litellm")
            .expect("litellm provider should exist");
        match provider {
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
                ..
            } => {
                assert_eq!(api_endpoint, "http://localhost:4000");
                assert_eq!(api_key, &Some("sk-1234".to_string()));
            }
            _ => panic!("Expected Custom provider"),
        }
    }

    #[test]
    fn test_generate_custom_provider_profile_without_api_key() {
        let profile = generate_custom_provider_profile(
            "ollama".to_string(),
            "http://localhost:11434/v1".to_string(),
            None,
            "llama3".to_string(),
        );

        assert!(matches!(profile.provider, Some(ProviderType::Local)));
        assert_eq!(profile.model, Some("ollama/llama3".to_string()));

        let provider = profile
            .providers
            .get("ollama")
            .expect("ollama provider should exist");
        match provider {
            ProviderConfig::Custom {
                api_key,
                api_endpoint,
                ..
            } => {
                assert_eq!(api_endpoint, "http://localhost:11434/v1");
                assert!(api_key.is_none());
            }
            _ => panic!("Expected Custom provider"),
        }
    }

    #[test]
    fn test_config_to_toml_preview_with_custom_provider() {
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            model: Some("litellm/claude-opus".to_string()),
            ..ProfileConfig::default()
        };
        profile.providers.insert(
            "litellm".to_string(),
            ProviderConfig::Custom {
                api_endpoint: "http://localhost:4000".to_string(),
                api_key: Some("sk-1234".to_string()),
                auth: None,
            },
        );

        let toml = config_to_toml_preview(&profile, "default");

        assert!(toml.contains("provider = \"local\""));
        assert!(toml.contains("model = \"litellm/claude-opus\""));
        assert!(toml.contains("[profiles.default.providers.litellm]"));
        assert!(toml.contains("type = \"custom\""));
        assert!(toml.contains("api_endpoint = \"http://localhost:4000\""));
        assert!(toml.contains("api_key = \"***\"")); // Should be masked
    }

    #[test]
    fn test_config_to_toml_preview_custom_provider_no_api_key() {
        let mut profile = ProfileConfig {
            provider: Some(ProviderType::Local),
            model: Some("ollama/llama3".to_string()),
            ..ProfileConfig::default()
        };
        profile.providers.insert(
            "ollama".to_string(),
            ProviderConfig::Custom {
                api_endpoint: "http://localhost:11434/v1".to_string(),
                api_key: None,
                auth: None,
            },
        );

        let toml = config_to_toml_preview(&profile, "default");

        assert!(toml.contains("[profiles.default.providers.ollama]"));
        assert!(toml.contains("type = \"custom\""));
        assert!(toml.contains("api_endpoint = \"http://localhost:11434/v1\""));
        // api_key line should not appear since it's None
        let lines: Vec<&str> = toml.lines().collect();
        let has_api_key_in_ollama_section = lines
            .iter()
            .skip_while(|l| !l.contains("providers.ollama"))
            .take_while(|l| !l.starts_with('[') || l.contains("providers.ollama"))
            .any(|l| l.contains("api_key"));
        assert!(!has_api_key_in_ollama_section);
    }

    #[test]
    fn test_generate_multi_provider_profile() {
        let providers = vec![
            ProviderSetup {
                provider: BuiltinProvider::Anthropic,
                api_key: "sk-ant-xxx".to_string(),
            },
            ProviderSetup {
                provider: BuiltinProvider::OpenAI,
                api_key: "sk-xxx".to_string(),
            },
        ];

        let profile = generate_multi_provider_profile(providers, DEFAULT_MODEL.to_string());

        assert!(matches!(profile.provider, Some(ProviderType::Local)));
        assert_eq!(profile.model, Some(DEFAULT_MODEL.to_string()));
        assert!(profile.providers.contains_key("anthropic"));
        assert!(profile.providers.contains_key("openai"));
    }
}
