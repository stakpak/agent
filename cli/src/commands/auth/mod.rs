//! Authentication commands for LLM providers
//!
//! This module provides commands for authenticating with LLM providers
//! using OAuth or API keys.
//!
//! # Commands
//!
//! - `stakpak auth login` - Authenticate with a provider
//! - `stakpak auth logout` - Remove stored credentials
//! - `stakpak auth list` - List configured credentials

mod list;
mod login;
mod logout;

use crate::config::AppConfig;
use clap::Subcommand;
use stakpak_shared::auth_manager::AuthManager;
use stakpak_shared::models::auth::ProviderAuth;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Authentication subcommands
#[derive(Subcommand, PartialEq, Debug)]
pub enum AuthCommands {
    /// Login to an LLM provider
    Login {
        /// Provider to authenticate with (e.g., "anthropic", "openai", "gemini", "stakpak", "amazon-bedrock")
        #[arg(long, default_value = "stakpak")]
        provider: String,

        /// Profile to save credentials to
        #[arg(long, short)]
        profile: Option<String>,

        /// API key for non-interactive setup
        #[arg(long)]
        api_key: Option<String>,

        /// Custom API endpoint URL (provider-specific, non-interactive --api-key flow)
        #[arg(long)]
        endpoint: Option<String>,

        /// AWS region for Bedrock provider (e.g., "us-east-1")
        #[arg(long)]
        region: Option<String>,

        /// AWS named profile for Bedrock provider (from ~/.aws/config)
        #[arg(long)]
        profile_name: Option<String>,
    },

    /// Logout from an LLM provider
    Logout {
        /// Provider to logout from
        #[arg(long)]
        provider: Option<String>,

        /// Profile to remove credentials from
        #[arg(long, short)]
        profile: Option<String>,
    },

    /// List configured credentials
    List {
        /// Filter by profile
        #[arg(long, short)]
        profile: Option<String>,
    },
}

impl AuthCommands {
    /// Run the auth command
    pub async fn run(self, config: AppConfig) -> Result<(), String> {
        // Get the config directory from the config path
        let config_dir = get_config_dir(&config)?;

        match self {
            AuthCommands::Login {
                provider,
                profile,
                api_key,
                endpoint,
                region,
                profile_name,
            } => {
                login::handle_login(
                    &config_dir,
                    &provider,
                    profile.as_deref(),
                    api_key,
                    endpoint,
                    region,
                    profile_name,
                )
                .await
            }
            AuthCommands::Logout { provider, profile } => {
                logout::handle_logout(&config_dir, provider.as_deref(), profile.as_deref())
            }
            AuthCommands::List { profile } => list::handle_list(&config_dir, profile.as_deref()),
        }
    }
}

/// Where a credential was found — needed by logout to remove from the right file.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum CredentialSource {
    ConfigToml,
    AuthToml,
}

/// Collect all credentials from both config.toml and auth.toml.
///
/// Reads credentials from config.toml (new format) first, then fills in any
/// missing ones from auth.toml (legacy). Config.toml entries take precedence
/// when both sources have the same profile+provider pair.
pub(super) fn collect_all_credentials(
    config_dir: &Path,
) -> HashMap<String, HashMap<String, (ProviderAuth, CredentialSource)>> {
    let mut all_credentials: HashMap<String, HashMap<String, (ProviderAuth, CredentialSource)>> =
        HashMap::new();

    // 1. Read from config.toml (new format)
    let config_path = config_dir.join("config.toml");
    if let Ok(config_file) = AppConfig::load_config_file(&config_path) {
        for (profile_name, profile_config) in &config_file.profiles {
            for (provider_name, provider_config) in &profile_config.providers {
                if let Some(auth) = provider_config.get_auth() {
                    all_credentials
                        .entry(profile_name.clone())
                        .or_default()
                        .insert(provider_name.clone(), (auth, CredentialSource::ConfigToml));
                }
            }
        }
    }

    // 2. Read from auth.toml (legacy) — only add if not already in config.toml
    if let Ok(auth_manager) = AuthManager::new(config_dir) {
        for (profile_name, providers) in auth_manager.list() {
            for (provider_name, auth) in providers {
                let profile_creds = all_credentials.entry(profile_name.clone()).or_default();
                if !profile_creds.contains_key(provider_name.as_str()) {
                    profile_creds.insert(
                        provider_name.clone(),
                        (auth.clone(), CredentialSource::AuthToml),
                    );
                }
            }
        }
    }

    all_credentials
}

/// Get the config directory from the app config
fn get_config_dir(config: &AppConfig) -> Result<PathBuf, String> {
    if !config.config_path.is_empty() {
        // Use the directory containing the config file
        let path = PathBuf::from(&config.config_path);
        if let Some(parent) = path.parent() {
            return Ok(parent.to_path_buf());
        }
    }

    // Default to ~/.stakpak/
    dirs::home_dir()
        .map(|h| h.join(".stakpak"))
        .ok_or_else(|| "Could not determine home directory".to_string())
}
