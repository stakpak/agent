use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Scratchpad {
    /// Name of the application being containerized
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,

    // App source
    /// URL of the git repository containing the application code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_repository: Option<String>,
    /// Path to the local directory containing the application code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_source_directory_path: Option<String>,
    /// Layout and organization of the application's files and folders
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_structure: Option<String>,
    /// Primary programming language used in the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub programming_language: Option<String>,
    /// Web framework or other major frameworks used by the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub programming_framework: Option<String>,

    // Runtime requirements
    /// Command or script or file used to start the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    /// Configuration variables needed by the application at runtime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_environment_variables: Option<String>,
    /// Sensitive configuration values needed by the application at runtime, and should be securely stored
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_secrets: Option<String>,
    /// Network ports that the application listens on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listening_ports: Option<String>,
    /// Version of the programming language runtime needed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    /// Runtime OS dependencies needed by the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_dependencies: Option<String>,

    /// Current application Dockerfile path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dockerfile_path: Option<String>,
    /// Current application Dockerfile
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dockerfile_content: Option<String>,

    // Misc
    /// Operating system of the user's local machine
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_os: Option<String>,
    /// Any other relevant information about the deployment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_notes: Option<String>,
}
