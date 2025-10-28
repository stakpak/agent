use serde::{Deserialize, Serialize};

/// Sandbox mode determines what level of restrictions to apply
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxMode {
    #[serde(rename = "readonly")]
    ReadOnly,

    #[serde(rename = "workspace-write")]
    WorkspaceWrite,

    #[serde(rename = "full-access")]
    FullAccess,
}

/// Network policy for controlling network access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// Whether to allow network access at all
    pub allow_network: bool,

    /// Log network syscalls (connect, socket, etc.)
    pub log_network: bool,

    /// Additional rules for specific commands
    pub command_rules: Vec<CommandRule>,
}

/// Command-specific rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRule {
    /// Command pattern (exact match or regex)
    pub pattern: String,

    /// Whether this command should have network access
    pub allow_network: bool,

    /// Whether this command is considered destructive
    pub destructive: bool,
}

/// Filesystem policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    /// Allowed read-only paths
    pub read_only_paths: Vec<String>,

    /// Allowed write paths
    pub write_paths: Vec<String>,

    /// Blocked paths (always denied)
    pub blocked_paths: Vec<String>,
}

/// Audit logging policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPolicy {
    /// Enable audit logging
    pub enabled: bool,

    /// Log file path
    pub log_file: Option<String>,

    /// Log level: debug, info, warn, error
    pub log_level: Option<String>,

    /// What to log
    pub log_file_access: bool,
    pub log_network: bool,
    pub log_commands: bool,
    pub log_security_blocks: bool,
}

/// Main sandbox policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// Sandbox mode
    pub mode: SandboxMode,

    /// Network policy
    pub network: NetworkPolicy,

    /// Filesystem policy
    pub filesystem: Option<FilesystemPolicy>,

    /// Audit logging policy
    pub audit: AuditPolicy,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            mode: SandboxMode::ReadOnly,
            network: NetworkPolicy {
                allow_network: true,
                log_network: true,
                command_rules: vec![
                    CommandRule {
                        pattern: "rm.*-rf".to_string(),
                        allow_network: false,
                        destructive: true,
                    },
                    CommandRule {
                        pattern: "drop.*database".to_string(),
                        allow_network: false,
                        destructive: true,
                    },
                ],
            },
            filesystem: None,
            audit: AuditPolicy {
                enabled: true,
                log_file: Some(".stakpak/sandbox/audit.log".to_string()),
                log_level: Some("info".to_string()),
                log_file_access: true,
                log_network: true,
                log_commands: true,
                log_security_blocks: true,
            },
        }
    }
}

impl SandboxPolicy {
    /// Load policy from TOML file
    pub fn from_file<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let policy: Self = toml::from_str(&content)?;
        Ok(policy)
    }

    /// Save policy to TOML file
    pub fn to_file<P: AsRef<std::path::Path>>(
        &self,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Load policy from .stakpak/sandbox/sandbox-policy.toml in current working directory
    /// Falls back to default if file doesn't exist
    pub fn load_or_default() -> Self {
        if let Ok(cwd) = std::env::current_dir() {
            let policy_path = cwd
                .join(".stakpak")
                .join("sandbox")
                .join("sandbox-policy.toml");

            // Create directory structure
            if let Some(parent) = policy_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            // Try to load from file
            if policy_path.exists() {
                match Self::from_file(&policy_path) {
                    Ok(policy) => {
                        log::info!("Loaded sandbox policy from: {:?}", policy_path);
                        return policy;
                    }
                    Err(e) => {
                        log::warn!("Failed to load policy from {:?}: {}", policy_path, e);
                    }
                }
            }

            // Create default policy file if it doesn't exist
            let default_policy = Self::default();
            if let Err(e) = default_policy.to_file(&policy_path) {
                log::warn!("Failed to save default policy: {}", e);
            } else {
                log::info!("Created default sandbox policy at: {:?}", policy_path);
            }
        }

        Self::default()
    }

    /// Check if a command should be allowed network access
    pub fn should_allow_network(&self, command: &str) -> bool {
        // First check global policy
        if !self.network.allow_network {
            return false;
        }

        // Then check command-specific rules
        for rule in &self.network.command_rules {
            if self.matches_pattern(command, &rule.pattern) {
                return rule.allow_network;
            }
        }

        // Default to global policy
        self.network.allow_network
    }

    /// Check if a command is destructive
    pub fn is_destructive(&self, command: &str) -> bool {
        for rule in &self.network.command_rules {
            if rule.destructive && self.matches_pattern(command, &rule.pattern) {
                return true;
            }
        }
        false
    }

    fn matches_pattern(&self, input: &str, pattern: &str) -> bool {
        // Try regex first
        if let Ok(re) = regex::Regex::new(pattern) {
            return re.is_match(input);
        }

        // Fall back to simple contains
        input.contains(pattern)
    }
}
