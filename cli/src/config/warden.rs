//! Warden (runtime security) configuration.

use serde::{Deserialize, Serialize};

// Re-export from the shared crate — single source of truth for container layout.
pub use stakpak_shared::container::stakpak_agent_default_mounts;

/// Configuration for the Warden runtime security system.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WardenConfig {
    /// Whether warden is enabled
    pub enabled: bool,
    /// Volume mounts for the warden container
    #[serde(default)]
    pub volumes: Vec<String>,
}

impl WardenConfig {
    /// Create a readonly profile configuration for warden.
    pub(crate) fn readonly_profile() -> Self {
        WardenConfig {
            enabled: true,
            volumes: stakpak_agent_default_mounts(),
        }
    }
}
