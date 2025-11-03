use rmcp::model::ServerCapabilities;
use rmcp::service::RequestContext;
use rmcp::{
    RoleServer, ServerHandler, ServiceError,
    model::{
        CallToolRequestParam, CallToolResult, ErrorData, GetPromptRequestParam, GetPromptResult,
        Implementation, InitializeRequestParam, InitializeResult, ListPromptsResult,
        ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParam,
        ProtocolVersion, ReadResourceRequestParam, ReadResourceResult,
    },
};

use crate::client::ClientPool;
use std::sync::Arc;

pub struct ProxyServer {
    pool: Arc<ClientPool>,
}

impl ProxyServer {
    pub fn new(pool: Arc<ClientPool>) -> Self {
        Self { pool }
    }
}

impl ServerHandler for ProxyServer {
    async fn initialize(
        &self,
        _params: InitializeRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        // Initialize all client servers

        // Return combined capabilities from all servers
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "proxy-server".to_string(),
                version: "0.1.0".to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: None,
        })
    }

    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut all_tools = Vec::new();

        // Aggregate tools from all MCP servers
        let clients = self.pool.get_clients().await;
        for (name, client) in clients.iter() {
            match client.list_tools(params.clone()).await {
                Ok(result) => {
                    all_tools.extend(result.tools.into_iter().map(|mut tool| {
                        tool.name = format!("{}_{}", name, tool.name).into();
                        tool
                    }));
                }
                Err(e) => {
                    tracing::warn!("Failed to list tools from client {}: {:?}", name, e);
                }
            }
        }

        Ok(ListToolsResult {
            tools: all_tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // Parse the client name from the tool name (format: client_name_tool_name)
        let parts: Vec<&str> = params.name.splitn(2, '_').collect();

        if parts.len() != 2 {
            return Err(ErrorData::invalid_params(
                format!(
                    "Invalid tool name format: {}. Expected format: client_name_tool_name",
                    params.name
                ),
                None,
            ));
        }

        let client_name = parts[0];
        let tool_name = parts[1];

        // Find the specific client and call the tool
        let clients = self.pool.get_clients().await;
        match clients.get(client_name) {
            Some(client) => {
                let mut tool_params = params.clone();
                tool_params.name = tool_name.to_string().into();

                // Hold the lock during the async call since RunningService is not Clone
                client.call_tool(tool_params).await.map_err(|e| match e {
                    ServiceError::McpError(err) => err,
                    _ => ErrorData::internal_error(
                        format!(
                            "Failed to call tool {} on client {}",
                            tool_name, client_name
                        ),
                        None,
                    ),
                })
            }
            None => Err(ErrorData::resource_not_found(
                format!("Client {} not found", client_name),
                None,
            )),
        }
    }

    async fn list_prompts(
        &self,
        params: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let mut all_prompts = Vec::new();

        // Aggregate prompts from all MCP servers
        let clients = self.pool.get_clients().await;
        for (name, client) in clients.iter() {
            match client.list_prompts(params.clone()).await {
                Ok(result) => {
                    all_prompts.extend(result.prompts);
                }
                Err(e) => {
                    tracing::warn!("Failed to list prompts from client {}: {:?}", name, e);
                }
            }
        }

        Ok(ListPromptsResult {
            prompts: all_prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        // Try to get the prompt from each server until one succeeds
        let mut last_error = None;

        let clients = self.pool.get_clients().await;
        for (name, client) in clients.iter() {
            match client.get_prompt(params.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    tracing::debug!("Prompt {} not found on server {}", params.name, name);
                }
            }
        }

        Err(match last_error {
            Some(ServiceError::McpError(e)) => e,
            _ => ErrorData::resource_not_found(
                format!("Prompt {} not found on any server", params.name),
                None,
            ),
        })
    }

    async fn list_resources(
        &self,
        params: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let mut all_resources = Vec::new();

        // Aggregate resources from all MCP servers
        let clients = self.pool.get_clients().await;
        for (name, client) in clients.iter() {
            match client.list_resources(params.clone()).await {
                Ok(result) => {
                    all_resources.extend(result.resources);
                }
                Err(e) => {
                    tracing::warn!("Failed to list resources from client {}: {:?}", name, e);
                }
            }
        }

        Ok(ListResourcesResult {
            resources: all_resources,
            next_cursor: None,
        })
    }

    async fn list_resource_templates(
        &self,
        params: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        let mut all_templates = Vec::new();

        // Aggregate resource templates from all MCP servers
        let clients = self.pool.get_clients().await;
        for (name, client) in clients.iter() {
            match client.list_resource_templates(params.clone()).await {
                Ok(result) => {
                    all_templates.extend(result.resource_templates);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to list resource templates from client {}: {:?}",
                        name,
                        e
                    );
                }
            }
        }

        Ok(ListResourceTemplatesResult {
            resource_templates: all_templates,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        // Try to read the resource from each server until one succeeds
        let mut last_error = None;

        let clients = self.pool.get_clients().await;
        for (name, client) in clients.iter() {
            match client.read_resource(params.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    tracing::debug!("Resource {} not found on server {}", params.uri, name);
                }
            }
        }

        Err(match last_error {
            Some(ServiceError::McpError(e)) => e,
            _ => ErrorData::resource_not_found(
                format!("Resource {} not found on any server", params.uri),
                None,
            ),
        })
    }
}
