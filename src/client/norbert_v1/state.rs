use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Scratchpad {
    /// Name of the application being deployed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,

    // Infrastructure
    /// The cloud provider where the application will be deployed, must be one of ("AWS", "Google Cloud", "Digital Ocean", "Azure")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_provider: Option<String>,
    /// The specific compute service to use (e.g. EC2, Cloud Run, App Service)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_compute_service_name: Option<String>,
    /// The geographic region where the application will be deployed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_region: Option<String>,
    /// CPU resources allocated to the compute instance (e.g. 1 vCPU, 2 cores)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_compute_cpu: Option<String>,
    /// Memory resources allocated to the compute instance (e.g. 512MB, 2GB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_compute_memory: Option<String>,
    /// Instance type/size of the compute resource (e.g. t2.micro for AWS EC2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_compute_instance_type: Option<String>,

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
    /// Operating system of the deployment server
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_os: Option<String>,
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

    // Database configuration    #[serde(skip_serializing_if = "Option::is_none")]
    /// Type of database system to use, must be one of ("PostgreSQL", "MySQL", "MongoDB")
    pub database_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_version: Option<String>,

    // Connectivity
    /// Custom domain name to be used for the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_name: Option<String>,
    /// DNS provider used for managing domain records (e.g. Cloudflare, Route53, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_provider: Option<String>,
    /// Address of the server where the application is deployed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_server_address: Option<String>,

    // Security
    /// Whether HTTPS/TLS encryption is required for the application
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_required: Option<String>,

    // Misc
    /// Operating system of the user's local machine
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_os: Option<String>,
    /// Any other relevant information about the deployment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_notes: Option<String>,
}
