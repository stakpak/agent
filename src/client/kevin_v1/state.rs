use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Scratchpad {
    /// Name of the infra project being applied
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,

    // Configurations source
    /// Path to the local directory containing configurations source
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_source_directory_path: Option<String>,
    /// Layout and organization of the application's files and folders
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_structure: Option<String>,
    /// Primary configuration language used in the project
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_language: Option<String>,
    /// DevOps tool to be used to apply these configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_tool: Option<String>,

    // Runtime requirements
    /// Configuration variables needed by the application at runtime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_variables: Option<String>,
    /// Sensitive configuration values needed by the application at runtime, and should be securely stored
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<String>,
    /// Runtime OS dependencies needed to apply the configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_dependencies: Option<String>,

    // Misc
    /// Operating system of the user's local machine
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_os: Option<String>,
    /// Any other relevant information about the deployment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_notes: Option<String>,
}
