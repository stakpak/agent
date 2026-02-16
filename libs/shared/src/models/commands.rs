//! Custom slash command types and configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CustomCommand {
    pub id: String,
    pub description: String,
    pub content: String,
    pub source: CommandSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    Predefined,
    PredefinedRemote,
    PersonalFile,
    ProjectFile,
    ConfigDefinition,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CommandsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub definitions: HashMap<String, String>,
}

impl CommandsConfig {
    pub fn file_prefix(&self) -> &str {
        self.file_prefix.as_deref().unwrap_or("cmd_")
    }

    pub fn id_prefix(&self) -> &str {
        self.id_prefix.as_deref().unwrap_or("/cmd:")
    }

    pub fn should_load(&self, command_name: &str) -> bool {
        self.matches_include(command_name) && self.matches_exclude(command_name)
    }

    fn matches_include(&self, name: &str) -> bool {
        match &self.include {
            Some(patterns) if !patterns.is_empty() => {
                patterns.iter().any(|p| crate::utils::matches_glob(name, p))
            }
            _ => true,
        }
    }

    fn matches_exclude(&self, name: &str) -> bool {
        match &self.exclude {
            Some(patterns) if !patterns.is_empty() => {
                !patterns.iter().any(|p| crate::utils::matches_glob(name, p))
            }
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_getters() {
        let default = CommandsConfig::default();
        assert_eq!(default.file_prefix(), "cmd_");
        assert_eq!(default.id_prefix(), "/cmd:");

        let custom = CommandsConfig {
            file_prefix: Some("prompt_".to_string()),
            id_prefix: Some("/prompt:".to_string()),
            ..Default::default()
        };
        assert_eq!(custom.file_prefix(), "prompt_");
        assert_eq!(custom.id_prefix(), "/prompt:");

        let empty_prefix = CommandsConfig {
            file_prefix: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(empty_prefix.file_prefix(), "");
    }

    #[test]
    fn test_should_load_include_only() {
        let config = CommandsConfig {
            include: Some(vec!["security-*".to_string(), "code-*".to_string()]),
            exclude: None,
            definitions: HashMap::new(),
            ..Default::default()
        };
        assert!(config.should_load("security-review"));
        assert!(config.should_load("code-review"));
        assert!(!config.should_load("write-tests"));
    }

    #[test]
    fn test_should_load_exclude_only() {
        let config = CommandsConfig {
            include: None,
            exclude: Some(vec!["*-deprecated".to_string()]),
            definitions: HashMap::new(),
            ..Default::default()
        };
        assert!(config.should_load("security-review"));
        assert!(!config.should_load("old-deprecated"));
    }

    #[test]
    fn test_should_load_exclude_takes_precedence() {
        let config = CommandsConfig {
            include: Some(vec!["security-*".to_string()]),
            exclude: Some(vec!["security-old".to_string()]),
            definitions: HashMap::new(),
            ..Default::default()
        };
        assert!(config.should_load("security-review"));
        assert!(!config.should_load("security-old"));
    }
}
