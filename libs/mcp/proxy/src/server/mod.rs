use rmcp::model::ServerCapabilities;
use rmcp::service::{NotificationContext, Peer, RequestContext};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    RoleClient, RoleServer, ServerHandler, ServiceError,
    model::{
        CallToolRequestParam, CallToolResult, CancelledNotificationParam, Content, ErrorData,
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
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::secret_manager::{SecretManagerHandle, launch_secret_manager};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::broadcast::Receiver;

/// Helper to convert ServiceError to ErrorData with context
fn service_error_to_error_data(e: ServiceError, context: &str) -> ErrorData {
    match e {
        ServiceError::McpError(err) => err,
        ServiceError::Cancelled { reason } => ErrorData::internal_error(
            format!(
                "{}: cancelled - {}",
                context,
                reason.unwrap_or_else(|| "unknown reason".to_string())
            ),
            None,
        ),
        _ => ErrorData::internal_error(context.to_string(), None),
    }
}

pub struct ProxyServer {
    pool: Arc<ClientPool>,
    // Map downstream request IDs to upstream client names
    request_id_to_client: Arc<Mutex<HashMap<RequestId, String>>>,
    // Configuration for upstream clients
    client_config: Arc<Mutex<Option<ClientPoolConfig>>>,
    // Track if upstream clients have been initialized
    clients_initialized: Arc<Mutex<bool>>,
    // Secret manager for redacting secrets in tool responses
    secret_manager: Arc<SecretManagerHandle>,
}

impl ProxyServer {
    pub fn new(config: ClientPoolConfig, redact_secrets: bool, privacy_mode: bool) -> Self {
        let secret_manager_handle = launch_secret_manager(redact_secrets, privacy_mode, None);

        Self {
            pool: Arc::new(ClientPool::new()),
            request_id_to_client: Arc::new(Mutex::new(HashMap::new())),
            client_config: Arc::new(Mutex::new(Some(config))),
            clients_initialized: Arc::new(Mutex::new(false)),
            secret_manager: secret_manager_handle,
        }
    }

    /// Set the configuration for upstream clients
    pub async fn set_client_config(&self, config: ClientPoolConfig) {
        let mut stored_config = self.client_config.lock().await;
        *stored_config = Some(config);
    }

    /// Track a request ID to client mapping for cancellation forwarding
    async fn track_request(&self, request_id: RequestId, client_name: String) {
        self.request_id_to_client
            .lock()
            .await
            .insert(request_id, client_name);
    }

    /// Remove and return the client name for a request ID
    async fn untrack_request(&self, request_id: &RequestId) -> Option<String> {
        self.request_id_to_client.lock().await.remove(request_id)
    }

    /// Aggregate results from all clients using a provided async operation.
    /// Collects successful results and logs failures.
    async fn aggregate_from_clients<T, F, Fut>(&self, operation_name: &str, operation: F) -> Vec<T>
    where
        F: Fn(String, Peer<RoleClient>) -> Fut,
        Fut: Future<Output = Result<Vec<T>, (String, ServiceError)>>,
    {
        let client_peers = self.pool.get_all_client_peers().await;
        let mut results = Vec::new();

        for (name, peer) in client_peers {
            match operation(name.clone(), peer).await {
                Ok(items) => results.extend(items),
                Err((client_name, e)) => {
                    tracing::warn!(
                        "Failed to {} from client {}: {:?}",
                        operation_name,
                        client_name,
                        e
                    );
                }
            }
        }

        results
    }

    /// Try an operation on each client until one succeeds.
    /// Returns the first successful result or the last error.
    async fn find_in_clients<T, F, Fut>(
        &self,
        resource_type: &str,
        resource_name: &str,
        operation: F,
    ) -> Result<T, ErrorData>
    where
        F: Fn(String, Peer<RoleClient>) -> Fut,
        Fut: Future<Output = Result<T, ServiceError>>,
    {
        let client_peers = self.pool.get_all_client_peers().await;
        let mut last_error = None;

        for (name, peer) in client_peers {
            match operation(name.clone(), peer).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::debug!(
                        "{} {} not found on server {}",
                        resource_type,
                        resource_name,
                        name
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(match last_error {
            Some(ServiceError::McpError(e)) => e,
            _ => ErrorData::resource_not_found(
                format!(
                    "{} {} not found on any server",
                    resource_type, resource_name
                ),
                None,
            ),
        })
    }

    /// Parse tool name in format "client_name__tool_name"
    fn parse_tool_name(full_name: &str) -> Result<(String, String), ErrorData> {
        let parts: Vec<&str> = full_name.splitn(2, "__").collect();
        if parts.len() != 2 {
            return Err(ErrorData::invalid_params(
                format!(
                    "Invalid tool name format: {}. Expected format: client_name__tool_name",
                    full_name
                ),
                None,
            ));
        }
        Ok((parts[0].to_string(), parts[1].to_string()))
    }

    /// Prepare tool parameters, restoring any redacted secrets
    async fn prepare_tool_params(
        &self,
        params: &CallToolRequestParam,
        tool_name: &str,
    ) -> CallToolRequestParam {
        let mut tool_params = params.clone();
        tool_params.name = tool_name.to_string().into();

        if let Some(arguments) = &tool_params.arguments
            && let Ok(arguments_str) = serde_json::to_string(arguments)
        {
            if let Ok(restored) = self
                .secret_manager
                .restore_secrets_in_string(&arguments_str)
                .await
            {
                if let Ok(restored_arguments) = serde_json::from_str(&restored) {
                    tool_params.arguments = Some(restored_arguments);
                }
            }
        }

        tool_params
    }

    /// Execute a tool call with cancellation monitoring
    async fn execute_with_cancellation(
        &self,
        ctx: &RequestContext<RoleServer>,
        client_peer: &Peer<RoleClient>,
        tool_params: CallToolRequestParam,
    ) -> Result<CallToolResult, ServiceError> {
        tokio::select! {
            biased;

            _ = ctx.ct.cancelled() => {
                // Forward cancellation to upstream server
                let _ = client_peer
                    .notify_cancelled(CancelledNotificationParam {
                        request_id: ctx.id.clone(),
                        reason: Some("Request cancelled by downstream client".to_string()),
                    })
                    .await;

                Err(ServiceError::Cancelled {
                    reason: Some("Request cancelled by downstream client".to_string()),
                })
            }

            result = client_peer.call_tool(tool_params) => result
        }
    }

    /// Redact secrets in content items
    async fn redact_content(&self, content: Vec<Content>) -> Vec<Content> {
        let mut result = Vec::with_capacity(content.len());
        for item in content {
            if let Some(text_content) = item.raw.as_text() {
                let redacted = self
                    .secret_manager
                    .redact_and_store_secrets(&text_content.text, None)
                    .await
                    .unwrap_or_else(|_| text_content.text.clone());
                result.push(Content::text(&redacted));
            } else {
                result.push(item);
            }
        }
        result
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
            crate::client::ServerConfig::Http {
                url,
                headers,
                certificate_chain,
            } => {
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

                let mut client_builder = reqwest::Client::builder()
                    .pool_idle_timeout(std::time::Duration::from_secs(90))
                    .pool_max_idle_per_host(10)
                    .tcp_keepalive(std::time::Duration::from_secs(60));

                // Configure mTLS if certificate chain is provided
                if let Some(cert_chain) = certificate_chain.as_ref() {
                    match cert_chain.create_client_config() {
                        Ok(tls_config) => {
                            client_builder = client_builder.use_preconfigured_tls(tls_config);
                        }
                        Err(e) => {
                            tracing::error!("Failed to create TLS config for {}: {:?}", name, e);
                            return;
                        }
                    }
                }

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
                    let pool = self.pool.clone();
                    let peer = Arc::new(Mutex::new(Some(ctx.peer.clone())));

                    // Initialize all clients and wait for them to complete
                    let mut handles = Vec::new();
                    for (name, server_config) in config.servers {
                        let pool_clone = pool.clone();
                        let peer_clone = peer.clone();
                        let handle = tokio::spawn(async move {
                            Self::initialize_single_client(
                                pool_clone,
                                name,
                                server_config,
                                peer_clone,
                            )
                            .await;
                        });
                        handles.push(handle);
                    }

                    // Wait for all clients to initialize
                    for handle in handles {
                        let _ = handle.await;
                    }

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
        let tools = self
            .aggregate_from_clients("list tools", |name, peer| {
                let params = params.clone();
                async move {
                    peer.list_tools(params)
                        .await
                        .map(|result| {
                            result
                                .tools
                                .into_iter()
                                .map(|mut tool| {
                                    // Prefix tool name with client name using double underscore separator
                                    tool.name = format!("{}__{}", name, tool.name).into();
                                    tool
                                })
                                .collect()
                        })
                        .map_err(|e| (name, e))
                }
            })
            .await;

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: Default::default(),
        })
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // Parse the client name from the tool name (format: client_name__tool_name)
        let (client_name, tool_name) = Self::parse_tool_name(&params.name)?;

        // Get a cloned peer for the client (releases lock immediately)
        let client_peer = self
            .pool
            .get_client_peer(&client_name)
            .await
            .ok_or_else(|| {
                ErrorData::resource_not_found(format!("Client {} not found", client_name), None)
            })?;

        // Track request for cancellation forwarding
        self.track_request(ctx.id.clone(), client_name.clone())
            .await;

        // Prepare and execute the tool call
        let tool_params = self.prepare_tool_params(&params, &tool_name).await;
        let result = self
            .execute_with_cancellation(&ctx, &client_peer, tool_params)
            .await;

        // Always clean up request tracking
        self.untrack_request(&ctx.id).await;

        // Process result and redact secrets
        let mut result = result.map_err(|e| {
            service_error_to_error_data(
                e,
                &format!(
                    "Failed to call tool {} on client {}",
                    tool_name, client_name
                ),
            )
        })?;

        result.content = self.redact_content(result.content).await;
        Ok(result)
    }

    async fn list_prompts(
        &self,
        params: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let prompts = self
            .aggregate_from_clients("list prompts", |name, peer| {
                let params = params.clone();
                async move {
                    peer.list_prompts(params)
                        .await
                        .map(|result| result.prompts)
                        .map_err(|e| (name, e))
                }
            })
            .await;

        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
            meta: Default::default(),
        })
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        let name = params.name.clone();
        self.find_in_clients("Prompt", &name, |_, peer| {
            let params = params.clone();
            async move { peer.get_prompt(params).await }
        })
        .await
    }

    async fn list_resources(
        &self,
        params: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let resources = self
            .aggregate_from_clients("list resources", |name, peer| {
                let params = params.clone();
                async move {
                    peer.list_resources(params)
                        .await
                        .map(|result| result.resources)
                        .map_err(|e| (name, e))
                }
            })
            .await;

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: Default::default(),
        })
    }

    async fn list_resource_templates(
        &self,
        params: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        let resource_templates = self
            .aggregate_from_clients("list resource templates", |name, peer| {
                let params = params.clone();
                async move {
                    peer.list_resource_templates(params)
                        .await
                        .map(|result| result.resource_templates)
                        .map_err(|e| (name, e))
                }
            })
            .await;

        Ok(ListResourceTemplatesResult {
            resource_templates,
            next_cursor: None,
            meta: Default::default(),
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let uri = params.uri.to_string();
        self.find_in_clients("Resource", &uri, |_, peer| {
            let params = params.clone();
            async move { peer.read_resource(params).await }
        })
        .await
    }

    async fn on_cancelled(
        &self,
        notification: CancelledNotificationParam,
        _ctx: NotificationContext<RoleServer>,
    ) {
        let request_id = notification.request_id.clone();

        // Atomically get and remove the mapping
        let Some(client_name) = self.untrack_request(&request_id).await else {
            tracing::debug!(
                "Cancellation notification received but no request ID mapping found for: {:?}",
                request_id
            );
            return;
        };

        // Get a cloned peer and forward cancellation
        let Some(client_peer) = self.pool.get_client_peer(&client_name).await else {
            tracing::warn!(
                "Cancellation notification received for unknown client: {}",
                client_name
            );
            return;
        };

        if let Err(e) = client_peer.notify_cancelled(notification).await {
            tracing::warn!(
                "Failed to forward cancellation to upstream server {}: {:?}",
                client_name,
                e
            );
        } else {
            tracing::debug!(
                "Forwarded cancellation for request {:?} to client {}",
                request_id,
                client_name
            );
        }
    }
}

/// Start the proxy server as an HTTPS service with mTLS
pub async fn start_proxy_server(
    config: ClientPoolConfig,
    tcp_listener: TcpListener,
    certificate_chain: Arc<CertificateChain>,
    redact_secrets: bool,
    privacy_mode: bool,
    shutdown_rx: Option<Receiver<()>>,
) -> anyhow::Result<()> {
    let service = StreamableHttpService::new(
        move || {
            Ok(ProxyServer::new(
                config.clone(),
                redact_secrets,
                privacy_mode,
            ))
        },
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);

    let tls_config = certificate_chain.create_server_config()?;
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(tls_config));

    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();

    tokio::spawn(async move {
        if let Some(mut shutdown_rx) = shutdown_rx {
            let _ = shutdown_rx.recv().await;
        } else {
            // Wait for ctrl+c
            let _ = tokio::signal::ctrl_c().await;
        }
        shutdown_handle.graceful_shutdown(None);
    });

    axum_server::from_tcp_rustls(tcp_listener.into_std()?, rustls_config)
        .handle(handle)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}
