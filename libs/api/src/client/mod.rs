//! Unified AgentClient
//!
//! The AgentClient provides a unified interface that:
//! - Uses stakai for all LLM inference (with StakpakProvider when available)
//! - Uses StakpakApiClient for non-inference APIs (sessions, billing, etc.)
//! - Falls back to local SQLite DB when Stakpak is unavailable
//! - Integrates with hooks for lifecycle events

mod provider;

use crate::local::db;
use crate::local::hooks::inline_scratchpad_context::{
    InlineScratchpadContextHook, InlineScratchpadContextHookOptions,
};
use crate::models::AgentState;
use crate::stakpak::{StakpakApiClient, StakpakApiConfig};
use libsql::Connection;
use stakpak_shared::hooks::{HookRegistry, LifecycleEvent};
use stakpak_shared::models::llm::{LLMModel, LLMProviderConfig, ProviderConfig};
use stakpak_shared::models::stakai_adapter::StakAIClient;
use std::path::PathBuf;
use std::sync::Arc;

// =============================================================================
// AgentClient Configuration
// =============================================================================

/// Model options for the AgentClient
#[derive(Clone, Debug, Default)]
pub struct ModelOptions {
    /// Primary model for complex tasks
    pub smart_model: Option<LLMModel>,
    /// Economy model for simpler tasks
    pub eco_model: Option<LLMModel>,
    /// Fallback model when primary providers fail
    pub recovery_model: Option<LLMModel>,
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
/// - Session/checkpoint management (Stakpak API or local SQLite)
/// - MCP tools, billing, rulebooks (Stakpak API only)
#[derive(Clone)]
pub struct AgentClient {
    /// StakAI client for all LLM inference
    pub(crate) stakai: StakAIClient,
    /// Stakpak API client for non-inference operations (optional)
    pub(crate) stakpak_api: Option<StakpakApiClient>,
    /// Local SQLite database for fallback storage
    pub(crate) local_db: Connection,
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

        // 4. Initialize local SQLite database
        let store_path = config.store_path.map(PathBuf::from).unwrap_or_else(|| {
            std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(DEFAULT_STORE_PATH)
        });

        if let Some(parent) = store_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create database directory: {}", e))?;
        }

        let db = libsql::Builder::new_local(store_path.display().to_string())
            .build()
            .await
            .map_err(|e| format!("Failed to open database: {}", e))?;
        let local_db = db
            .connect()
            .map_err(|e| format!("Failed to connect to database: {}", e))?;
        db::init_schema(&local_db).await?;

        // 5. Parse model options
        let model_options = ModelOptions {
            smart_model: config.smart_model.map(LLMModel::from),
            eco_model: config.eco_model.map(LLMModel::from),
            recovery_model: config.recovery_model.map(LLMModel::from),
        };

        // 6. Setup hook registry with context management hooks
        let mut hook_registry = config
            .hook_registry
            .unwrap_or_else(|| HookRegistry::default());
        hook_registry.register(
            LifecycleEvent::BeforeInference,
            Box::new(InlineScratchpadContextHook::new(
                InlineScratchpadContextHookOptions {
                    history_action_message_size_limit: Some(100),
                    history_action_message_keep_last_n: Some(1),
                    history_action_result_keep_last_n: Some(50),
                },
            )),
        );
        let hook_registry = Arc::new(hook_registry);

        Ok(Self {
            stakai,
            stakpak_api,
            local_db,
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

    /// Get reference to the local database
    pub fn local_db(&self) -> &Connection {
        &self.local_db
    }

    /// Get reference to the hook registry
    pub fn hook_registry(&self) -> &Arc<HookRegistry<AgentState>> {
        &self.hook_registry
    }

    /// Get the model options
    pub fn model_options(&self) -> &ModelOptions {
        &self.model_options
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
