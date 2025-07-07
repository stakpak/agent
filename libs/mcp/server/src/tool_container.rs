use rmcp::tool_handler;
use rmcp::{
    Error as McpError, RoleServer, ServerHandler, handler::server::tool::ToolRouter, model::*,
    service::RequestContext, tool_router,
};
use stakpak_api::{Client, ClientConfig};
use stakpak_shared::secret_manager::SecretManager;

#[derive(Clone)]
pub struct ToolContainer {
    pub client: Option<Client>,
    pub secret_manager: SecretManager,
    pub tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ToolContainer {
    pub fn new(
        api_config: Option<ClientConfig>,
        redact_secrets: bool,
        privacy_mode: bool,
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
            tool_router,
        })
    }

    pub fn get_secret_manager(&self) -> &SecretManager {
        &self.secret_manager
    }

    pub fn get_client(&self) -> Option<&Client> {
        self.client.as_ref()
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
