use crate::app::InputEvent;
use crate::constants::AUTO_APPROVE_CONFIG_PATH;
use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_shared::utils::{backward_compatibility_mapping, strip_tool_name};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

const SHELL_TOOLS: &[&str] = &[
    "run_command",
    "run_command_task",
    "run_remote_command",
    "run_remote_command_task",
];

const BASE_SHELL_TOOL: &str = "run_command";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum AutoApprovePolicy {
    /// Least restrictive — auto-approve.
    Auto = 0,
    /// Prompt the user.
    #[default]
    Prompt = 1,
    /// Most restrictive — hard reject.
    Never = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoApproveConfig {
    pub enabled: bool,
    pub default_policy: AutoApprovePolicy,
    pub tools: HashMap<String, AutoApprovePolicy>,
    pub command_patterns: CommandPatterns,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
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
        tools.insert("load_skill".to_string(), AutoApprovePolicy::Auto);
        tools.insert("local_code_search".to_string(), AutoApprovePolicy::Auto);
        tools.insert("get_all_tasks".to_string(), AutoApprovePolicy::Auto);
        tools.insert("get_task_details".to_string(), AutoApprovePolicy::Auto);
        tools.insert("wait_for_tasks".to_string(), AutoApprovePolicy::Auto);
        tools.insert(
            "run_command::stakpak::version".to_string(),
            AutoApprovePolicy::Auto,
        );
        tools.insert(
            "run_command::stakpak::account".to_string(),
            AutoApprovePolicy::Auto,
        );
        tools.insert(
            "run_command::stakpak::completion".to_string(),
            AutoApprovePolicy::Auto,
        );
        tools.insert(
            "run_command::stakpak::ak".to_string(),
            AutoApprovePolicy::Auto,
        );
        tools.insert(
            "run_command::stakpak::sessions".to_string(),
            AutoApprovePolicy::Auto,
        );
        tools.insert(
            "run_command::stakpak::config::show".to_string(),
            AutoApprovePolicy::Auto,
        );
        tools.insert(
            "run_command::stakpak::config::sample".to_string(),
            AutoApprovePolicy::Auto,
        );

        // Prompt tools (always require confirmation):
        tools.insert("create".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("str_replace".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("generate_code".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("run_command".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("run_command_task".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("run_remote_command".to_string(), AutoApprovePolicy::Prompt);
        tools.insert(
            "run_remote_command_task".to_string(),
            AutoApprovePolicy::Prompt,
        );
        tools.insert(
            "dynamic_subagent_task".to_string(),
            AutoApprovePolicy::Prompt,
        );
        tools.insert("cancel_task".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("remove".to_string(), AutoApprovePolicy::Prompt);

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
    original_config: AutoApproveConfig,
    pub config_path: PathBuf,
    input_tx: Option<mpsc::Sender<InputEvent>>,
}

impl AutoApproveManager {
    pub fn new(
        auto_approve_tools: Option<&Vec<String>>,
        input_tx: Option<mpsc::Sender<InputEvent>>,
    ) -> Self {
        match Self::try_new(auto_approve_tools, input_tx.clone()) {
            Ok(manager) => manager,
            Err(e) => {
                let config_path = PathBuf::from(AUTO_APPROVE_CONFIG_PATH);
                let config = Self::merge_profile_and_session_config(auto_approve_tools, None);
                let error_msg = format!("Failed to load auto-approve config: {}", e);

                // Send error via InputEvent if sender is available
                if let Some(ref sender) = input_tx {
                    let _ = sender.try_send(InputEvent::Error(error_msg));
                }

                // Try to save the default config even if loading failed
                if let Err(e) = config.save(&config_path, input_tx.clone()) {
                    let warning_msg = format!("Warning: Failed to save auto-approve config: {}", e);
                    if let Some(ref sender) = input_tx {
                        let _ = sender.try_send(InputEvent::Error(warning_msg));
                    }
                }

                AutoApproveManager {
                    original_config: config.clone(),
                    config: config.clone(),
                    config_path,
                    input_tx: input_tx.clone(),
                }
            }
        }
    }

    pub fn try_new(
        auto_approve_tools: Option<&Vec<String>>,
        input_tx: Option<mpsc::Sender<InputEvent>>,
    ) -> Result<Self, String> {
        let config_path = Self::get_config_path()?;
        let session_config = if config_path.exists() {
            // Load existing session config
            Some(Self::load_config(&config_path, input_tx.clone())?)
        } else {
            None
        };

        // Create merged config: profile defaults + session overrides
        let config =
            Self::merge_profile_and_session_config(auto_approve_tools, session_config.as_ref());

        Ok(AutoApproveManager {
            original_config: config.clone(),
            config: config.clone(),
            config_path,
            input_tx,
        })
    }

    fn get_config_path() -> Result<PathBuf, String> {
        // Always use local config in current working directory
        let local_config = Path::new(AUTO_APPROVE_CONFIG_PATH);
        Ok(local_config.to_path_buf())
    }

    fn load_config(
        config_path: &Path,
        error_sender: Option<mpsc::Sender<InputEvent>>,
    ) -> Result<AutoApproveConfig, String> {
        if !config_path.exists() {
            // Create default config
            let config = AutoApproveConfig::default();
            config
                .save(config_path, error_sender.clone())
                .map_err(|e| {
                    let error_msg = format!("Failed to load auto-approve config: {}", e);
                    if let Some(ref sender) = error_sender {
                        let _ = sender.try_send(InputEvent::Error(error_msg.clone()));
                    }
                    error_msg
                })?;
            return Ok(config);
        }

        let content = fs::read_to_string(config_path).map_err(|e| {
            let error_msg = format!("Failed to read config file: {}", e);
            if let Some(ref sender) = error_sender {
                let _ = sender.try_send(InputEvent::Error(error_msg.clone()));
            }
            error_msg
        })?;

        let mut config: AutoApproveConfig = serde_json::from_str(&content).map_err(|e| {
            let error_msg = format!("Failed to parse config file: {}", e);
            if let Some(ref sender) = error_sender {
                let _ = sender.try_send(InputEvent::Error(error_msg.clone()));
            }
            error_msg
        })?;

        config
            .tools
            .insert("run_command".to_string(), AutoApprovePolicy::Prompt);

        // Save the updated config back to file
        config
            .save(config_path, error_sender.clone())
            .map_err(|e| {
                let error_msg = format!("Failed to load auto-approve config: {}", e);
                if let Some(ref sender) = error_sender {
                    let _ = sender.try_send(InputEvent::Error(error_msg.clone()));
                }
                error_msg
            })?;

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
        let binding = tool_call.function.name.clone();
        let tool_name = strip_tool_name(&binding);

        // For shell commands, resolve hierarchical scope keys
        if SHELL_TOOLS.contains(&tool_name)
            && let Some(action) =
                resolve_shell_scope(tool_call, &self.config.tools, &self.config.default_policy)
        {
            return action;
        }

        // Check if there's a specific policy for this tool
        if let Some(policy) = self.config.tools.get(tool_name) {
            return policy.clone();
        }

        // Return default policy
        self.config.default_policy.clone()
    }

    pub fn get_policy_for_tool_name(&self, tool_name: &str) -> AutoApprovePolicy {
        // Check if there's a specific policy for this tool
        if let Some(policy) = self.config.tools.get(strip_tool_name(tool_name)) {
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
        Ok(())
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
        Ok(())
    }

    pub fn set_default_policy(&mut self, policy: AutoApprovePolicy) -> Result<(), String> {
        self.config.default_policy = policy;
        Ok(())
    }

    pub fn toggle_enabled(&mut self) -> Result<(), String> {
        self.config.enabled = !self.config.enabled;
        Ok(())
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn get_config(&self) -> &AutoApproveConfig {
        &self.config
    }

    /// Returns the config as loaded from disk at session start.
    pub fn get_original_config(&self) -> &AutoApproveConfig {
        &self.original_config
    }

    /// Returns true if the current in-memory config differs from what was loaded at session start.
    pub fn has_unsaved_changes(&self) -> bool {
        self.config != self.original_config
    }

    /// Extract the list of tool names that are set to Auto policy.
    pub fn get_auto_approved_tool_names(&self) -> Vec<String> {
        self.config
            .tools
            .iter()
            .filter(|(_, policy)| **policy == AutoApprovePolicy::Auto)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Persist the current config to the project-local `.stakpak/session/auto_approve.json`.
    pub fn save_to_project(&self) -> Result<(), String> {
        self.config
            .save(&self.config_path, self.input_tx.clone())
            .map_err(|e| {
                let error_msg = format!("Failed to save auto-approve config: {}", e);
                if let Some(ref sender) = self.input_tx {
                    let _ = sender.try_send(InputEvent::Error(error_msg.clone()));
                }
                error_msg
            })
    }

    /// Persist auto-approved tools to the profile-level config
    pub fn save_to_profile(&self, output_tx: &tokio::sync::mpsc::Sender<crate::app::OutputEvent>) {
        let tools = self.get_auto_approved_tool_names();
        let _ = output_tx.try_send(crate::app::OutputEvent::SaveAutoApproveToProfile(tools));
    }

    /// Snapshot the current config as the new baseline (after persisting).
    pub fn snapshot(&mut self) {
        self.original_config = self.config.clone();
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

    /// Merge profile auto-approve settings with existing session config.
    /// Session settings take precedence over profile defaults.
    fn merge_profile_and_session_config(
        auto_approve_tools: Option<&Vec<String>>,
        session_config: Option<&AutoApproveConfig>,
    ) -> AutoApproveConfig {
        // Start with default config
        let mut config = AutoApproveConfig::default();

        // Normalize profile auto-approve tools (mapping legacy names)
        let normalized_profile_tools: Option<Vec<String>> = auto_approve_tools.map(|pt| {
            pt.iter()
                .map(|s| backward_compatibility_mapping(s).to_string())
                .collect()
        });

        // Apply profile auto-approve tools (these override default config)
        if let Some(profile_tools) = &normalized_profile_tools {
            for name in profile_tools {
                config.tools.insert(name.clone(), AutoApprovePolicy::Auto);
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
                let mapped_name = backward_compatibility_mapping(tool_name);

                // Only override if this tool is NOT in the profile auto_approve list
                // This ensures profile settings take precedence over session for profile-specified tools
                if let Some(profile_tools) = &normalized_profile_tools {
                    if !profile_tools.iter().any(|s| s == mapped_name) {
                        config.tools.insert(mapped_name.to_string(), policy.clone());
                    }
                } else {
                    config.tools.insert(mapped_name.to_string(), policy.clone());
                }
            }
        }

        config
    }
}

/// Resolve hierarchical shell scope for a tool call.
///
/// Parses the shell command string from the tool call arguments, then resolves
/// each parsed command against scope keys in the rules map.
/// Returns the most restrictive policy across all commands, or `None` if the
/// command string cannot be extracted or parsed.
fn resolve_shell_scope(
    tool_call: &ToolCall,
    rules: &HashMap<String, AutoApprovePolicy>,
    default: &AutoApprovePolicy,
) -> Option<AutoApprovePolicy> {
    let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments).ok()?;
    let command_str = args.get("command")?.as_str()?;
    let tool_name = strip_tool_name(&tool_call.function.name);
    let fallback_scopes = if SHELL_TOOLS.contains(&tool_name) && tool_name != BASE_SHELL_TOOL {
        vec![BASE_SHELL_TOOL]
    } else {
        Vec::new()
    };
    let mut effective_rules = rules.clone();

    if !fallback_scopes.is_empty() && effective_rules.get(tool_name) == Some(default) {
        effective_rules.remove(tool_name);
    }

    match stakpak_shell_tool_approvals::resolve_hierarchical_policy(
        command_str,
        tool_name,
        &fallback_scopes,
        &effective_rules,
        default.clone(),
    ) {
        Ok(action) => action,
        Err(_) => Some(
            conservative_shell_parse_fallback(tool_name, rules, default)
                .max(AutoApprovePolicy::Prompt),
        ),
    }
}

fn conservative_shell_parse_fallback(
    tool_scope: &str,
    rules: &HashMap<String, AutoApprovePolicy>,
    default: &AutoApprovePolicy,
) -> AutoApprovePolicy {
    rules
        .get(tool_scope)
        .cloned()
        .or_else(|| {
            if SHELL_TOOLS.contains(&tool_scope) && tool_scope != BASE_SHELL_TOOL {
                rules.get(BASE_SHELL_TOOL).cloned()
            } else {
                None
            }
        })
        .unwrap_or_else(|| default.clone())
}

impl AutoApproveConfig {
    fn save(
        &self,
        path: &Path,
        _error_sender: Option<mpsc::Sender<InputEvent>>,
    ) -> Result<(), String> {
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
        assert!(!config.enabled); // Session override
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

    #[tokio::test]
    async fn test_error_handling_with_invalid_config_file() {
        use std::fs;
        use tempfile::TempDir;

        // Create a temporary directory for the test
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let config_dir = temp_dir.path().join(".stakpak/session");
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
        let config_path = config_dir.join("auto_approve.json");

        // Create an invalid JSON file that will cause a parse error
        fs::write(&config_path, "invalid json content {").expect("Failed to write invalid config");

        // Temporarily change directory to the temp directory so the config path resolution works
        let original_dir = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(temp_dir.path());

        // Create a channel to receive error events
        let (error_tx, mut error_rx) = tokio::sync::mpsc::channel::<InputEvent>(10);

        // Try to create AutoApproveManager with invalid config - should send error via channel
        let _manager = AutoApproveManager::new(None, Some(error_tx.clone()));

        // Check that we received an error event (try_send is synchronous)
        let error_received = error_rx.try_recv();
        assert!(error_received.is_ok(), "Expected error event to be sent");

        if let Ok(InputEvent::Error(error_msg)) = error_received {
            assert!(
                error_msg.contains("Failed to load auto-approve config")
                    || error_msg.contains("Failed to parse config file"),
                "Error message should indicate config loading/parsing failure. Got: {}",
                error_msg
            );
        } else {
            panic!("Expected InputEvent::Error, got: {:?}", error_received);
        }

        // Restore original directory if it existed
        if let Some(original) = original_dir {
            let _ = std::env::set_current_dir(&original);
        }
    }

    // --- Tests for hierarchical shell scope resolution ---

    use stakpak_shared::models::integrations::openai::{FunctionCall, ToolCall};

    fn make_tool_call(tool_name: &str, command: &str) -> ToolCall {
        ToolCall {
            id: "tc-1".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: tool_name.to_string(),
                arguments: serde_json::json!({"command": command}).to_string(),
            },
            metadata: None,
        }
    }

    fn make_run_command_tool_call(command: &str) -> ToolCall {
        make_tool_call("run_command", command)
    }

    #[test]
    fn default_config_includes_stakpak_allowlist_entries() {
        let config = AutoApproveConfig::default();

        for key in [
            "run_command::stakpak::version",
            "run_command::stakpak::account",
            "run_command::stakpak::completion",
            "run_command::stakpak::ak",
            "run_command::stakpak::sessions",
            "run_command::stakpak::config::show",
            "run_command::stakpak::config::sample",
        ] {
            assert_eq!(
                config.tools.get(key),
                Some(&AutoApprovePolicy::Auto),
                "missing or incorrect policy for {key}"
            );
        }
    }

    #[test]
    fn default_config_does_not_include_stakpak_scope_level_entry() {
        let config = AutoApproveConfig::default();
        assert_eq!(config.tools.get("run_command::stakpak"), None);
    }

    #[test]
    fn resolve_shell_scope_command_level_auto() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), AutoApprovePolicy::Auto);
        let tc = make_run_command_tool_call("git status");
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Prompt);
        assert_eq!(result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn resolve_shell_scope_most_restrictive_wins_in_pipeline() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), AutoApprovePolicy::Auto);
        rules.insert(
            "run_command::git::push".to_string(),
            AutoApprovePolicy::Never,
        );
        let tc = make_run_command_tool_call("git log && git push origin main");
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Prompt);
        assert_eq!(result, Some(AutoApprovePolicy::Never));
    }

    #[test]
    fn resolve_shell_scope_unknown_command_falls_back_to_default() {
        let rules = HashMap::new();
        let tc = make_run_command_tool_call("rm -rf /tmp/test");
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Prompt);
        // rules is empty, so base = run_command rule (absent) → default (Prompt)
        assert_eq!(result, Some(AutoApprovePolicy::Prompt));
    }

    #[test]
    fn resolve_shell_scope_returns_none_for_empty_parse() {
        // An empty command string parses to zero commands → None
        let rules = HashMap::new();
        let tc = ToolCall {
            id: "tc-2".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": ""}).to_string(),
            },
            metadata: None,
        };
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Prompt);
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_shell_scope_glob_pattern_in_arg_rule() {
        let mut rules = HashMap::new();
        rules.insert("run_command::curl".to_string(), AutoApprovePolicy::Auto);
        rules.insert(
            "run_command::curl::*.prod.*".to_string(),
            AutoApprovePolicy::Never,
        );
        let staging_tc = make_run_command_tool_call("curl https://api.staging.example.com");
        assert_eq!(
            resolve_shell_scope(&staging_tc, &rules, &AutoApprovePolicy::Prompt),
            Some(AutoApprovePolicy::Auto)
        );
        let prod_tc = make_run_command_tool_call("curl https://api.prod.example.com");
        assert_eq!(
            resolve_shell_scope(&prod_tc, &rules, &AutoApprovePolicy::Prompt),
            Some(AutoApprovePolicy::Never)
        );
    }

    #[test]
    fn resolve_shell_scope_nested_sh_c() {
        let mut rules = HashMap::new();
        rules.insert("run_command::rm".to_string(), AutoApprovePolicy::Never);
        let tc = make_run_command_tool_call("sh -c 'rm -rf /tmp/old'");
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Auto);
        assert_eq!(result, Some(AutoApprovePolicy::Never));
    }

    #[test]
    fn resolve_shell_scope_argument_rule_can_relax_default_when_more_specific() {
        let mut rules = HashMap::new();
        rules.insert(
            "run_command::git::status".to_string(),
            AutoApprovePolicy::Auto,
        );
        let tc = make_run_command_tool_call("git status");
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Prompt);
        assert_eq!(result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn resolve_shell_scope_stakpak_version_auto_approved() {
        let config = AutoApproveConfig::default();
        let tc = make_run_command_tool_call("stakpak version");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn resolve_shell_scope_stakpak_ak_namespace_matches_trailing_args() {
        let config = AutoApproveConfig::default();
        let tc = make_run_command_tool_call("stakpak ak write notes.md");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn resolve_shell_scope_stakpak_config_show_two_segment_match() {
        let config = AutoApproveConfig::default();
        let tc = make_run_command_tool_call("stakpak config show");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn resolve_shell_scope_stakpak_config_list_still_prompts() {
        let config = AutoApproveConfig::default();
        let tc = make_run_command_tool_call("stakpak config list");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Prompt));
    }

    #[test]
    fn resolve_shell_scope_stakpak_update_still_prompts() {
        let config = AutoApproveConfig::default();
        let tc = make_run_command_tool_call("stakpak update");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Prompt));
    }

    #[test]
    fn resolve_shell_scope_stakpak_bare_still_prompts() {
        let config = AutoApproveConfig::default();
        let tc = make_run_command_tool_call("stakpak");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Prompt));
    }

    #[test]
    fn resolve_shell_scope_stakpak_browser_does_not_bypass_ak_allowlist() {
        let config = AutoApproveConfig::default();
        let tc = make_run_command_tool_call("stakpak browser ak visit example.com");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Prompt));
    }

    #[test]
    fn resolve_shell_scope_run_command_task_rule_overrides_shared_scope() {
        let mut rules = HashMap::new();
        rules.insert("run_command_task".to_string(), AutoApprovePolicy::Never);
        rules.insert("run_command::git".to_string(), AutoApprovePolicy::Auto);
        let tc = make_tool_call("run_command_task", "git status");
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Prompt);
        assert_eq!(result, Some(AutoApprovePolicy::Never));
    }

    #[test]
    fn resolve_shell_scope_run_command_task_can_fallback_to_shared_scope() {
        let mut rules = HashMap::new();
        rules.insert("run_command::git".to_string(), AutoApprovePolicy::Auto);
        let tc = make_tool_call("run_command_task", "git status");
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Prompt);
        assert_eq!(result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn resolve_shell_scope_run_command_task_inherits_stakpak_ak_allowlist() {
        let config = AutoApproveConfig::default();
        let tc = make_tool_call("run_command_task", "stakpak ak tree");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn resolve_shell_scope_user_override_prompt_wins_over_allowlist() {
        let mut config = AutoApproveConfig::default();
        config.tools.insert(
            "run_command::stakpak::ak".to_string(),
            AutoApprovePolicy::Prompt,
        );

        let tc = make_run_command_tool_call("stakpak ak tree");
        let result = resolve_shell_scope(&tc, &config.tools, &config.default_policy);
        assert_eq!(result, Some(AutoApprovePolicy::Prompt));
    }

    #[test]
    fn resolve_shell_scope_user_subrule_tightens_namespace_default() {
        let mut config = AutoApproveConfig::default();
        config.tools.insert(
            "run_command::stakpak::ak::write".to_string(),
            AutoApprovePolicy::Prompt,
        );

        let write_tc = make_run_command_tool_call("stakpak ak write notes.md");
        let write_result = resolve_shell_scope(&write_tc, &config.tools, &config.default_policy);
        assert_eq!(write_result, Some(AutoApprovePolicy::Prompt));

        let tree_tc = make_run_command_tool_call("stakpak ak tree");
        let tree_result = resolve_shell_scope(&tree_tc, &config.tools, &config.default_policy);
        assert_eq!(tree_result, Some(AutoApprovePolicy::Auto));
    }

    #[test]
    fn should_auto_approve_stakpak_ak_tree_with_fresh_defaults() {
        let manager = AutoApproveManager {
            config: AutoApproveConfig::default(),
            config_path: PathBuf::from(AUTO_APPROVE_CONFIG_PATH),
            input_tx: None,
        };
        let tc = make_run_command_tool_call("stakpak ak tree");

        assert!(manager.should_auto_approve(&tc));
    }

    #[test]
    fn should_not_auto_approve_stakpak_update_with_fresh_defaults() {
        let manager = AutoApproveManager {
            config: AutoApproveConfig::default(),
            config_path: PathBuf::from(AUTO_APPROVE_CONFIG_PATH),
            input_tx: None,
        };
        let tc = make_run_command_tool_call("stakpak update");

        assert!(!manager.should_auto_approve(&tc));
    }

    #[test]
    fn should_not_auto_approve_stakpak_browser_ak_visit_with_fresh_defaults() {
        let manager = AutoApproveManager {
            config: AutoApproveConfig::default(),
            config_path: PathBuf::from(AUTO_APPROVE_CONFIG_PATH),
            input_tx: None,
        };
        let tc = make_run_command_tool_call("stakpak browser ak visit example.com");

        assert!(!manager.should_auto_approve(&tc));
    }

    #[test]
    fn resolve_shell_scope_parse_error_fails_closed() {
        let mut rules = HashMap::new();
        rules.insert("run_command_task".to_string(), AutoApprovePolicy::Never);

        let mut command = "echo deeply nested".to_string();
        for _ in 0..=6 {
            command = format!("sh -c '{}'", command.replace('\'', "'\\''"));
        }

        let tc = make_tool_call("run_command_task", &command);
        let result = resolve_shell_scope(&tc, &rules, &AutoApprovePolicy::Auto);
        assert_eq!(result, Some(AutoApprovePolicy::Never));
    }

    #[test]
    fn matches_pattern_exact() {
        assert!(stakpak_shell_tool_approvals::matches_pattern(
            "push", "push"
        ));
        assert!(!stakpak_shell_tool_approvals::matches_pattern(
            "push", "pull"
        ));
    }

    #[test]
    fn matches_pattern_glob() {
        assert!(stakpak_shell_tool_approvals::matches_pattern(
            "*.prod.*",
            "api.prod.example.com"
        ));
        assert!(!stakpak_shell_tool_approvals::matches_pattern(
            "*.prod.*",
            "api.staging.example.com"
        ));
    }

    #[test]
    fn matches_pattern_regex() {
        assert!(stakpak_shell_tool_approvals::matches_pattern(
            "re:^push$",
            "push"
        ));
        assert!(!stakpak_shell_tool_approvals::matches_pattern(
            "re:^push$",
            "push-force"
        ));
    }

    #[test]
    fn matches_pattern_invalid_regex_returns_false() {
        assert!(!stakpak_shell_tool_approvals::matches_pattern(
            "re:[invalid",
            "anything"
        ));
    }

    #[tokio::test]
    async fn test_error_handling_without_error_sender() {
        use std::fs;
        use tempfile::TempDir;

        // Create a temporary directory for the test
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let config_dir = temp_dir.path().join(".stakpak/session");
        fs::create_dir_all(&config_dir).expect("Failed to create config directory");
        let config_path = config_dir.join("auto_approve.json");

        // Create an invalid JSON file that will cause a parse error
        fs::write(&config_path, "invalid json content {").expect("Failed to write invalid config");

        // Temporarily change directory to the temp directory so the config path resolution works
        let original_dir = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(temp_dir.path());

        // Try to create AutoApproveManager without error sender - should not panic
        let manager = AutoApproveManager::new(None, None);

        // Manager should still be created with default config despite the error
        assert!(manager.config.enabled);

        // Restore original directory if it existed
        if let Some(original) = original_dir {
            let _ = std::env::set_current_dir(&original);
        }
    }
}
