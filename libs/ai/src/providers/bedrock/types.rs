//! Bedrock-specific types and configuration
//!
//! Authentication is handled entirely by the AWS credential chain:
//! - Environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`)
//! - Shared credentials file (`~/.aws/credentials`)
//! - AWS SSO / IAM Identity Center
//! - ECS container credentials
//! - EC2 instance metadata (IMDS)
//!
//! No API key or secret key fields exist on this config — that's by design.

/// Configuration for the AWS Bedrock provider
///
/// # Example
///
/// ```rust,no_run
/// use stakai::providers::bedrock::BedrockConfig;
///
/// // Minimal: just specify the region
/// let config = BedrockConfig::new("us-east-1");
///
/// // With a named AWS profile
/// let config = BedrockConfig::new("us-west-2")
///     .with_profile_name("production");
///
/// // With a VPC endpoint for private connectivity
/// let config = BedrockConfig::new("us-east-1")
///     .with_endpoint_override("https://vpce-xxx.bedrock-runtime.us-east-1.vpce.amazonaws.com");
///
/// // From environment (reads AWS_REGION or AWS_DEFAULT_REGION)
/// let config = BedrockConfig::from_env();
/// ```
#[derive(Debug, Clone)]
pub struct BedrockConfig {
    /// AWS region (e.g., "us-east-1")
    pub region: String,
    /// Optional AWS named profile (from `~/.aws/config`)
    pub profile_name: Option<String>,
    /// Optional custom endpoint URL (for VPC endpoints or testing)
    pub endpoint_override: Option<String>,
}

impl BedrockConfig {
    /// Create a new Bedrock config with the given AWS region
    pub fn new(region: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            profile_name: None,
            endpoint_override: None,
        }
    }

    /// Create config from environment variables
    ///
    /// Reads `AWS_REGION` or `AWS_DEFAULT_REGION`. Falls back to `us-east-1`
    /// if neither is set.
    pub fn from_env() -> Self {
        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        let profile_name = std::env::var("AWS_PROFILE").ok();

        Self {
            region,
            profile_name,
            endpoint_override: None,
        }
    }

    /// Set the AWS named profile (from `~/.aws/config`)
    pub fn with_profile_name(mut self, profile_name: impl Into<String>) -> Self {
        self.profile_name = Some(profile_name.into());
        self
    }

    /// Set a custom endpoint URL (for VPC endpoints or local testing)
    pub fn with_endpoint_override(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint_override = Some(endpoint.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_sets_region_and_defaults() {
        let config = BedrockConfig::new("us-west-2");
        assert_eq!(config.region, "us-west-2");
        assert!(config.profile_name.is_none());
        assert!(config.endpoint_override.is_none());
    }

    #[test]
    fn test_with_profile_name() {
        let config = BedrockConfig::new("eu-west-1").with_profile_name("staging");
        assert_eq!(config.region, "eu-west-1");
        assert_eq!(config.profile_name.as_deref(), Some("staging"));
    }

    #[test]
    fn test_with_endpoint_override() {
        let config = BedrockConfig::new("us-east-1").with_endpoint_override(
            "https://vpce-xxx.bedrock-runtime.us-east-1.vpce.amazonaws.com",
        );
        assert_eq!(
            config.endpoint_override.as_deref(),
            Some("https://vpce-xxx.bedrock-runtime.us-east-1.vpce.amazonaws.com")
        );
    }

    #[test]
    fn test_builder_chaining() {
        let config = BedrockConfig::new("ap-southeast-1")
            .with_profile_name("prod")
            .with_endpoint_override("http://localhost:4566");
        assert_eq!(config.region, "ap-southeast-1");
        assert_eq!(config.profile_name.as_deref(), Some("prod"));
        assert_eq!(
            config.endpoint_override.as_deref(),
            Some("http://localhost:4566")
        );
    }

    #[test]
    fn test_from_env_returns_a_config() {
        // from_env() reads AWS_REGION/AWS_DEFAULT_REGION/AWS_PROFILE from the
        // environment. We don't manipulate env vars in tests — just verify it
        // returns a valid config with a non-empty region (falls back to us-east-1).
        let config = BedrockConfig::from_env();
        assert!(!config.region.is_empty());
    }
}
