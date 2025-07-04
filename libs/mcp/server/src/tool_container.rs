use rmcp::tool_handler;
use rmcp::{
    Error as McpError, RoleServer, ServerHandler, handler::server::tool::ToolRouter, model::*,
    service::RequestContext, tool_router,
};
use stakpak_api::ClientConfig;

use crate::secret_manager::SecretManager;

#[derive(Clone)]
pub struct ToolContainer {
    pub api_config: ClientConfig,
    pub secret_manager: SecretManager,
    pub tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ToolContainer {
    pub fn new(
        api_config: ClientConfig,
        redact_secrets: bool,
        tool_router: ToolRouter<Self>,
    ) -> Self {
        Self {
            api_config,
            secret_manager: SecretManager::new(redact_secrets),
            tool_router,
        }
    }

    pub fn get_secret_manager(&self) -> &SecretManager {
        &self.secret_manager
    }

    pub fn get_api_config(&self) -> &ClientConfig {
        &self.api_config
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
