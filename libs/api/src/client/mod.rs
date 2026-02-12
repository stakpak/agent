//! Unified AgentClient
//!
//! The AgentClient provides a unified interface that:
//! - Uses stakai for all LLM inference (with StakpakProvider when available)
//! - Uses StakpakApiClient for non-inference APIs (sessions, billing, etc.)
//! - Falls back to local SQLite DB when Stakpak is unavailable
//! - Integrates with hooks for lifecycle events

mod provider;

use crate::local::hooks::task_board_context::{TaskBoardContextHook, TaskBoardContextHookOptions};
use crate::local::storage::LocalStorage;
use crate::models::AgentState;
use crate::stakpak::storage::StakpakStorage;
use crate::stakpak::{StakpakApiClient, StakpakApiConfig};
use crate::storage::SessionStorage;

use stakpak_shared::hooks::{HookRegistry, LifecycleEvent};
use stakpak_shared::models::llm::{LLMProviderConfig, ProviderConfig};
use stakpak_shared::models::stakai_adapter::StakAIClient;
use std::sync::Arc;

// =============================================================================
// AgentClient Configuration
// =============================================================================

/// Model options for the AgentClient
#[derive(Clone, Debug, Default)]
pub struct ModelOptions {
    /// Primary model for complex tasks
    pub smart_model: Option<String>,
    /// Economy model for simpler tasks
    pub eco_model: Option<String>,
    /// Fallback model when primary providers fail
    pub recovery_model: Option<String>,
}

/// Default Stakpak API endpoint
pub const DEFAULT_STAKPAK_ENDPOINT: &str = "https://apiv2.stakpak.dev";

/// Stakpak connection configuration
#[derive(Debug, Clone)]
pub struct StakpakConfig {
    /// Stakpak API key
    pub api_key: String,
    /// Stakpak API endpoint (default: https://apiv2.stakpak.dev)
    pub api_endpoint: String,
}

impl StakpakConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_endpoint: DEFAULT_STAKPAK_ENDPOINT.to_string(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.api_endpoint = endpoint.into();
        self
    }
}

/// Configuration for creating an AgentClient
#[derive(Debug, Default)]
pub struct AgentClientConfig {
    /// Stakpak configuration (optional - enables remote features when present)
    pub stakpak: Option<StakpakConfig>,
    /// LLM provider configurations
    pub providers: LLMProviderConfig,
    /// Smart model override
    pub smart_model: Option<String>,
    /// Eco model override
    pub eco_model: Option<String>,
    /// Recovery model override
    pub recovery_model: Option<String>,
    /// Local database path (default: ~/.stakpak/data/local.db)
    pub store_path: Option<String>,
    /// Hook registry for lifecycle events
    pub hook_registry: Option<HookRegistry<AgentState>>,
}

impl AgentClientConfig {
    /// Create new config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set Stakpak configuration
    ///
    /// Use `StakpakConfig::new(api_key).with_endpoint(endpoint)` to configure.
    pub fn with_stakpak(mut self, config: StakpakConfig) -> Self {
        self.stakpak = Some(config);
        self
    }

    /// Set providers
    pub fn with_providers(mut self, providers: LLMProviderConfig) -> Self {
        self.providers = providers;
        self
    }

    /// Set smart model
    pub fn with_smart_model(mut self, model: impl Into<String>) -> Self {
        self.smart_model = Some(model.into());
        self
    }

    /// Set eco model
    pub fn with_eco_model(mut self, model: impl Into<String>) -> Self {
        self.eco_model = Some(model.into());
        self
    }

    /// Set recovery model
    pub fn with_recovery_model(mut self, model: impl Into<String>) -> Self {
        self.recovery_model = Some(model.into());
        self
    }

    /// Set local database path
    pub fn with_store_path(mut self, path: impl Into<String>) -> Self {
        self.store_path = Some(path.into());
        self
    }

    /// Set hook registry
    pub fn with_hook_registry(mut self, registry: HookRegistry<AgentState>) -> Self {
        self.hook_registry = Some(registry);
        self
    }
}

// =============================================================================
// AgentClient
// =============================================================================

const DEFAULT_STORE_PATH: &str = ".stakpak/data/local.db";

/// Unified agent client
///
/// Provides a single interface for:
/// - LLM inference via stakai (with Stakpak or direct providers)
/// - Session/checkpoint management via SessionStorage trait (Stakpak API or local SQLite)
/// - MCP tools, billing, rulebooks (Stakpak API only)
#[derive(Clone)]
pub struct AgentClient {
    /// StakAI client for all LLM inference
    pub(crate) stakai: StakAIClient,
    /// Stakpak API client for non-inference operations (optional)
    pub(crate) stakpak_api: Option<StakpakApiClient>,
    /// Session storage implementation (abstracts Stakpak API vs local SQLite)
    pub(crate) session_storage: Arc<dyn SessionStorage>,
    /// Hook registry for lifecycle events
    pub(crate) hook_registry: Arc<HookRegistry<AgentState>>,
    /// Model configuration
    pub(crate) model_options: ModelOptions,
    /// Stakpak configuration (for reference)
    pub(crate) stakpak: Option<StakpakConfig>,
}

impl AgentClient {
    /// Create a new AgentClient
    pub async fn new(config: AgentClientConfig) -> Result<Self, String> {
        // 1. Build LLMProviderConfig with Stakpak if configured (only if api_key is not empty)
        let mut providers = config.providers.clone();
        if let Some(stakpak) = &config.stakpak
            && !stakpak.api_key.is_empty()
        {
            providers.providers.insert(
                "stakpak".to_string(),
                ProviderConfig::Stakpak {
                    api_key: stakpak.api_key.clone(),
                    api_endpoint: Some(stakpak.api_endpoint.clone()),
                },
            );
        }

        // 2. Create StakAIClient with all providers
        let stakai = StakAIClient::new(&providers)
            .map_err(|e| format!("Failed to create StakAI client: {}", e))?;

        // 3. Create StakpakApiClient if configured (only if api_key is not empty)
        let stakpak_api = if let Some(stakpak) = &config.stakpak {
            if !stakpak.api_key.is_empty() {
                Some(
                    StakpakApiClient::new(&StakpakApiConfig {
                        api_key: stakpak.api_key.clone(),
                        api_endpoint: stakpak.api_endpoint.clone(),
                    })
                    .map_err(|e| format!("Failed to create Stakpak API client: {}", e))?,
                )
            } else {
                None
            }
        } else {
            None
        };

        // 4. Create session storage (Stakpak API or local SQLite)
        let session_storage: Arc<dyn SessionStorage> = if let Some(stakpak) = &config.stakpak
            && !stakpak.api_key.is_empty()
        {
            Arc::new(
                StakpakStorage::new(&stakpak.api_key, &stakpak.api_endpoint)
                    .map_err(|e| format!("Failed to create Stakpak storage: {}", e))?,
            )
        } else {
            let store_path = config.store_path.clone().unwrap_or_else(|| {
                std::env::var("HOME")
                    .map(|h| format!("{}/{}", h, DEFAULT_STORE_PATH))
                    .unwrap_or_else(|_| DEFAULT_STORE_PATH.to_string())
            });
            Arc::new(
                LocalStorage::new(&store_path)
                    .await
                    .map_err(|e| format!("Failed to create local storage: {}", e))?,
            )
        };

        // 5. Parse model options
        let model_options = ModelOptions {
            smart_model: config.smart_model,
            eco_model: config.eco_model,
            recovery_model: config.recovery_model,
        };

        // 6. Setup hook registry with context management hooks
        let mut hook_registry = config.hook_registry.unwrap_or_default();
        hook_registry.register(
            LifecycleEvent::BeforeInference,
            Box::new(TaskBoardContextHook::new(TaskBoardContextHookOptions {
                keep_last_n_assistant_messages: Some(5), // Keep the last 5 assistant messages in context
                context_budget_threshold: Some(0.8),     // defaults to 0.8 (80%)
            })),
        );
        let hook_registry = Arc::new(hook_registry);

        Ok(Self {
            stakai,
            stakpak_api,
            session_storage,
            hook_registry,
            model_options,
            stakpak: config.stakpak,
        })
    }

    /// Check if Stakpak API is available
    pub fn has_stakpak(&self) -> bool {
        self.stakpak_api.is_some()
    }

    /// Get the Stakpak API endpoint (with default fallback)
    pub fn get_stakpak_api_endpoint(&self) -> &str {
        self.stakpak
            .as_ref()
            .map(|s| s.api_endpoint.as_str())
            .unwrap_or(DEFAULT_STAKPAK_ENDPOINT)
    }

    /// Get reference to the StakAI client
    pub fn stakai(&self) -> &StakAIClient {
        &self.stakai
    }

    /// Get reference to the Stakpak API client (if available)
    pub fn stakpak_api(&self) -> Option<&StakpakApiClient> {
        self.stakpak_api.as_ref()
    }

    /// Get reference to the hook registry
    pub fn hook_registry(&self) -> &Arc<HookRegistry<AgentState>> {
        &self.hook_registry
    }

    /// Get the model options
    pub fn model_options(&self) -> &ModelOptions {
        &self.model_options
    }

    /// Get reference to the session storage
    ///
    /// Use this for all session and checkpoint operations.
    pub fn session_storage(&self) -> &Arc<dyn SessionStorage> {
        &self.session_storage
    }
}

// Debug implementation for AgentClient
impl std::fmt::Debug for AgentClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentClient")
            .field("has_stakpak", &self.has_stakpak())
            .field("model_options", &self.model_options)
            .finish_non_exhaustive()
    }
}
