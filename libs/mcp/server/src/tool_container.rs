use super::{EnabledToolsConfig, SubagentConfig};
use rmcp::tool_handler;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, handler::server::tool::ToolRouter, model::*,
    service::RequestContext, tool_router,
};
use stakpak_api::AgentProvider;
use stakpak_shared::remote_connection::RemoteConnectionManager;
use stakpak_shared::task_manager::TaskManagerHandle;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct LocalToolRuntimeDefaults {
    active_profile_name: Option<String>,
}

impl LocalToolRuntimeDefaults {
    pub fn new(active_profile_name: Option<String>) -> Self {
        let active_profile_name = active_profile_name
            .map(|profile| profile.trim().to_string())
            .filter(|profile| !profile.is_empty());

        Self {
            active_profile_name,
        }
    }

    pub fn active_profile_name(&self) -> Option<&str> {
        self.active_profile_name.as_deref()
    }
}

#[derive(Clone)]
pub struct ToolContainer {
    pub client: Option<Arc<dyn AgentProvider>>,
    pub task_manager: Arc<TaskManagerHandle>,
    pub remote_connection_manager: Arc<RemoteConnectionManager>,
    pub enabled_tools: EnabledToolsConfig,
    pub tool_router: ToolRouter<Self>,
    pub skill_directories: Vec<PathBuf>,
    pub subagent_config: SubagentConfig,
    pub local_runtime_defaults: LocalToolRuntimeDefaults,
}

#[tool_router]
impl ToolContainer {
    pub fn new(
        client: Option<Arc<dyn AgentProvider>>,
        enabled_tools: EnabledToolsConfig,
        task_manager: Arc<TaskManagerHandle>,
        tool_router: ToolRouter<Self>,
        skill_directories: Vec<PathBuf>,
        subagent_config: SubagentConfig,
    ) -> Result<Self, String> {
        let local_runtime_defaults =
            LocalToolRuntimeDefaults::new(subagent_config.profile_name.clone());

        Ok(Self {
            client,
            task_manager,
            remote_connection_manager: Arc::new(RemoteConnectionManager::new()),
            enabled_tools,
            tool_router,
            skill_directories,
            subagent_config,
            local_runtime_defaults,
        })
    }

    pub fn get_client(&self) -> Option<&Arc<dyn AgentProvider>> {
        self.client.as_ref()
    }

    pub fn get_task_manager(&self) -> &Arc<TaskManagerHandle> {
        &self.task_manager
    }

    pub fn get_remote_connection_manager(&self) -> &Arc<RemoteConnectionManager> {
        &self.remote_connection_manager
    }

    pub fn get_session_id(&self, ctx: &RequestContext<RoleServer>) -> Option<String> {
        ctx.meta
            .get("session_id")
            .and_then(|s| s.as_str().map(|s| s.to_string()))
    }
}

#[tool_handler]
impl ServerHandler for ToolContainer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("The Stakpak MCP server.".to_string()),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}
