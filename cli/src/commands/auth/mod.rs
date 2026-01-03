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
use std::path::PathBuf;

/// Authentication subcommands
#[derive(Subcommand, PartialEq, Debug)]
pub enum AuthCommands {
    /// Login to an LLM provider
    Login {
        /// Provider to authenticate with (e.g., "anthropic", "stakpak")
        #[arg(long)]
        provider: Option<String>,

        /// Profile to save credentials to (default: "all" for shared)
        #[arg(long, short)]
        profile: Option<String>,
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
            AuthCommands::Login { provider, profile } => {
                login::handle_login(&config_dir, provider.as_deref(), profile.as_deref()).await
            }
            AuthCommands::Logout { provider, profile } => {
                logout::handle_logout(&config_dir, provider.as_deref(), profile.as_deref())
            }
            AuthCommands::List { profile } => list::handle_list(&config_dir, profile.as_deref()),
        }
    }
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
