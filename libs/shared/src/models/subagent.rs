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
