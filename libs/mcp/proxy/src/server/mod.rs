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
use stakpak_shared::secret_manager::SecretManager;
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

/// Single-pass restoration of `[REDACTED_SECRET:...]` placeholders in a string.
///
/// Unlike the iterative `restore_secrets()` helper (which calls `String::replace`
/// for every map entry), this scans forward through the string once, resolving
/// each placeholder from the map as it is encountered.  Because we advance past
/// the *replacement text* without re-scanning it, a secret whose value happens
/// to contain another `[REDACTED_SECRET:...]` token will **not** trigger a
/// chain replacement.
fn restore_secrets_single_pass(s: &str, redaction_map: &HashMap<String, String>) -> String {
    const PREFIX: &str = "[REDACTED_SECRET:";

    if redaction_map.is_empty() {
        return s.to_string();
    }

    let mut result = String::with_capacity(s.len());
    let mut remaining = s;

    while let Some(start) = remaining.find(PREFIX) {
        // Push everything before the placeholder.
        result.push_str(&remaining[..start]);

        // Look for the closing `]`.
        if let Some(rel_end) = remaining[start..].find(']') {
            let key = &remaining[start..start + rel_end + 1];
            if let Some(original) = redaction_map.get(key) {
                result.push_str(original);
            } else {
                // Unknown placeholder — keep it verbatim.
                result.push_str(key);
            }
            remaining = &remaining[start + rel_end + 1..];
        } else {
            // No closing bracket — push from the prefix onward as-is and stop.
            result.push_str(&remaining[start..]);
            return result;
        }
    }

    result.push_str(remaining);
    result
}

/// Recursively restore redacted secrets in a JSON value tree.
///
/// Walks through all string values in the JSON structure and restores
/// any `[REDACTED_SECRET:...]` placeholders to their original values.
/// This avoids the pitfall of serializing to a JSON string, doing raw
/// text replacement (which can break JSON when secret values contain
/// `"`, `\`, or newlines), and parsing back.
///
/// Uses [`restore_secrets_single_pass`] for each string to prevent chain
/// replacement when a secret's value itself contains a redaction placeholder.
fn restore_secrets_in_json_value(
    value: &mut serde_json::Value,
    redaction_map: &HashMap<String, String>,
) {
    match value {
        serde_json::Value::String(s) => {
            let restored = restore_secrets_single_pass(s, redaction_map);
            if restored != *s {
                *s = restored;
            }
        }
        serde_json::Value::Object(map) => {
            for (_k, v) in map.iter_mut() {
                restore_secrets_in_json_value(v, redaction_map);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                restore_secrets_in_json_value(v, redaction_map);
            }
        }
        _ => {}
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
    secret_manager: SecretManager,
}

impl ProxyServer {
    pub fn new(config: ClientPoolConfig, redact_secrets: bool, privacy_mode: bool) -> Self {
        Self {
            pool: Arc::new(ClientPool::new()),
            request_id_to_client: Arc::new(Mutex::new(HashMap::new())),
            client_config: Arc::new(Mutex::new(Some(config))),
            clients_initialized: Arc::new(Mutex::new(false)),
            secret_manager: SecretManager::new(redact_secrets, privacy_mode),
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
    fn prepare_tool_params(
        &self,
        params: &CallToolRequestParam,
        tool_name: &str,
    ) -> CallToolRequestParam {
        let mut tool_params = params.clone();
        tool_params.name = tool_name.to_string().into();

        // Load the redaction map once, then walk the entire JSON value tree.
        let redaction_map = self.secret_manager.load_session_redaction_map();
        if !redaction_map.is_empty()
            && let Some(arguments) = &mut tool_params.arguments
        {
            for (_key, value) in arguments.iter_mut() {
                restore_secrets_in_json_value(value, &redaction_map);
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
    fn redact_content(&self, content: Vec<Content>) -> Vec<Content> {
        content
            .into_iter()
            .map(|item| {
                if let Some(text_content) = item.raw.as_text() {
                    let redacted = self
                        .secret_manager
                        .redact_and_store_secrets(&text_content.text, None);
                    Content::text(&redacted)
                } else {
                    item
                }
            })
            .collect()
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

                // Configure TLS: use mTLS cert chain if provided, otherwise use
                // platform-verified TLS so the OS CA store is trusted (needed for
                // warden container where a custom CA is installed).
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
                } else {
                    // No mTLS cert chain — use platform verifier to trust system CA store
                    let arc_crypto_provider =
                        std::sync::Arc::new(rustls::crypto::ring::default_provider());
                    if let Ok(tls_config) = rustls::ClientConfig::builder_with_provider(
                        arc_crypto_provider,
                    )
                    .with_safe_default_protocol_versions()
                    .map(|builder| {
                        rustls_platform_verifier::BuilderVerifierExt::with_platform_verifier(
                            builder,
                        )
                        .with_no_client_auth()
                    }) {
                        client_builder = client_builder.use_preconfigured_tls(tls_config);
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
        let tool_params = self.prepare_tool_params(&params, &tool_name);
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

        result.content = self.redact_content(result.content);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: build a redaction map from pairs
    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // ---------------------------------------------------------------
    // restore_secrets_in_json_value — basic string restoration
    // ---------------------------------------------------------------

    #[test]
    fn test_restore_simple_string() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:abc]", "s3cret")]);
        let mut value = json!("password is [REDACTED_SECRET:pw:abc]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("password is s3cret"));
    }

    #[test]
    fn test_restore_no_placeholder() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:abc]", "s3cret")]);
        let mut value = json!("nothing to replace here");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("nothing to replace here"));
    }

    #[test]
    fn test_restore_empty_map() {
        let redaction_map = HashMap::new();
        let mut value = json!("[REDACTED_SECRET:pw:abc] stays");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("[REDACTED_SECRET:pw:abc] stays"));
    }

    #[test]
    fn test_restore_multiple_placeholders_same_string() {
        let redaction_map = map(&[
            ("[REDACTED_SECRET:a:1]", "alpha"),
            ("[REDACTED_SECRET:b:2]", "beta"),
        ]);
        let mut value = json!("[REDACTED_SECRET:a:1] and [REDACTED_SECRET:b:2]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("alpha and beta"));
    }

    #[test]
    fn test_restore_repeated_placeholder() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:x]", "pass")]);
        let mut value = json!("[REDACTED_SECRET:pw:x]words and [REDACTED_SECRET:pw:x]words");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("passwords and passwords"));
    }

    // ---------------------------------------------------------------
    // restore_secrets_in_json_value — nested objects
    // ---------------------------------------------------------------

    #[test]
    fn test_restore_flat_object() {
        let redaction_map = map(&[("[REDACTED_SECRET:key:1]", "actual_key")]);
        let mut value = json!({
            "path": "README.md",
            "old_str": "key=[REDACTED_SECRET:key:1]",
            "new_str": "key=new_value"
        });
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value["old_str"], json!("key=actual_key"));
        // Untouched fields stay the same
        assert_eq!(value["path"], json!("README.md"));
        assert_eq!(value["new_str"], json!("key=new_value"));
    }

    #[test]
    fn test_restore_nested_object() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:z]", "secret123")]);
        let mut value = json!({
            "level1": {
                "level2": {
                    "password": "[REDACTED_SECRET:pw:z]"
                }
            }
        });
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value["level1"]["level2"]["password"], json!("secret123"));
    }

    // ---------------------------------------------------------------
    // restore_secrets_in_json_value — arrays
    // ---------------------------------------------------------------

    #[test]
    fn test_restore_array_of_strings() {
        let redaction_map = map(&[("[REDACTED_SECRET:t:1]", "token_abc")]);
        let mut value = json!(["no secret", "[REDACTED_SECRET:t:1]", "also clean"]);
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!(["no secret", "token_abc", "also clean"]));
    }

    #[test]
    fn test_restore_array_of_objects() {
        let redaction_map = map(&[
            ("[REDACTED_SECRET:a:1]", "val_a"),
            ("[REDACTED_SECRET:b:2]", "val_b"),
        ]);
        let mut value = json!([
            {"key": "[REDACTED_SECRET:a:1]"},
            {"key": "[REDACTED_SECRET:b:2]"}
        ]);
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value[0]["key"], json!("val_a"));
        assert_eq!(value[1]["key"], json!("val_b"));
    }

    #[test]
    fn test_restore_nested_arrays() {
        let redaction_map = map(&[("[REDACTED_SECRET:x:1]", "found")]);
        let mut value = json!([["a", "[REDACTED_SECRET:x:1]"], ["b"]]);
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!([["a", "found"], ["b"]]));
    }

    // ---------------------------------------------------------------
    // restore_secrets_in_json_value — non-string types unchanged
    // ---------------------------------------------------------------

    #[test]
    fn test_restore_number_unchanged() {
        let redaction_map = map(&[("[REDACTED_SECRET:x:1]", "val")]);
        let mut value = json!(42);
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!(42));
    }

    #[test]
    fn test_restore_bool_unchanged() {
        let redaction_map = map(&[("[REDACTED_SECRET:x:1]", "val")]);
        let mut value = json!(true);
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!(true));
    }

    #[test]
    fn test_restore_null_unchanged() {
        let redaction_map = map(&[("[REDACTED_SECRET:x:1]", "val")]);
        let mut value = json!(null);
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!(null));
    }

    #[test]
    fn test_restore_mixed_types_in_object() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:1]", "secret")]);
        let mut value = json!({
            "string_field": "has [REDACTED_SECRET:pw:1]",
            "number_field": 123,
            "bool_field": false,
            "null_field": null,
            "array_field": [1, "[REDACTED_SECRET:pw:1]", true]
        });
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value["string_field"], json!("has secret"));
        assert_eq!(value["number_field"], json!(123));
        assert_eq!(value["bool_field"], json!(false));
        assert_eq!(value["null_field"], json!(null));
        assert_eq!(value["array_field"], json!([1, "secret", true]));
    }

    // ---------------------------------------------------------------
    // restore_secrets_in_json_value — secret values with JSON-special chars
    // This is the key bug the new approach fixes: secrets containing
    // `"`, `\`, or newlines would break the old serialize→replace→parse path.
    // ---------------------------------------------------------------

    #[test]
    fn test_restore_secret_with_double_quote() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:q]", "pass\"word")]);
        let mut value = json!("auth=[REDACTED_SECRET:pw:q]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("auth=pass\"word"));
    }

    #[test]
    fn test_restore_secret_with_backslash() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:b]", "C:\\Users\\admin")]);
        let mut value = json!("path=[REDACTED_SECRET:pw:b]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("path=C:\\Users\\admin"));
    }

    #[test]
    fn test_restore_secret_with_newline() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:n]", "line1\nline2")]);
        let mut value = json!("content=[REDACTED_SECRET:pw:n]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("content=line1\nline2"));
    }

    #[test]
    fn test_restore_secret_with_tab() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:t]", "col1\tcol2")]);
        let mut value = json!("[REDACTED_SECRET:pw:t]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("col1\tcol2"));
    }

    #[test]
    fn test_restore_secret_with_all_special_chars() {
        let secret = "p@ss\"\n\\word\t{end}";
        let redaction_map = map(&[("[REDACTED_SECRET:pw:all]", secret)]);
        let mut value = json!({
            "old_str": "before [REDACTED_SECRET:pw:all] after",
            "new_str": "replacement"
        });
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value["old_str"], json!(format!("before {} after", secret)));
    }

    // ---------------------------------------------------------------
    // restore_secrets_in_json_value — realistic str_replace scenario
    // ---------------------------------------------------------------

    #[test]
    fn test_restore_str_replace_tool_call() {
        let redaction_map = map(&[
            ("[REDACTED_SECRET:url-embedded-passwords:2f6lt3]", "pass"),
            (
                "[REDACTED_SECRET:generic-api-key:abc123]",
                "sk-ant-secret-key-value",
            ),
        ]);

        let mut value = json!({
            "path": "README.md",
            "old_str": "Generate cryptographically secure [REDACTED_SECRET:url-embedded-passwords:2f6lt3]words with configurable complexity",
            "new_str": "Generate strong passwords"
        });

        restore_secrets_in_json_value(&mut value, &redaction_map);

        assert_eq!(
            value["old_str"],
            json!("Generate cryptographically secure passwords with configurable complexity")
        );
        assert_eq!(value["new_str"], json!("Generate strong passwords"));
        assert_eq!(value["path"], json!("README.md"));
    }

    #[test]
    fn test_restore_run_command_tool_call() {
        let redaction_map = map(&[("[REDACTED_SECRET:generic-api-key:k1]", "sk-live-abc123")]);

        let mut value = json!({
            "command": "curl -H 'Authorization: Bearer [REDACTED_SECRET:generic-api-key:k1]' https://api.example.com",
            "description": "Test API call"
        });

        restore_secrets_in_json_value(&mut value, &redaction_map);

        assert_eq!(
            value["command"],
            json!("curl -H 'Authorization: Bearer sk-live-abc123' https://api.example.com")
        );
    }

    // ---------------------------------------------------------------
    // restore_secrets_in_json_value — edge cases
    // ---------------------------------------------------------------

    #[test]
    fn test_restore_empty_string() {
        let redaction_map = map(&[("[REDACTED_SECRET:x:1]", "val")]);
        let mut value = json!("");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!(""));
    }

    #[test]
    fn test_restore_placeholder_is_entire_string() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:full]", "the_whole_secret")]);
        let mut value = json!("[REDACTED_SECRET:pw:full]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!("the_whole_secret"));
    }

    #[test]
    fn test_restore_deeply_nested() {
        let redaction_map = map(&[("[REDACTED_SECRET:d:1]", "deep_val")]);
        let mut value = json!({
            "a": {
                "b": {
                    "c": {
                        "d": {
                            "e": "[REDACTED_SECRET:d:1]"
                        }
                    }
                }
            }
        });
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value["a"]["b"]["c"]["d"]["e"], json!("deep_val"));
    }

    #[test]
    fn test_restore_large_redaction_map() {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for i in 0..100 {
            pairs.push((
                format!("[REDACTED_SECRET:rule:{}]", i),
                format!("secret_value_{}", i),
            ));
        }
        let redaction_map: HashMap<String, String> = pairs.iter().cloned().collect();

        let mut value = json!({
            "field0": "has [REDACTED_SECRET:rule:0]",
            "field50": "has [REDACTED_SECRET:rule:50]",
            "field99": "has [REDACTED_SECRET:rule:99]",
            "clean": "no secrets here"
        });

        restore_secrets_in_json_value(&mut value, &redaction_map);

        assert_eq!(value["field0"], json!("has secret_value_0"));
        assert_eq!(value["field50"], json!("has secret_value_50"));
        assert_eq!(value["field99"], json!("has secret_value_99"));
        assert_eq!(value["clean"], json!("no secrets here"));
    }

    #[test]
    fn test_restore_secret_value_looks_like_placeholder() {
        // Edge case: a secret value itself looks like a redaction placeholder
        let redaction_map = map(&[("[REDACTED_SECRET:outer:1]", "[REDACTED_SECRET:inner:2]")]);
        let mut value = json!("contains [REDACTED_SECRET:outer:1]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        // Should restore to the literal string (no recursive resolution)
        assert_eq!(value, json!("contains [REDACTED_SECRET:inner:2]"));
    }

    #[test]
    fn test_restore_no_chain_replacement_both_keys_present() {
        // Both outer and inner are valid keys in the map.
        // Outer's value contains inner's key — single-pass must NOT chain.
        let redaction_map = map(&[
            ("[REDACTED_SECRET:outer:1]", "[REDACTED_SECRET:inner:2]"),
            ("[REDACTED_SECRET:inner:2]", "final_secret"),
        ]);
        let mut value = json!("[REDACTED_SECRET:outer:1]");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        // Must stop at the first restoration, not chain into "final_secret"
        assert_eq!(value, json!("[REDACTED_SECRET:inner:2]"));
    }

    #[test]
    fn test_restore_no_chain_replacement_in_object() {
        let redaction_map = map(&[
            ("[REDACTED_SECRET:a:1]", "text [REDACTED_SECRET:b:2] text"),
            ("[REDACTED_SECRET:b:2]", "chained"),
        ]);
        let mut value = json!({
            "field": "before [REDACTED_SECRET:a:1] after"
        });
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(
            value["field"],
            json!("before text [REDACTED_SECRET:b:2] text after")
        );
    }

    #[test]
    fn test_restore_partial_placeholder_not_matched() {
        let redaction_map = map(&[("[REDACTED_SECRET:pw:abc]", "secret")]);
        let mut value = json!("partial [REDACTED_SECRET:pw:ab is not replaced");
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(
            value,
            json!("partial [REDACTED_SECRET:pw:ab is not replaced")
        );
    }

    // ---------------------------------------------------------------
    // restore_secrets_single_pass — unit tests
    // ---------------------------------------------------------------

    #[test]
    fn test_single_pass_basic() {
        let map = map(&[("[REDACTED_SECRET:pw:1]", "secret")]);
        assert_eq!(
            restore_secrets_single_pass("auth=[REDACTED_SECRET:pw:1]", &map),
            "auth=secret"
        );
    }

    #[test]
    fn test_single_pass_no_placeholder() {
        let map = map(&[("[REDACTED_SECRET:pw:1]", "secret")]);
        assert_eq!(
            restore_secrets_single_pass("no placeholders here", &map),
            "no placeholders here"
        );
    }

    #[test]
    fn test_single_pass_empty_map() {
        let map = HashMap::new();
        assert_eq!(
            restore_secrets_single_pass("[REDACTED_SECRET:pw:1] stays", &map),
            "[REDACTED_SECRET:pw:1] stays"
        );
    }

    #[test]
    fn test_single_pass_unknown_key_preserved() {
        let map = map(&[("[REDACTED_SECRET:pw:1]", "secret")]);
        assert_eq!(
            restore_secrets_single_pass("[REDACTED_SECRET:unknown:99]", &map),
            "[REDACTED_SECRET:unknown:99]"
        );
    }

    #[test]
    fn test_single_pass_multiple_placeholders() {
        let map = map(&[
            ("[REDACTED_SECRET:a:1]", "alpha"),
            ("[REDACTED_SECRET:b:2]", "beta"),
        ]);
        assert_eq!(
            restore_secrets_single_pass("[REDACTED_SECRET:a:1] and [REDACTED_SECRET:b:2]", &map),
            "alpha and beta"
        );
    }

    #[test]
    fn test_single_pass_no_closing_bracket() {
        let map = map(&[("[REDACTED_SECRET:pw:1]", "secret")]);
        assert_eq!(
            restore_secrets_single_pass("broken [REDACTED_SECRET:pw:1 missing bracket", &map),
            "broken [REDACTED_SECRET:pw:1 missing bracket"
        );
    }

    #[test]
    fn test_single_pass_no_chain() {
        let map = map(&[
            ("[REDACTED_SECRET:a:1]", "value has [REDACTED_SECRET:b:2]"),
            ("[REDACTED_SECRET:b:2]", "should not appear"),
        ]);
        assert_eq!(
            restore_secrets_single_pass("[REDACTED_SECRET:a:1]", &map),
            "value has [REDACTED_SECRET:b:2]"
        );
    }

    #[test]
    fn test_single_pass_adjacent_placeholders() {
        let map = map(&[
            ("[REDACTED_SECRET:a:1]", "X"),
            ("[REDACTED_SECRET:b:2]", "Y"),
        ]);
        assert_eq!(
            restore_secrets_single_pass("[REDACTED_SECRET:a:1][REDACTED_SECRET:b:2]", &map),
            "XY"
        );
    }

    #[test]
    fn test_restore_empty_object() {
        let redaction_map = map(&[("[REDACTED_SECRET:x:1]", "val")]);
        let mut value = json!({});
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!({}));
    }

    #[test]
    fn test_restore_empty_array() {
        let redaction_map = map(&[("[REDACTED_SECRET:x:1]", "val")]);
        let mut value = json!([]);
        restore_secrets_in_json_value(&mut value, &redaction_map);
        assert_eq!(value, json!([]));
    }
}
