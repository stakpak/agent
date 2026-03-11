use crate::app::InputEvent;
use crate::constants::AUTO_APPROVE_CONFIG_PATH;
use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::openai::ToolCall;
use stakpak_shared::utils::{backward_compatibility_mapping, strip_tool_name};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

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
        tools.insert("load_skill".to_string(), AutoApprovePolicy::Auto);
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
                    config,
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
            config,
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
        if (tool_name == "run_command" || tool_name == "run_command_task")
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
    let fallback_scopes = if tool_name == "run_command_task" {
        vec!["run_command"]
    } else {
        Vec::new()
    };

    match shell_parser::resolve_hierarchical_policy(
        command_str,
        tool_name,
        &fallback_scopes,
        rules,
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
            if tool_scope == "run_command_task" {
                rules.get("run_command").cloned()
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
        assert!(shell_parser::matches_pattern("push", "push"));
        assert!(!shell_parser::matches_pattern("push", "pull"));
    }

    #[test]
    fn matches_pattern_glob() {
        assert!(shell_parser::matches_pattern(
            "*.prod.*",
            "api.prod.example.com"
        ));
        assert!(!shell_parser::matches_pattern(
            "*.prod.*",
            "api.staging.example.com"
        ));
    }

    #[test]
    fn matches_pattern_regex() {
        assert!(shell_parser::matches_pattern("re:^push$", "push"));
        assert!(!shell_parser::matches_pattern("re:^push$", "push-force"));
    }

    #[test]
    fn matches_pattern_invalid_regex_returns_false() {
        assert!(!shell_parser::matches_pattern("re:[invalid", "anything"));
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
