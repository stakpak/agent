use super::EnabledToolsConfig;
use rmcp::tool_handler;
use rmcp::{
    Error as McpError, RoleServer, ServerHandler, handler::server::tool::ToolRouter, model::*,
    service::RequestContext, tool_router,
};
use stakpak_api::{Client, ClientConfig};
use stakpak_shared::models::subagent::SubagentConfigs;
use stakpak_shared::remote_connection::RemoteConnectionManager;
use stakpak_shared::secret_manager::SecretManager;
use stakpak_shared::task_manager::TaskManagerHandle;
use std::sync::Arc;

#[derive(Clone)]
pub struct ToolContainer {
    pub client: Option<Client>,
    pub secret_manager: SecretManager,
    pub task_manager: Arc<TaskManagerHandle>,
    pub remote_connection_manager: Arc<RemoteConnectionManager>,
    pub subagent_configs: Option<SubagentConfigs>,
    pub enabled_tools: EnabledToolsConfig,
    pub tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ToolContainer {
    pub fn new(
        api_config: Option<ClientConfig>,
        redact_secrets: bool,
        privacy_mode: bool,
        enabled_tools: EnabledToolsConfig,
        task_manager: Arc<TaskManagerHandle>,
        subagent_configs: Option<SubagentConfigs>,
        tool_router: ToolRouter<Self>,
    ) -> Result<Self, String> {
        let client = if let Some(api_config) = api_config {
            Some(Client::new(&api_config)?)
        } else {
            None
        };

        Ok(Self {
            client,
            secret_manager: SecretManager::new(redact_secrets, privacy_mode),
            task_manager,
            remote_connection_manager: Arc::new(RemoteConnectionManager::new()),
            subagent_configs,
            enabled_tools,
            tool_router,
        })
    }

    pub fn get_secret_manager(&self) -> &SecretManager {
        &self.secret_manager
    }

    pub fn get_client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    pub fn get_task_manager(&self) -> &Arc<TaskManagerHandle> {
        &self.task_manager
    }

    pub fn get_remote_connection_manager(&self) -> &Arc<RemoteConnectionManager> {
        &self.remote_connection_manager
    }

    pub fn get_subagent_configs(&self) -> &Option<SubagentConfigs> {
        &self.subagent_configs
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
