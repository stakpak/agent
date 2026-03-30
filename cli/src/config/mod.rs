//! Configuration management for the Stakpak CLI.
//!
//! This module handles loading, saving, and managing configuration including:
//! - Profile configurations (per-environment settings)
//! - Provider configurations (OpenAI, Anthropic, Gemini)
//! - Rulebook filtering
//! - Warden (runtime security) settings
//! - Authentication and credential resolution
//! - Models cache from models.dev

mod app;
mod file;
pub mod models_cache;
pub(crate) mod openai_resolver;
mod profile;
pub(crate) mod profile_resolver;
mod rulebook;
mod types;
pub(crate) mod warden;

#[cfg(test)]
mod tests;

// Re-export public types
pub use app::AppConfig;
pub use file::ConfigFile;
pub use models_cache::ModelsCache;
pub use profile::{ProfileConfig, format_recent_model_id};
pub use types::ProviderType;

// Re-export for internal use (used by tests and submodules)
#[allow(unused_imports)]
pub use rulebook::RulebookConfig;
#[allow(unused_imports)]
pub use types::Settings;
#[allow(unused_imports)]
pub use warden::WardenConfig;

// Constants
pub const STAKPAK_API_ENDPOINT: &str = "https://apiv2.stakpak.dev";
pub const STAKPAK_CONFIG_PATH: &str = ".stakpak/config.toml";
