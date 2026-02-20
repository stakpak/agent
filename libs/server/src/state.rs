use crate::{
    checkpoint_store::CheckpointStore, event_log::EventLog, idempotency::IdempotencyStore,
    sandbox::SandboxConfig, session_manager::SessionManager,
};
use stakpak_agent_core::{ProposedToolCall, ToolApprovalPolicy};
use stakpak_api::SessionStorage;
use stakpak_mcp_client::McpClient;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PendingToolApprovals {
    pub run_id: Uuid,
    pub tool_calls: Vec<ProposedToolCall>,
}

#[derive(Clone)]
pub struct AppState {
    pub run_manager: SessionManager,
    /// Durable session/checkpoint backend (SQLite/remote API).
    pub session_store: Arc<dyn SessionStorage>,
    pub events: Arc<EventLog>,
    pub idempotency: Arc<IdempotencyStore>,
    pub inference: Arc<stakai::Inference>,
    /// Server-side latest-envelope cache (`stakai::Message` + runtime metadata).
    pub checkpoint_store: Arc<CheckpointStore>,
    pub models: Arc<Vec<stakai::Model>>,
    pub default_model: Option<stakai::Model>,
    pub tool_approval_policy: ToolApprovalPolicy,
    pub started_at: Instant,
    pub mcp_client: Option<Arc<McpClient>>,
    pub mcp_tools: Arc<RwLock<Vec<stakai::Tool>>>,
    pub mcp_server_shutdown_tx: Option<broadcast::Sender<()>>,
    pub mcp_proxy_shutdown_tx: Option<broadcast::Sender<()>>,
    pub sandbox_config: Option<SandboxConfig>,
    pending_tools: Arc<RwLock<HashMap<Uuid, PendingToolApprovals>>>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_store: Arc<dyn SessionStorage>,
        events: Arc<EventLog>,
        idempotency: Arc<IdempotencyStore>,
        inference: Arc<stakai::Inference>,
        models: Vec<stakai::Model>,
        default_model: Option<stakai::Model>,
        tool_approval_policy: ToolApprovalPolicy,
    ) -> Self {
        Self {
            run_manager: SessionManager::new(),
            session_store,
            events,
            idempotency,
            inference,
            checkpoint_store: Arc::new(CheckpointStore::default_local()),
            models: Arc::new(models),
            default_model,
            tool_approval_policy,
            started_at: Instant::now(),
            mcp_client: None,
            mcp_tools: Arc::new(RwLock::new(Vec::new())),
            mcp_server_shutdown_tx: None,
            mcp_proxy_shutdown_tx: None,
            sandbox_config: None,
            pending_tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_mcp(
        mut self,
        mcp_client: Arc<McpClient>,
        mcp_tools: Vec<stakai::Tool>,
        mcp_server_shutdown_tx: Option<broadcast::Sender<()>>,
        mcp_proxy_shutdown_tx: Option<broadcast::Sender<()>>,
    ) -> Self {
        self.mcp_client = Some(mcp_client);
        self.mcp_tools = Arc::new(RwLock::new(mcp_tools));
        self.mcp_server_shutdown_tx = mcp_server_shutdown_tx;
        self.mcp_proxy_shutdown_tx = mcp_proxy_shutdown_tx;
        self
    }

    pub fn with_sandbox(mut self, sandbox_config: SandboxConfig) -> Self {
        self.sandbox_config = Some(sandbox_config);
        self
    }

    pub fn with_checkpoint_store(mut self, checkpoint_store: Arc<CheckpointStore>) -> Self {
        self.checkpoint_store = checkpoint_store;
        self
    }

    pub async fn current_mcp_tools(&self) -> Vec<stakai::Tool> {
        self.mcp_tools.read().await.clone()
    }

    pub async fn refresh_mcp_tools(&self) -> Result<usize, String> {
        let Some(mcp_client) = self.mcp_client.as_ref() else {
            return Ok(self.mcp_tools.read().await.len());
        };

        let raw_tools = stakpak_mcp_client::get_tools(mcp_client)
            .await
            .map_err(|error| format!("Failed to refresh MCP tools: {error}"))?;

        let converted = raw_tools
            .into_iter()
            .map(|tool| stakai::Tool {
                tool_type: "function".to_string(),
                function: stakai::ToolFunction {
                    name: tool.name.as_ref().to_string(),
                    description: tool
                        .description
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        .unwrap_or_default(),
                    parameters: serde_json::Value::Object((*tool.input_schema).clone()),
                },
                provider_options: None,
            })
            .collect::<Vec<_>>();

        let mut guard = self.mcp_tools.write().await;
        *guard = converted;
        Ok(guard.len())
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn resolve_model(&self, requested: Option<&str>) -> Option<stakai::Model> {
        match requested {
            Some(requested_model) => self.find_model(requested_model),
            None => self
                .default_model
                .clone()
                .or_else(|| self.models.first().cloned()),
        }
    }

    pub async fn set_pending_tools(
        &self,
        session_id: Uuid,
        run_id: Uuid,
        tool_calls: Vec<ProposedToolCall>,
    ) {
        let mut guard = self.pending_tools.write().await;
        guard.insert(session_id, PendingToolApprovals { run_id, tool_calls });
    }

    pub async fn clear_pending_tools(&self, session_id: Uuid, run_id: Uuid) {
        let mut guard = self.pending_tools.write().await;
        if guard
            .get(&session_id)
            .is_some_and(|pending| pending.run_id == run_id)
        {
            guard.remove(&session_id);
        }
    }

    pub async fn pending_tools(&self, session_id: Uuid) -> Option<PendingToolApprovals> {
        let guard = self.pending_tools.read().await;
        guard.get(&session_id).cloned()
    }

    fn find_model(&self, requested: &str) -> Option<stakai::Model> {
        if let Some((provider, id)) = requested.split_once('/') {
            return self
                .models
                .iter()
                .find(|model| model.provider == provider && model.id == id)
                .cloned()
                .or_else(|| Some(stakai::Model::custom(id, provider)));
        }

        self.models
            .iter()
            .find(|model| model.id == requested)
            .cloned()
            .or_else(|| {
                self.default_model.as_ref().map(|default_model| {
                    stakai::Model::custom(requested.to_string(), default_model.provider.clone())
                })
            })
            .or_else(|| {
                self.models.first().map(|model| {
                    stakai::Model::custom(requested.to_string(), model.provider.clone())
                })
            })
            .or_else(|| Some(stakai::Model::custom(requested.to_string(), "openai")))
    }
}
