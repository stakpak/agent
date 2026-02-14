//! Client configuration

use crate::providers::{
    anthropic::AnthropicConfig, gemini::GeminiConfig, openai::OpenAIConfig,
    stakpak::StakpakProviderConfig,
};

#[cfg(feature = "bedrock")]
use crate::providers::bedrock::BedrockConfig;

/// Configuration for the AI client
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Default temperature for requests
    pub default_temperature: Option<f32>,
    /// Default max tokens for requests
    pub default_max_tokens: Option<u32>,
    /// Request timeout in seconds
    pub timeout_seconds: Option<u64>,
}

impl ClientConfig {
    /// Create a new default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set default temperature
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.default_temperature = Some(temperature);
        self
    }

    /// Set default max tokens
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.default_max_tokens = Some(max_tokens);
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_seconds = Some(seconds);
        self
    }
}

/// Provider configuration for Inference client
///
/// Allows configuring multiple AI providers with their API keys and base URLs.
///
/// # Example
///
/// ```rust,no_run
/// use stakai::{Inference, InferenceConfig};
///
/// let client = Inference::with_config(
///     InferenceConfig::new()
///         .openai("sk-...", None)
///         .anthropic("sk-ant-...", None)
///         .gemini("your-key", None)
///         .stakpak("your-stakpak-key", None)
/// );
/// ```
#[derive(Debug, Default)]
pub struct InferenceConfig {
    pub(crate) openai_config: Option<OpenAIConfig>,
    pub(crate) anthropic_config: Option<AnthropicConfig>,
    pub(crate) gemini_config: Option<GeminiConfig>,
    pub(crate) stakpak_config: Option<StakpakProviderConfig>,
    #[cfg(feature = "bedrock")]
    pub(crate) bedrock_config: Option<BedrockConfig>,
    pub(crate) client_config: ClientConfig,
}

impl InferenceConfig {
    /// Create a new inference configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure OpenAI provider with API key and optional base URL
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::InferenceConfig;
    /// let config = InferenceConfig::new()
    ///     .openai("sk-...", None);
    ///
    /// // With custom base URL (e.g., Azure OpenAI)
    /// let config = InferenceConfig::new()
    ///     .openai("sk-...", Some("https://your-endpoint.openai.azure.com/v1".to_string()));
    /// ```
    pub fn openai(mut self, api_key: impl Into<String>, base_url: Option<String>) -> Self {
        let mut config = OpenAIConfig::new(api_key);
        if let Some(url) = base_url {
            config = config.with_base_url(url);
        }
        self.openai_config = Some(config);
        self
    }

    /// Configure OpenAI provider with full OpenAIConfig
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{InferenceConfig, providers::openai::OpenAIConfig};
    /// let openai_config = OpenAIConfig::new("sk-...")
    ///     .with_base_url("https://custom.com/v1")
    ///     .with_organization("org-123");
    ///
    /// let config = InferenceConfig::new()
    ///     .openai_config(openai_config);
    /// ```
    pub fn openai_config(mut self, config: OpenAIConfig) -> Self {
        self.openai_config = Some(config);
        self
    }

    /// Configure Anthropic provider with API key and optional base URL
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::InferenceConfig;
    /// let config = InferenceConfig::new()
    ///     .anthropic("sk-ant-...", None);
    ///
    /// // With custom base URL
    /// let config = InferenceConfig::new()
    ///     .anthropic("sk-ant-...", Some("https://custom-anthropic.com/v1".to_string()));
    /// ```
    pub fn anthropic(mut self, api_key: impl Into<String>, base_url: Option<String>) -> Self {
        let mut config = AnthropicConfig::new(api_key);
        if let Some(url) = base_url {
            config = config.with_base_url(url);
        }
        self.anthropic_config = Some(config);
        self
    }

    /// Configure Anthropic provider with full AnthropicConfig
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{InferenceConfig, providers::anthropic::AnthropicConfig};
    /// let anthropic_config = AnthropicConfig::new("sk-ant-...")
    ///     .with_version("2023-06-01")
    ///     .with_beta_feature("prompt-caching-2024-07-31");
    ///
    /// let config = InferenceConfig::new()
    ///     .anthropic_config(anthropic_config);
    /// ```
    pub fn anthropic_config(mut self, config: AnthropicConfig) -> Self {
        self.anthropic_config = Some(config);
        self
    }

    /// Configure Gemini provider with API key and optional base URL
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::InferenceConfig;
    /// let config = InferenceConfig::new()
    ///     .gemini("your-api-key", None);
    ///
    /// // With custom base URL
    /// let config = InferenceConfig::new()
    ///     .gemini("your-key", Some("https://custom-gemini.com/v1beta".to_string()));
    /// ```
    pub fn gemini(mut self, api_key: impl Into<String>, base_url: Option<String>) -> Self {
        let mut config = GeminiConfig::new(api_key);
        if let Some(url) = base_url {
            config = config.with_base_url(url);
        }
        self.gemini_config = Some(config);
        self
    }

    /// Configure Gemini provider with full GeminiConfig
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{InferenceConfig, providers::gemini::GeminiConfig};
    /// let gemini_config = GeminiConfig::new("your-key")
    ///     .with_base_url("https://custom.com/v1beta");
    ///
    /// let config = InferenceConfig::new()
    ///     .gemini_config(gemini_config);
    /// ```
    pub fn gemini_config(mut self, config: GeminiConfig) -> Self {
        self.gemini_config = Some(config);
        self
    }

    /// Set default temperature for all requests
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.client_config.default_temperature = Some(temperature);
        self
    }

    /// Set default max tokens for all requests
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.client_config.default_max_tokens = Some(max_tokens);
        self
    }

    /// Set request timeout in seconds
    pub fn timeout(mut self, seconds: u64) -> Self {
        self.client_config.timeout_seconds = Some(seconds);
        self
    }

    /// Configure Stakpak provider with API key and optional base URL
    ///
    /// Stakpak provides unified access to multiple LLM providers through
    /// a single API endpoint with usage tracking and billing.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::InferenceConfig;
    /// let config = InferenceConfig::new()
    ///     .stakpak("your-api-key", None);
    ///
    /// // With custom base URL
    /// let config = InferenceConfig::new()
    ///     .stakpak("your-api-key", Some("https://custom.stakpak.dev".to_string()));
    /// ```
    pub fn stakpak(mut self, api_key: impl Into<String>, base_url: Option<String>) -> Self {
        let mut config = StakpakProviderConfig::new(api_key);
        if let Some(url) = base_url {
            config = config.with_base_url(url);
        }
        self.stakpak_config = Some(config);
        self
    }

    /// Configure Stakpak provider with full StakpakProviderConfig
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{InferenceConfig, providers::stakpak::StakpakProviderConfig};
    /// let stakpak_config = StakpakProviderConfig::new("your-api-key")
    ///     .with_base_url("https://custom.stakpak.dev");
    ///
    /// let config = InferenceConfig::new()
    ///     .stakpak_config(stakpak_config);
    /// ```
    pub fn stakpak_config(mut self, config: StakpakProviderConfig) -> Self {
        self.stakpak_config = Some(config);
        self
    }

    /// Configure AWS Bedrock provider with a region
    ///
    /// Uses the AWS credential chain for authentication (env vars, shared credentials,
    /// SSO, ECS/EC2 instance roles). No API key needed.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::InferenceConfig;
    /// let config = InferenceConfig::new()
    ///     .bedrock("us-east-1");
    /// ```
    #[cfg(feature = "bedrock")]
    pub fn bedrock(mut self, region: impl Into<String>) -> Self {
        self.bedrock_config = Some(BedrockConfig::new(region));
        self
    }

    /// Configure AWS Bedrock provider from environment variables
    ///
    /// Reads `AWS_REGION` (or `AWS_DEFAULT_REGION`) and `AWS_PROFILE` from the environment.
    /// Falls back to `us-east-1` if no region is set.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::InferenceConfig;
    /// let config = InferenceConfig::new()
    ///     .bedrock_from_env();
    /// ```
    #[cfg(feature = "bedrock")]
    pub fn bedrock_from_env(mut self) -> Self {
        self.bedrock_config = Some(BedrockConfig::from_env());
        self
    }

    /// Configure AWS Bedrock provider with full BedrockConfig
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use stakai::{InferenceConfig, providers::bedrock::BedrockConfig};
    /// let bedrock_config = BedrockConfig::new("us-west-2")
    ///     .with_profile_name("production");
    ///
    /// let config = InferenceConfig::new()
    ///     .bedrock_config(bedrock_config);
    /// ```
    #[cfg(feature = "bedrock")]
    pub fn bedrock_config(mut self, config: BedrockConfig) -> Self {
        self.bedrock_config = Some(config);
        self
    }
}
