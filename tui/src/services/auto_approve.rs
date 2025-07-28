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
    Smart,
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

        // Prompt tools (always require confirmation):
        tools.insert("create".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("str_replace".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("generate_code".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("run_command".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("run_command_async".to_string(), AutoApprovePolicy::Prompt);
        tools.insert("cancel_async_task".to_string(), AutoApprovePolicy::Prompt);

        AutoApproveConfig {
            enabled: true,
            default_policy: AutoApprovePolicy::Prompt,
            tools,
            command_patterns: CommandPatterns::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

pub struct AutoApproveManager {
    pub config: AutoApproveConfig,
    pub config_path: PathBuf,
}

impl AutoApproveManager {
    pub fn new() -> Self {
        match Self::try_new() {
            Ok(manager) => manager,
            Err(e) => {
                // Fallback to default config if loading fails
                let config_path = PathBuf::from(AUTO_APPROVE_CONFIG_PATH);
                let config = AutoApproveConfig::default();
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
        Self::new()
    }
}

impl AutoApproveManager {
    pub fn try_new() -> Result<Self, String> {
        let config_path = Self::get_config_path()?;
        let config = Self::load_config(&config_path)?;

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

        // Auto-update run_command policy based on command patterns
        if !config.command_patterns.safe_readonly.is_empty()
            || !config.command_patterns.sensitive_destructive.is_empty()
            || !config.command_patterns.interactive_required.is_empty()
        {
            config
                .tools
                .insert("run_command".to_string(), AutoApprovePolicy::Smart);
        } else {
            config
                .tools
                .insert("run_command".to_string(), AutoApprovePolicy::Prompt);
        }

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
            AutoApprovePolicy::Smart => self.is_safe_command(tool_call),
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

    pub fn get_risk_level(&self, tool_call: &ToolCall) -> RiskLevel {
        let command = self.extract_command(tool_call);

        // Check for critical risk patterns
        if self.is_critical_risk(&command) {
            return RiskLevel::Critical;
        }

        // Check for high risk patterns
        if self.is_high_risk(&command) {
            return RiskLevel::High;
        }

        // Check for medium risk patterns
        if self.is_medium_risk(&command) {
            return RiskLevel::Medium;
        }

        // Default to low risk
        RiskLevel::Low
    }

    fn is_safe_command(&self, tool_call: &ToolCall) -> bool {
        let command = self.extract_command(tool_call);
        let command_lower = command.to_lowercase();

        // Check if command matches any safe_readonly patterns
        for pattern in &self.config.command_patterns.safe_readonly {
            if command_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }

        // Check if command matches any sensitive_destructive patterns
        for pattern in &self.config.command_patterns.sensitive_destructive {
            if command_lower.contains(&pattern.to_lowercase()) {
                return false;
            }
        }

        // Check if command matches any interactive_required patterns
        for pattern in &self.config.command_patterns.interactive_required {
            if command_lower.contains(&pattern.to_lowercase()) {
                return false;
            }
        }

        // If no patterns are configured, fall back to risk level assessment
        if self.config.command_patterns.safe_readonly.is_empty()
            && self
                .config
                .command_patterns
                .sensitive_destructive
                .is_empty()
            && self.config.command_patterns.interactive_required.is_empty()
        {
            let risk_level = self.get_risk_level(tool_call);
            return matches!(risk_level, RiskLevel::Low);
        }

        // If patterns are configured but command doesn't match any safe patterns, it's not safe
        false
    }

    fn is_critical_risk(&self, command: &str) -> bool {
        let command_lower = command.to_lowercase();

        // Check for destructive operations
        self.config
            .command_patterns
            .sensitive_destructive
            .iter()
            .any(|pattern| command_lower.contains(pattern))
    }

    fn is_high_risk(&self, command: &str) -> bool {
        let command_lower = command.to_lowercase();

        // Check for system modifications
        command_lower.contains("/etc/")
            || command_lower.contains("/bin/")
            || command_lower.contains("/usr/")
            || command_lower.contains("systemctl")
            || command_lower.contains("service")
    }

    fn is_medium_risk(&self, command: &str) -> bool {
        let command_lower = command.to_lowercase();

        // Check for file operations outside current directory
        command_lower.contains("../")
            || command_lower.contains("/home/")
            || command_lower.contains("/tmp/")
    }

    fn extract_command(&self, tool_call: &ToolCall) -> String {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
            if let Some(cmd) = json.get("command").and_then(|v| v.as_str()) {
                return cmd.to_string();
            }
        }

        // Fallback to old parsing method
        tool_call
            .function
            .arguments
            .split("\"command\": \"")
            .nth(1)
            .and_then(|s| s.split('\"').next())
            .unwrap_or("")
            .to_string()
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

        // If any command patterns are configured, change run_command from Prompt to Smart
        if !self.config.command_patterns.safe_readonly.is_empty()
            || !self
                .config
                .command_patterns
                .sensitive_destructive
                .is_empty()
            || !self.config.command_patterns.interactive_required.is_empty()
        {
            self.config
                .tools
                .insert("run_command".to_string(), AutoApprovePolicy::Smart);
        } else {
            // If no patterns are configured, revert run_command back to Prompt
            self.config
                .tools
                .insert("run_command".to_string(), AutoApprovePolicy::Prompt);
        }

        self.save_config()
    }

    pub fn add_command_pattern(
        &mut self,
        pattern_type: &str,
        pattern: String,
    ) -> Result<(), String> {
        let patterns = match pattern_type {
            "safe_readonly" => &mut self.config.command_patterns.safe_readonly,
            "sensitive_destructive" => &mut self.config.command_patterns.sensitive_destructive,
            "interactive_required" => &mut self.config.command_patterns.interactive_required,
            _ => return Err(format!("Unknown pattern type: {}", pattern_type)),
        };

        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }

        // If any command patterns are configured, change run_command from Prompt to Smart
        if !self.config.command_patterns.safe_readonly.is_empty()
            || !self
                .config
                .command_patterns
                .sensitive_destructive
                .is_empty()
            || !self.config.command_patterns.interactive_required.is_empty()
        {
            self.config
                .tools
                .insert("run_command".to_string(), AutoApprovePolicy::Smart);
        }

        self.save_config()
    }

    pub fn remove_command_pattern(
        &mut self,
        pattern_type: &str,
        pattern: &str,
    ) -> Result<(), String> {
        let patterns = match pattern_type {
            "safe_readonly" => &mut self.config.command_patterns.safe_readonly,
            "sensitive_destructive" => &mut self.config.command_patterns.sensitive_destructive,
            "interactive_required" => &mut self.config.command_patterns.interactive_required,
            _ => return Err(format!("Unknown pattern type: {}", pattern_type)),
        };

        patterns.retain(|p| p != pattern);

        // If no patterns are configured, revert run_command back to Prompt
        if self.config.command_patterns.safe_readonly.is_empty()
            && self
                .config
                .command_patterns
                .sensitive_destructive
                .is_empty()
            && self.config.command_patterns.interactive_required.is_empty()
        {
            self.config
                .tools
                .insert("run_command".to_string(), AutoApprovePolicy::Prompt);
        }

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
