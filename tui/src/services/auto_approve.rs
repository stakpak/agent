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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPatterns {
    pub safe_readonly: Vec<String>,
    pub sensitive_destructive: Vec<String>,
    pub interactive_required: Vec<String>,
}

impl Default for CommandPatterns {
    fn default() -> Self {
        CommandPatterns {
            safe_readonly: vec![
                // "ls".to_string(),       // ✅ Only lists, never writes
                // "pwd".to_string(),      // ✅ Only prints current directory
                // "whoami".to_string(),   // ✅ Only prints username
                // "date".to_string(),     // ✅ Only displays date/time
                // "uptime".to_string(),   // ✅ Only shows system uptime
                // "id".to_string(),       // ✅ Only shows user/group IDs
                // "groups".to_string(),   // ✅ Only shows group membership
                // "which".to_string(),    // ✅ Only shows command locations
                // "whereis".to_string(),  // ✅ Only locates files
                // "file".to_string(),     // ✅ Only identifies file types
                // "stat".to_string(),     // ✅ Only displays file stats
                // "du".to_string(),       // ✅ Only shows disk usage
                // "df".to_string(),       // ✅ Only shows filesystem usage
                // "ps".to_string(),       // ✅ Only lists processes
                // "env".to_string(),      // ✅ Only shows environment variables
                // "printenv".to_string(), // ✅ Only pri
            ],
            sensitive_destructive: vec![],
            interactive_required: vec![],
        }
    }
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
            Err(_) => {
                // Fallback to default config if loading fails
                let config_path = PathBuf::from(AUTO_APPROVE_CONFIG_PATH);
                AutoApproveManager {
                    config: AutoApproveConfig::default(),
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
        // First try local config in current working directory
        let local_config = Path::new(AUTO_APPROVE_CONFIG_PATH);
        if local_config.exists() {
            return Ok(local_config.to_path_buf());
        }

        // Fallback to global config
        let home_dir =
            std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
        let global_config = Path::new(&home_dir).join(AUTO_APPROVE_CONFIG_PATH);

        Ok(global_config)
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

        serde_json::from_str(&content).map_err(|e| format!("Failed to parse config file: {}", e))
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
        let risk_level = self.get_risk_level(tool_call);
        matches!(risk_level, RiskLevel::Low)
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
