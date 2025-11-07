use rmcp::model::ServerCapabilities;
use rmcp::service::{NotificationContext, Peer, RequestContext};
use rmcp::{
    RoleServer, ServerHandler, ServiceError,
    model::{
        CallToolRequestParam, CallToolResult, CancelledNotificationParam, ErrorData,
        GetPromptRequestParam, GetPromptResult, Implementation, InitializeRequestParam,
        InitializeResult, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
        ListToolsResult, PaginatedRequestParam, ProtocolVersion, ReadResourceRequestParam,
        ReadResourceResult, RequestId,
    },
};

use crate::client::{ClientPool, ClientPoolConfig, ProxyClientHandler};
use rmcp::ServiceExt;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;

pub struct ProxyServer {
    pool: Arc<ClientPool>,
    // Map downstream request IDs to upstream client names
    request_id_to_client: Arc<Mutex<HashMap<RequestId, String>>>,
    // Configuration for upstream clients
    client_config: Arc<Mutex<Option<ClientPoolConfig>>>,
    // Track if upstream clients have been initialized
    clients_initialized: Arc<Mutex<bool>>,
}

impl ProxyServer {
    pub fn new(pool: Arc<ClientPool>) -> Self {
        Self {
            pool,
            request_id_to_client: Arc::new(Mutex::new(HashMap::new())),
            client_config: Arc::new(Mutex::new(None)),
            clients_initialized: Arc::new(Mutex::new(false)),
        }
    }

    /// Set the configuration for upstream clients
    pub async fn set_client_config(&self, config: ClientPoolConfig) {
        let mut stored_config = self.client_config.lock().await;
        *stored_config = Some(config);
    }

    /// Initialize a single upstream client from server configuration
    async fn initialize_single_client(
        pool: Arc<ClientPool>,
        name: String,
        server_config: crate::client::ServerConfig,
        downstream_peer: Arc<Mutex<Option<Peer<RoleServer>>>>,
    ) {
        let handler = ProxyClientHandler::new(downstream_peer);

        match server_config {
            crate::client::ServerConfig::Stdio { command, args, env } => {
                let mut cmd = Command::new(&command);
                for arg in args {
                    cmd.arg(arg);
                }
                if let Some(env_vars) = env {
                    cmd.envs(&env_vars);
                }
                let proc = match TokioChildProcess::new(cmd) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Failed to create process for {}: {:?}", name, e);
                        return;
                    }
                };

                match handler.serve(proc).await {
                    Ok(client) => {
                        pool.add_client(name.clone(), client).await;
                        tracing::info!("{} MCP client initialized", name);
                    }
                    Err(e) => {
                        tracing::error!("Failed to start {} MCP client: {:?}", name, e);
                    }
                }
            }
            crate::client::ServerConfig::Http { url, headers } => {
                // Validate TLS usage
                if !url.starts_with("https://") {
                    tracing::warn!(
                        "⚠️  MCP server '{}' is using insecure HTTP connection: {}",
                        name,
                        url
                    );
                    tracing::warn!(
                        "   Consider using HTTPS or pass --allow-insecure-mcp-transport flag"
                    );
                }

                let mut client_builder = reqwest::Client::builder();
                if let Some(headers_map) = headers {
                    let mut header_map = reqwest::header::HeaderMap::new();
                    for (key, value) in headers_map {
                        if let (Ok(header_name), Ok(header_value)) = (
                            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                            reqwest::header::HeaderValue::from_str(&value),
                        ) {
                            header_map.insert(header_name, header_value);
                        } else {
                            tracing::warn!("Invalid header for {}: {} = {}", name, key, value);
                        }
                    }
                    client_builder = client_builder.default_headers(header_map);
                }

                let http_client = match client_builder.build() {
                    Ok(client) => client,
                    Err(e) => {
                        tracing::error!("Failed to build HTTP client for {}: {:?}", name, e);
                        return;
                    }
                };

                let config = StreamableHttpClientTransportConfig::with_uri(url.as_str());
                let transport = StreamableHttpClientTransport::<reqwest::Client>::with_client(
                    http_client,
                    config,
                );
                match handler.serve(transport).await {
                    Ok(client) => {
                        pool.add_client(name.clone(), client).await;
                        tracing::info!("{} MCP client initialized", name);
                    }
                    Err(e) => {
                        tracing::error!("Failed to start {} MCP client: {:?}", name, e);
                    }
                }
            }
        }
    }
}

impl ServerHandler for ProxyServer {
    async fn initialize(
        &self,
        _params: InitializeRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        // Initialize upstream clients if config is available and not already initialized
        {
            let mut initialized = self.clients_initialized.lock().await;
            if !*initialized {
                let config = self.client_config.lock().await.take();
                if let Some(config) = config {
                    // Initialize clients asynchronously (don't wait for completion)
                    let pool = self.pool.clone();
                    let peer = Arc::new(Mutex::new(Some(ctx.peer.clone())));
                    tokio::spawn(async move {
                        for (name, server_config) in config.servers {
                            let pool_clone = pool.clone();
                            let peer_clone = peer.clone();
                            tokio::spawn(async move {
                                Self::initialize_single_client(
                                    pool_clone,
                                    name,
                                    server_config,
                                    peer_clone,
                                )
                                .await;
                            });
                        }
                    });
                    *initialized = true;
                }
            }
        }

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
                        // Using double underscore as separator to support underscores in server names
                        tool.name = format!("{}__{}", name, tool.name).into();
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
        // Parse the client name from the tool name (format: client_name__tool_name)
        // Using double underscore as separator to support underscores in server names
        let parts: Vec<&str> = params.name.splitn(2, "__").collect();

        if parts.len() != 2 {
            return Err(ErrorData::invalid_params(
                format!(
                    "Invalid tool name format: {}. Expected format: client_name__tool_name",
                    params.name
                ),
                None,
            ));
        }

        let client_name = parts[0].to_string();
        let tool_name = parts[1];

        // Find the specific client and call the tool
        let clients = self.pool.get_clients().await;
        match clients.get(&client_name) {
            Some(client) => {
                let mut tool_params = params.clone();
                tool_params.name = tool_name.to_string().into();

                let result = client.call_tool(tool_params).await.map_err(|e| match e {
                    ServiceError::McpError(err) => err,
                    _ => ErrorData::internal_error(
                        format!(
                            "Failed to call tool {} on client {}",
                            tool_name, client_name
                        ),
                        None,
                    ),
                })?;

                Ok(result)
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

    async fn on_cancelled(
        &self,
        notification: CancelledNotificationParam,
        _ctx: NotificationContext<RoleServer>,
    ) {
        // Forward cancellation notification from downstream client to upstream server
        let request_id = notification.request_id.clone();
        let client_name = {
            let mapping = self.request_id_to_client.lock().await;
            mapping.get(&request_id).cloned()
        };

        if let Some(client_name) = client_name {
            let clients = self.pool.get_clients().await;
            if let Some(client) = clients.get(&client_name) {
                // Forward cancellation to upstream server
                let _ = client.notify_cancelled(notification).await;
            } else {
                tracing::warn!(
                    "Cancellation notification received for unknown client: {}",
                    client_name
                );
            }
        } else {
            tracing::debug!(
                "Cancellation notification received but no request ID mapping found for: {:?}",
                request_id
            );
        }
    }
}
