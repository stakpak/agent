//! StakAI inference client for the Agent API.

use stakai::{
    GenerateRequest, GenerateResponse, GenerateStream, Inference,
    providers::anthropic::{AnthropicConfig, AnthropicProvider},
    providers::copilot::{CopilotConfig, CopilotProvider},
    providers::gemini::{GeminiConfig, GeminiProvider},
    providers::openai::{OpenAIConfig, OpenAIProvider},
    providers::stakpak::{StakpakProvider, StakpakProviderConfig},
    registry::ProviderRegistry,
};
use stakpak_shared::models::openai_runtime::{
    OpenAIBackendResolutionInput, resolve_openai_runtime,
};
use stakpak_shared::models::{
    error::{AgentError, BadRequestErrorMessage},
    llm::{LLMProviderConfig, ProviderConfig},
};

#[derive(Clone)]
pub struct StakAIClient {
    inference: Inference,
}

impl StakAIClient {
    pub fn new(config: &LLMProviderConfig) -> Result<Self, AgentError> {
        let registry = build_provider_registry(config)
            .map_err(|error| invalid_agent_input(format!("Failed to build providers: {error}")))?;
        Self::with_registry(registry)
    }

    pub fn with_registry(registry: ProviderRegistry) -> Result<Self, AgentError> {
        let inference = Inference::builder()
            .with_registry(registry)
            .build()
            .map_err(|error| invalid_agent_input(error.to_string()))?;

        Ok(Self { inference })
    }

    pub async fn generate(
        &self,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, AgentError> {
        self.inference
            .generate(request)
            .await
            .map_err(|error| invalid_agent_input(error.to_string()))
    }

    pub async fn stream(&self, request: &GenerateRequest) -> Result<GenerateStream, AgentError> {
        self.inference
            .stream(request)
            .await
            .map_err(|error| invalid_agent_input(error.to_string()))
    }

    pub fn registry(&self) -> &ProviderRegistry {
        self.inference.registry()
    }
}

fn build_provider_registry(config: &LLMProviderConfig) -> Result<ProviderRegistry, String> {
    let mut registry = ProviderRegistry::new();

    for (name, provider_config) in &config.providers {
        match provider_config {
            ProviderConfig::OpenAI { .. } => {
                if let Some(openai_config) = resolve_stakai_openai_config(provider_config)? {
                    let provider = OpenAIProvider::new(openai_config)
                        .map_err(|error| format!("Failed to create OpenAI provider: {error}"))?;
                    registry = registry.register("openai", provider);
                }
            }
            ProviderConfig::Anthropic { api_endpoint, .. } => {
                if let Some(config) = anthropic_config(provider_config, api_endpoint.as_deref()) {
                    let provider = AnthropicProvider::new(config)
                        .map_err(|error| format!("Failed to create Anthropic provider: {error}"))?;
                    registry = registry.register("anthropic", provider);
                }
            }
            ProviderConfig::Gemini { api_endpoint, .. } => {
                if let Some(api_key) = provider_config.api_key() {
                    let mut config = GeminiConfig::new(api_key.to_string());
                    if let Some(endpoint) = api_endpoint {
                        config = config.with_base_url(endpoint.clone());
                    }
                    let provider = GeminiProvider::new(config)
                        .map_err(|error| format!("Failed to create Gemini provider: {error}"))?;
                    registry = registry.register("google", provider);
                }
            }
            ProviderConfig::Stakpak { api_endpoint, .. } => {
                let Some(api_key) = provider_config.api_key() else {
                    continue;
                };
                let mut config = StakpakProviderConfig::new(api_key.to_string())
                    .with_user_agent(format!("Stakpak/{}", env!("CARGO_PKG_VERSION")));
                if let Some(endpoint) = api_endpoint {
                    config = config.with_base_url(endpoint.clone());
                }
                let provider = StakpakProvider::new(config)
                    .map_err(|error| format!("Failed to create Stakpak provider: {error}"))?;
                registry = registry.register("stakpak", provider);
            }
            ProviderConfig::GitHubCopilot { api_endpoint, .. } => {
                if let Some(access_token) = provider_config.access_token() {
                    let mut config = CopilotConfig::new(access_token.to_string());
                    if let Some(endpoint) = api_endpoint {
                        config = config.with_base_url(endpoint.clone());
                    }
                    let provider = CopilotProvider::new(config)
                        .map_err(|error| format!("Failed to create Copilot provider: {error}"))?;
                    registry = registry.register("github-copilot", provider);
                }
            }
            ProviderConfig::Custom { api_endpoint, .. } => {
                let key = provider_config.api_key().unwrap_or_default().to_string();
                let config = OpenAIConfig::new(key).with_base_url(api_endpoint.clone());
                let provider = OpenAIProvider::new(config).map_err(|error| {
                    format!("Failed to create custom provider '{name}': {error}")
                })?;
                registry = registry.register(name, provider);
            }
            ProviderConfig::Bedrock { .. } => {}
        }
    }

    Ok(registry)
}

fn resolve_stakai_openai_config(
    provider_config: &ProviderConfig,
) -> Result<Option<OpenAIConfig>, String> {
    let resolved = resolve_openai_runtime(OpenAIBackendResolutionInput::new(
        Some(provider_config.clone()),
        provider_config.get_auth(),
    ))
    .map_err(|error| format!("Failed to resolve OpenAI runtime config: {error}"))?;

    Ok(resolved.map(|config| config.to_stakai_config()))
}

fn anthropic_config(
    provider_config: &ProviderConfig,
    api_endpoint: Option<&str>,
) -> Option<AnthropicConfig> {
    let mut config = if let Some(token) = provider_config.access_token() {
        AnthropicConfig::with_oauth(token)
    } else if let Some(key) = provider_config.api_key() {
        AnthropicConfig::new(key)
    } else {
        return None;
    };

    if let Some(endpoint) = api_endpoint {
        config = config.with_base_url(endpoint);
    }

    Some(config)
}

fn invalid_agent_input(message: String) -> AgentError {
    AgentError::BadRequest(BadRequestErrorMessage::InvalidAgentInput(message))
}
