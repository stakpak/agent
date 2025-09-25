use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::openai::ToolCall;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const AUTO_APPROVE_CONFIG_PATH: &str = ".stakpak/session/auto_approve.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum AutoApprovePolicy {
    Auto,
    #[default]
    Prompt,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoApproveConfig {
    pub enabled: bool,
    pub default_policy: AutoApprovePolicy,
    pub tools: HashMap<String, AutoApprovePolicy>,
    pub command_patterns: CommandPatterns,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandPatterns {
    pub safe_readonly: Vec<String>,
    pub sensitive_destructive: Vec<String>,
    pub interactive_required: Vec<String>,
}

impl Default for AutoApproveConfig {
    fn default() -> Self {
        let mut tools = HashMap::new();

        // Auto-approve tools (always auto-approve):
        tools.insert("view".to_string(), AutoApprovePolicy::Auto);
        tools.insert("generate_password".to_string(), AutoApprovePolicy::Auto);
        tools.insert("search_docs".to_string(), AutoApprovePolicy::Auto);
        tools.insert("search_memory".to_string(), AutoApprovePolicy::Auto);
        tools.insert("read_rulebook".to_string(), AutoApprovePolicy::Auto);
        tools.insert("local_code_search".to_string(), AutoApprovePolicy::Auto);
        tools.insert("get_all_tasks".to_string(), AutoApprovePolicy::Auto);
        tools.insert("get_task_details".to_string(), AutoApprovePolicy::Auto);
        tools.insert("wait_for_tasks".to_string(), AutoApprovePolicy::Auto);

        // Prompt tools (always require confirmation):
        tools.insert("create".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("str_replace".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("generate_code".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("run_command".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("run_command_task".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("subagent_task".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("cancel_task".to_string(), AutoApprovePolicy::Prompt);

        AutoApproveConfig {
            enabled: true,
            default_policy: AutoApprovePolicy::Prompt,
            tools,
            command_patterns: CommandPatterns::default(),
        }
    }
}

pub struct AutoApproveManager {
    pub config: AutoApproveConfig,
    pub config_path: PathBuf,
}

impl AutoApproveManager {
    pub fn new(auto_approve_tools: Option<&Vec<String>>) -> Self {
        match Self::try_new(auto_approve_tools) {
            Ok(manager) => manager,
            Err(e) => {
                let config_path = PathBuf::from(AUTO_APPROVE_CONFIG_PATH);
                let config = Self::merge_profile_and_session_config(auto_approve_tools, None);
                eprintln!("Failed to load auto-approve config: {}", e);
                // Try to save the default config even if loading failed
                if let Err(e) = config.save(&config_path) {
                    eprintln!("Warning: Failed to save auto-approve config: {}", e);
                }

                AutoApproveManager {
                    config,
                    config_path,
                }
            }
        }
    }
}

impl Default for AutoApproveManager {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AutoApproveManager {
    pub fn try_new(auto_approve_tools: Option<&Vec<String>>) -> Result<Self, String> {
        let config_path = Self::get_config_path()?;
        let session_config = if config_path.exists() {
            // Load existing session config
            Some(Self::load_config(&config_path)?)
        } else {
            None
        };

        // Create merged config: profile defaults + session overrides
        let config =
            Self::merge_profile_and_session_config(auto_approve_tools, session_config.as_ref());

        Ok(AutoApproveManager {
            config,
            config_path,
        })
    }

    fn get_config_path() -> Result<PathBuf, String> {
        // Always use local config in current working directory
        let local_config = Path::new(AUTO_APPROVE_CONFIG_PATH);
        Ok(local_config.to_path_buf())
    }

    fn load_config(config_path: &Path) -> Result<AutoApproveConfig, String> {
        if !config_path.exists() {
            // Create default config
            let config = AutoApproveConfig::default();
            config.save(config_path)?;
            return Ok(config);
        }

        let content = fs::read_to_string(config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let mut config: AutoApproveConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        config
            .tools
            .insert("run_command".to_string(), AutoApprovePolicy::Prompt);

        // Save the updated config back to file
        config.save(config_path)?;

        Ok(config)
    }

    pub fn should_auto_approve(&self, tool_call: &ToolCall) -> bool {
        if !self.config.enabled {
            return false;
        }

        let policy = self.get_policy_for_tool(tool_call);

        match policy {
            AutoApprovePolicy::Auto => true,
            AutoApprovePolicy::Never => false,
            AutoApprovePolicy::Prompt => false,
        }
    }

    pub fn get_policy_for_tool(&self, tool_call: &ToolCall) -> AutoApprovePolicy {
        let tool_name = &tool_call.function.name;

        // Check if there's a specific policy for this tool
        if let Some(policy) = self.config.tools.get(tool_name) {
            return policy.clone();
        }

        // Return default policy
        self.config.default_policy.clone()
    }

    pub fn get_policy_for_tool_name(&self, tool_name: &str) -> AutoApprovePolicy {
        // Check if there's a specific policy for this tool
        if let Some(policy) = self.config.tools.get(tool_name) {
            return policy.clone();
        }

        // Return default policy
        self.config.default_policy.clone()
    }

    pub fn update_tool_policy(
        &mut self,
        tool_name: &str,
        policy: AutoApprovePolicy,
    ) -> Result<(), String> {
        self.config.tools.insert(tool_name.to_string(), policy);
        self.save_config()
    }

    pub fn update_command_patterns(
        &mut self,
        pattern_type: &str,
        patterns: Vec<String>,
    ) -> Result<(), String> {
        match pattern_type {
            "safe_readonly" => {
                self.config.command_patterns.safe_readonly = patterns;
            }
            "sensitive_destructive" => {
                self.config.command_patterns.sensitive_destructive = patterns;
            }
            "interactive_required" => {
                self.config.command_patterns.interactive_required = patterns;
            }
            _ => return Err(format!("Unknown pattern type: {}", pattern_type)),
        }
        // If no patterns are configured, revert run_command back to Prompt
        self.config
            .tools
            .insert("run_command".to_string(), AutoApprovePolicy::Prompt);
        self.save_config()
    }

    pub fn set_default_policy(&mut self, policy: AutoApprovePolicy) -> Result<(), String> {
        self.config.default_policy = policy;
        self.save_config()
    }

    pub fn toggle_enabled(&mut self) -> Result<(), String> {
        self.config.enabled = !self.config.enabled;
        self.save_config()
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn get_config(&self) -> &AutoApproveConfig {
        &self.config
    }

    /// Returns a filtered list of tool calls that require user approval (prompt)
    /// This excludes tool calls that are auto-approved or should never be approved
    pub fn get_prompt_tool_calls(&self, tool_calls: &[ToolCall]) -> Vec<ToolCall> {
        tool_calls
            .iter()
            .filter(|tool_call| {
                if !self.config.enabled {
                    return true; // If auto-approve is disabled, all tools need prompting
                }

                let policy = self.get_policy_for_tool(tool_call);
                match policy {
                    AutoApprovePolicy::Auto => false,  // Skip auto-approved tools
                    AutoApprovePolicy::Never => false, // Skip tools that should never be approved
                    AutoApprovePolicy::Prompt => true, // Always prompt for these
                }
            })
            .cloned()
            .collect()
    }

    /// Returns a filtered list of tool calls that can be auto-approved
    pub fn get_auto_approve_tool_calls(&self, tool_calls: &[ToolCall]) -> Vec<ToolCall> {
        tool_calls
            .iter()
            .filter(|tool_call| self.should_auto_approve(tool_call))
            .cloned()
            .collect()
    }

    fn save_config(&self) -> Result<(), String> {
        // Ensure directory exists
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(&self.config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&self.config_path, json)
            .map_err(|e| format!("Failed to write config file: {}", e))
    }

    /// Merge profile auto-approve settings with existing session config.
    /// Session settings take precedence over profile defaults.
    fn merge_profile_and_session_config(
        auto_approve_tools: Option<&Vec<String>>,
        session_config: Option<&AutoApproveConfig>,
    ) -> AutoApproveConfig {
        // Start with default config
        let mut config = AutoApproveConfig::default();

        // Apply profile auto-approve tools (these override default config)
        if let Some(profile_tools) = auto_approve_tools {
            for tool_name in profile_tools {
                config
                    .tools
                    .insert(tool_name.clone(), AutoApprovePolicy::Auto);
            }
        }

        // If we have existing session config, merge it in (session takes precedence over profile)
        if let Some(session) = session_config {
            // Preserve session-level settings
            config.enabled = session.enabled;
            config.default_policy = session.default_policy.clone();
            config.command_patterns = session.command_patterns.clone();

            // Session tool policies override both default and profile settings
            for (tool_name, policy) in &session.tools {
                // Only override if this tool is NOT in the profile auto_approve list
                // This ensures profile settings take precedence over session for profile-specified tools
                if let Some(profile_tools) = auto_approve_tools {
                    if !profile_tools.contains(tool_name) {
                        config.tools.insert(tool_name.clone(), policy.clone());
                    }
                } else {
                    config.tools.insert(tool_name.clone(), policy.clone());
                }
            }
        }

        config
    }
}

impl AutoApproveConfig {
    fn save(&self, path: &Path) -> Result<(), String> {
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(path, json).map_err(|e| format!("Failed to write config file: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_profile_and_session_config_profile_only() {
        let profile_tools = vec!["read".to_string(), "search".to_string()];
        let config =
            AutoApproveManager::merge_profile_and_session_config(Some(&profile_tools), None);

        // Profile tools should be set to Auto
        assert_eq!(config.tools.get("read"), Some(&AutoApprovePolicy::Auto));
        assert_eq!(config.tools.get("search"), Some(&AutoApprovePolicy::Auto));

        // Default config should still have its built-in tools
        assert_eq!(config.tools.get("view"), Some(&AutoApprovePolicy::Auto));
        assert_eq!(config.tools.get("create"), Some(&AutoApprovePolicy::Prompt));
    }

    #[test]
    fn test_merge_profile_and_session_config_session_precedence() {
        let profile_tools = vec!["read".to_string(), "write".to_string()];

        // Create session config that overrides profile settings
        let mut session_config = AutoApproveConfig::default();
        session_config
            .tools
            .insert("read".to_string(), AutoApprovePolicy::Prompt); // Try to override profile (should NOT work)
        session_config
            .tools
            .insert("delete".to_string(), AutoApprovePolicy::Auto); // Session-only
        session_config.enabled = false; // Override default

        let config = AutoApproveManager::merge_profile_and_session_config(
            Some(&profile_tools),
            Some(&session_config),
        );

        // Profile settings should take precedence for profile tools
        assert_eq!(config.tools.get("read"), Some(&AutoApprovePolicy::Auto)); // Profile wins
        assert_eq!(config.tools.get("write"), Some(&AutoApprovePolicy::Auto)); // Profile default
        assert_eq!(config.tools.get("delete"), Some(&AutoApprovePolicy::Auto)); // Session-only
        assert_eq!(config.enabled, false); // Session override
    }

    #[test]
    fn test_merge_profile_and_session_config_no_profile() {
        let mut session_config = AutoApproveConfig::default();
        session_config
            .tools
            .insert("custom".to_string(), AutoApprovePolicy::Never);

        let config =
            AutoApproveManager::merge_profile_and_session_config(None, Some(&session_config));

        // Should preserve session config without profile additions
        assert_eq!(config.tools.get("custom"), Some(&AutoApprovePolicy::Never));
        // Default tools should still be present
        assert_eq!(config.tools.get("view"), Some(&AutoApprovePolicy::Auto));
    }

    #[test]
    fn test_merge_profile_and_session_config_empty_profile() {
        let profile_tools = vec![];
        let config =
            AutoApproveManager::merge_profile_and_session_config(Some(&profile_tools), None);

        // Should just have default config
        assert_eq!(config.tools.get("view"), Some(&AutoApprovePolicy::Auto));
        assert_eq!(config.tools.get("create"), Some(&AutoApprovePolicy::Prompt));
    }
}
