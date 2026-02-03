use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WardenConfig {
    pub enabled: bool,
    #[serde(default)]
    pub volumes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubagentConfig {
    pub description: String,
    pub max_steps: usize,
    pub allowed_tools: Vec<String>,
    /// Model to use for the subagent (e.g., "eco", "smart")
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub warden: Option<WardenConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubagentConfigs {
    pub subagents: HashMap<String, SubagentConfig>,
}

impl SubagentConfigs {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Self::load_from_str(&content)
    }

    pub fn load_from_str(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config: SubagentConfigs = toml::from_str(content)?;
        Ok(config)
    }

    pub fn get_available_types(&self) -> Vec<String> {
        self.subagents.keys().cloned().collect()
    }

    pub fn get_config(&self, subagent_type: &str) -> Option<&SubagentConfig> {
        self.subagents.get(subagent_type)
    }

    pub fn format_for_context(&self) -> String {
        if self.subagents.is_empty() {
            "# No Subagents Available".to_string()
        } else {
            let subagents_text = self
                .subagents
                .iter()
                .map(|(name, config)| {
                    format!(
                        "  - Name: {}\n    Description: {}\n    Tools: {}",
                        name,
                        config.description,
                        config.allowed_tools.join(", ")
                    )
                })
                .collect::<Vec<String>>()
                .join("\n");

            format!("# Available Subagents:\n\n{}", subagents_text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_subagent_config_with_model() {
        let content = r#"
[subagents.DiscoveryAgent]
description = "Test agent"
model = "eco"
max_steps = 30
allowed_tools = ["stakpak__run_command", "stakpak__view"]

[subagents.DiscoveryAgent.warden]
enabled = true
volumes = ["./:/agent:ro"]
"#;
        let configs =
            SubagentConfigs::load_from_str(content).expect("Failed to parse subagent config");

        let agent = configs
            .get_config("DiscoveryAgent")
            .expect("DiscoveryAgent not found");

        assert_eq!(agent.model, Some("eco".to_string()));
        assert_eq!(agent.max_steps, 30);
        assert_eq!(
            agent.allowed_tools,
            vec!["stakpak__run_command", "stakpak__view"]
        );
        assert!(agent.warden.is_some());
    }

    #[test]
    fn test_parse_subagent_config_without_model() {
        let content = r#"
[subagents.BasicAgent]
description = "Agent without model"
max_steps = 10
allowed_tools = ["stakpak__view"]
"#;
        let configs =
            SubagentConfigs::load_from_str(content).expect("Failed to parse subagent config");

        let agent = configs
            .get_config("BasicAgent")
            .expect("BasicAgent not found");

        assert_eq!(agent.model, None);
        assert_eq!(agent.max_steps, 10);
    }

    #[test]
    fn test_parse_default_subagents_toml() {
        // Test parsing the actual default config
        let content = include_str!("../../../../cli/subagents.toml");
        let configs = SubagentConfigs::load_from_str(content)
            .expect("Failed to parse default subagents.toml");

        let discovery = configs
            .get_config("DiscoveryAgent")
            .expect("DiscoveryAgent not found in default config");

        assert_eq!(discovery.model, Some("eco".to_string()));
        assert!(
            discovery
                .allowed_tools
                .contains(&"stakpak__run_command".to_string())
        );
        assert!(
            discovery
                .allowed_tools
                .contains(&"stakpak__view".to_string())
        );
    }
}
