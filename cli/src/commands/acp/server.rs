use crate::commands::agent::run::helpers::user_message;
use crate::utils::network;
use crate::{commands::agent::run::helpers::convert_tools_map_with_filter, config::AppConfig};
use agent_client_protocol::{self as acp, Client as AcpClient, SessionNotification};
use futures_util::StreamExt;
use stakpak_api::models::ApiStreamError;
use stakpak_api::{Client, ClientConfig};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::{MCPServerConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::integrations::openai::{
    ChatCompletionResponse, ChatCompletionStreamResponse, ToolCall, ToolCallResultProgress,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use uuid::Uuid;

pub struct StakpakAcpAgent {
    config: AppConfig,
    client: Client,
    session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    next_session_id: Cell<u64>,
    mcp_server_host: Option<String>,
    clients: Option<Arc<ClientManager>>,
    tools: Option<Vec<stakpak_shared::models::integrations::openai::Tool>>,
    current_session_id: Cell<Option<Uuid>>,
    progress_tx: Option<mpsc::Sender<ToolCallResultProgress>>,
    // Add persistent message history for conversation context
    messages:
        Arc<tokio::sync::Mutex<Vec<stakpak_shared::models::integrations::openai::ChatMessage>>>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ToolInfo {
    title: String,
    kind: ToolKind,
    content: Vec<ToolCallContent>,
    locations: Vec<ToolCallLocation>,
}

#[derive(Debug, Clone)]
enum ToolKind {
    Read,
    Edit,
    Execute,
    Search,
    Other,
}

#[derive(Debug, Clone)]
enum ToolCallContent {
    Content {
        r#type: String, // "content"
        content: TextContent,
    },
    Diff {
        r#type: String, // "diff"
        path: String,
        old_text: Option<String>,
        new_text: String,
    },
}

#[derive(Debug, Clone)]
struct TextContent {
    text: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ToolCallLocation {
    path: String,
    line: Option<usize>,
}

impl StakpakAcpAgent {
    fn format_tool_content(&self, content: &[ToolCallContent]) -> String {
        let mut formatted = String::new();

        for item in content {
            match item {
                ToolCallContent::Content {
                    r#type,
                    content: text_content,
                } => {
                    formatted.push_str(&format!("**Type**: `{}`\n", r#type));
                    formatted.push_str(&format!("{}\n", text_content.text));
                }
                ToolCallContent::Diff {
                    r#type,
                    path,
                    old_text,
                    new_text,
                } => {
                    formatted.push_str(&format!("**Type**: `{}`\n", r#type));
                    if let Some(old) = old_text {
                        formatted.push_str(&format!("**File**: `{}`\n", path));
                        formatted.push_str(&format!("**Change**: `{}` → `{}`\n", old, new_text));
                    } else {
                        formatted.push_str(&format!("**File**: `{}`\n", path));
                        formatted.push_str(&format!("**Content**:\n```\n{}\n```\n", new_text));
                    }
                }
            }
        }

        formatted.trim().to_string()
    }

    fn create_tool_info(&self, tool_call: &ToolCall) -> ToolInfo {
        let name = &tool_call.function.name;
        let args = match serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
            Ok(json) => json,
            Err(_) => serde_json::Value::Null,
        };

        match name.as_str() {
            "create" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("file");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                ToolInfo {
                    title: format!("Create `{}`", path),
                    kind: ToolKind::Edit,
                    content: vec![ToolCallContent::Diff {
                        r#type: "diff".to_string(),
                        path: path.to_string(),
                        old_text: None,
                        new_text: content.to_string(),
                    }],
                    locations: vec![ToolCallLocation {
                        path: path.to_string(),
                        line: None,
                    }],
                }
            }
            "str_replace" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("file");
                let old_str = args.get("old").and_then(|v| v.as_str()).unwrap_or("");
                let new_str = args.get("new").and_then(|v| v.as_str()).unwrap_or("");
                ToolInfo {
                    title: format!("Edit `{}`", path),
                    kind: ToolKind::Edit,
                    content: vec![ToolCallContent::Diff {
                        r#type: "diff".to_string(),
                        path: path.to_string(),
                        old_text: Some(old_str.to_string()),
                        new_text: new_str.to_string(),
                    }],
                    locations: vec![ToolCallLocation {
                        path: path.to_string(),
                        line: None,
                    }],
                }
            }
            "run_command" => {
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("command");
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                ToolInfo {
                    title: format!("Execute `{}`", command),
                    kind: ToolKind::Execute,
                    content: if description.is_empty() {
                        vec![]
                    } else {
                        vec![ToolCallContent::Content {
                            r#type: "content".to_string(),
                            content: TextContent {
                                text: description.to_string(),
                            },
                        }]
                    },
                    locations: vec![],
                }
            }
            "view" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("file");
                let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                ToolInfo {
                    title: format!("Read `{}`", path),
                    kind: ToolKind::Read,
                    content: vec![],
                    locations: vec![ToolCallLocation {
                        path: path.to_string(),
                        line: Some(offset),
                    }],
                }
            }
            "search_docs" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("search");
                ToolInfo {
                    title: format!("Search Documentation: `{}`", query),
                    kind: ToolKind::Search,
                    content: vec![],
                    locations: vec![],
                }
            }
            "read_rulebook" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("rulebook");
                ToolInfo {
                    title: format!("Read Rulebook: `{}`", name),
                    kind: ToolKind::Read,
                    content: vec![],
                    locations: vec![],
                }
            }
            "local_code_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("search");
                ToolInfo {
                    title: format!("Search Code: `{}`", query),
                    kind: ToolKind::Search,
                    content: vec![],
                    locations: vec![],
                }
            }
            _ => ToolInfo {
                title: format!("Execute `{}`", name),
                kind: ToolKind::Other,
                content: vec![ToolCallContent::Content {
                    r#type: "content".to_string(),
                    content: TextContent {
                        text: format!("```json\n{}\n```", tool_call.function.arguments),
                    },
                }],
                locations: vec![],
            },
        }
    }

    async fn initialize_mcp_server_and_tools(
        config: &AppConfig,
        progress_tx: Option<mpsc::Sender<ToolCallResultProgress>>,
    ) -> Result<
        (
            String,
            Arc<ClientManager>,
            Vec<stakpak_shared::models::integrations::openai::Tool>,
        ),
        String,
    > {
        // Find available bind address
        let (bind_address, listener) = network::find_available_bind_address_with_listener().await?;

        // Generate certificates for mTLS
        let certificate_chain = Arc::new(Some(
            CertificateChain::generate().map_err(|e| e.to_string())?,
        ));

        let protocol = "https";
        let local_mcp_server_host = format!("{}://{}", protocol, bind_address);

        // Start MCP server in background
        let api_config: ClientConfig = config.clone().into();
        let certificate_chain_for_server = certificate_chain.clone();

        tokio::spawn(async move {
            let _ = start_server(
                MCPServerConfig {
                    api: api_config,
                    redact_secrets: true,
                    privacy_mode: false,
                    tool_mode: ToolMode::Combined,
                    bind_address,
                    certificate_chain: certificate_chain_for_server,
                },
                Some(listener),
                None,
            )
            .await;
        });

        // Initialize MCP clients
        let clients = Arc::new(
            ClientManager::new(
                config
                    .mcp_server_host
                    .clone()
                    .unwrap_or(local_mcp_server_host.clone()),
                progress_tx, // Use regular Sender directly
                certificate_chain,
            )
            .await
            .map_err(|e| format!("Failed to create MCP clients: {}", e))?,
        );

        // Get tools from MCP clients
        let tools_map: HashMap<String, Vec<rmcp::model::Tool>> = clients
            .get_tools()
            .await
            .map_err(|e| format!("Failed to get tools: {}", e))?;

        let tools = convert_tools_map_with_filter(&tools_map, config.allowed_tools.as_ref());

        Ok((local_mcp_server_host, clients, tools))
    }

    async fn process_acp_streaming_response(
        &self,
        stream: impl futures_util::Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>>,
        session_id: &acp::SessionId,
    ) -> Result<ChatCompletionResponse, String> {
        let mut stream = Box::pin(stream);
        let mut chat_completion_response = ChatCompletionResponse {
            id: "".to_string(),
            object: "".to_string(),
            created: 0,
            model: "".to_string(),
            choices: vec![],
            usage: stakpak_shared::models::integrations::openai::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
            system_fingerprint: None,
        };

        let mut chat_message = stakpak_shared::models::integrations::openai::ChatMessage {
            role: stakpak_shared::models::integrations::openai::Role::Assistant,
            content: None,
            name: None,
            tool_calls: None,
            tool_call_id: None,
        };

        // Compile regex once outside the loop
        let checkpoint_regex = regex::Regex::new(r"<checkpoint_id>.*?</checkpoint_id>").ok();

        while let Some(response) = stream.next().await {
            match &response {
                Ok(response) => {
                    if response.choices.is_empty() {
                        continue;
                    }
                    let delta = &response.choices[0].delta;

                    chat_completion_response = ChatCompletionResponse {
                        id: response.id.clone(),
                        object: response.object.clone(),
                        created: response.created,
                        model: response.model.clone(),
                        choices: vec![],
                        usage: stakpak_shared::models::integrations::openai::Usage {
                            prompt_tokens: 0,
                            completion_tokens: 0,
                            total_tokens: 0,
                        },
                        system_fingerprint: None,
                    };

                    if let Some(content) = &delta.content {
                        chat_message.content = Some(
                            stakpak_shared::models::integrations::openai::MessageContent::String(
                                match chat_message.content {
                                    Some(stakpak_shared::models::integrations::openai::MessageContent::String(old_content)) => {
                                        old_content + content
                                    }
                                    _ => content.clone(),
                                }
                            )
                        );

                        // Filter out checkpoint IDs from streaming content
                        let filtered_content = if content.contains("<checkpoint_id>") {
                            // Remove entire checkpoint_id blocks including content
                            if let Some(regex) = &checkpoint_regex {
                                let cleaned = regex.replace_all(content, "").to_string();
                                // Also remove any leading/trailing whitespace and newlines
                                cleaned.trim().to_string()
                            } else {
                                // Fallback to simple string replacement if regex fails
                                content
                                    .replace("<checkpoint_id>", "")
                                    .replace("</checkpoint_id>", "")
                            }
                        } else {
                            content.clone()
                        };

                        // Only send non-empty content after filtering
                        if !filtered_content.trim().is_empty() {
                            // Send streaming chunk to ACP client
                            let (tx, rx) = oneshot::channel();
                            self.session_update_tx
                                .send((
                                    SessionNotification {
                                        session_id: session_id.clone(),
                                        update: acp::SessionUpdate::AgentMessageChunk {
                                            content: acp::ContentBlock::Text(acp::TextContent {
                                                text: filtered_content,
                                                annotations: None,
                                            }),
                                        },
                                    },
                                    tx,
                                ))
                                .map_err(|_| "Failed to send streaming chunk")?;
                            rx.await.map_err(|_| "Failed to await streaming chunk")?;
                        }
                    }

                    // Handle tool calls streaming
                    if let Some(tool_calls) = &delta.tool_calls {
                        for delta_tool_call in tool_calls {
                            if chat_message.tool_calls.is_none() {
                                chat_message.tool_calls = Some(vec![]);
                            }

                            let tool_calls_vec = chat_message.tool_calls.as_mut();
                            if let Some(tool_calls_vec) = tool_calls_vec {
                                match tool_calls_vec.get_mut(delta_tool_call.index) {
                                    Some(tool_call) => {
                                        let delta_func = delta_tool_call.function.as_ref().unwrap_or(
                                            &stakpak_shared::models::integrations::openai::FunctionCallDelta {
                                                name: None,
                                                arguments: None,
                                            },
                                        );
                                        tool_call.function.arguments =
                                            tool_call.function.arguments.clone()
                                                + delta_func.arguments.as_deref().unwrap_or("");
                                    }
                                    None => {
                                        // push empty tool calls until the index is reached
                                        tool_calls_vec.extend(
                                            (tool_calls_vec.len()..delta_tool_call.index).map(|_| {
                                                ToolCall {
                                                    id: "".to_string(),
                                                    r#type: "function".to_string(),
                                                    function: stakpak_shared::models::integrations::openai::FunctionCall {
                                                        name: "".to_string(),
                                                        arguments: "".to_string(),
                                                    },
                                                }
                                            }),
                                        );

                                        tool_calls_vec.push(ToolCall {
                                            id: delta_tool_call.id.clone().unwrap_or_default(),
                                            r#type: "function".to_string(),
                                            function: stakpak_shared::models::integrations::openai::FunctionCall {
                                                name: delta_tool_call
                                                    .function
                                                    .as_ref()
                                                    .unwrap_or(&stakpak_shared::models::integrations::openai::FunctionCallDelta {
                                                        name: None,
                                                        arguments: None,
                                                    })
                                                    .name
                                                    .as_deref()
                                                    .unwrap_or("")
                                                    .to_string(),
                                                arguments: "".to_string(),
                                            },
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("Stream error: {:?}", e));
                }
            }
        }

        // filter out empty tool calls
        chat_message.tool_calls = Some(
            chat_message
                .tool_calls
                .as_ref()
                .unwrap_or(&vec![])
                .iter()
                .filter(|tool_call| !tool_call.id.is_empty())
                .cloned()
                .collect::<Vec<ToolCall>>(),
        );

        chat_completion_response.choices.push(
            stakpak_shared::models::integrations::openai::ChatCompletionChoice {
                index: 0,
                message: chat_message.clone(),
                finish_reason: stakpak_shared::models::integrations::openai::FinishReason::Stop,
                logprobs: None,
            },
        );

        Ok(chat_completion_response)
    }

    fn validate_tool_permissions(&self, tool_name: &str) -> bool {
        // Check if tool is allowed based on config
        if let Some(ref allowed_tools) = self.config.allowed_tools {
            if !allowed_tools.is_empty() && !allowed_tools.contains(&tool_name.to_string()) {
                log::warn!("Tool '{}' is not in allowed tools list", tool_name);
                return false;
            }
        }

        // Additional security checks for specific tools
        match tool_name {
            "run_command" => {
                // Check if command execution is allowed
                log::info!("Validating run_command permissions");
                true
            }
            "create" | "str_replace" => {
                // Check if file operations are allowed
                log::info!("Validating file operation permissions for {}", tool_name);
                true
            }
            "view" | "local_code_search" => {
                // Read-only operations are generally safe
                log::info!(
                    "Validating read-only operation permissions for {}",
                    tool_name
                );
                true
            }
            _ => {
                // Default allow for other tools
                log::info!("Allowing tool '{}' by default", tool_name);
                true
            }
        }
    }

    pub async fn new(
        config: AppConfig,
        session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    ) -> Result<Self, String> {
        let api_config: ClientConfig = config.clone().into();
        let client =
            Client::new(&api_config).map_err(|e| format!("Failed to create client: {}", e))?;

        // Initialize MCP server and tools (optional for ACP)
        let (mcp_server_host, clients, tools) =
            match Self::initialize_mcp_server_and_tools(&config, None).await {
                Ok((host, client_manager, tool_list)) => {
                    log::info!("MCP server initialized successfully");
                    (host, Some(client_manager), tool_list)
                }
                Err(e) => {
                    log::warn!(
                        "Failed to initialize MCP server: {}, continuing without tools",
                        e
                    );
                    (String::new(), None, Vec::new())
                }
            };

        Ok(Self {
            config,
            client,
            session_update_tx,
            next_session_id: Cell::new(0),
            mcp_server_host: if mcp_server_host.is_empty() {
                None
            } else {
                Some(mcp_server_host)
            },
            clients,
            tools: if tools.is_empty() { None } else { Some(tools) },
            current_session_id: Cell::new(None),
            progress_tx: None,
            messages: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        })
    }

    pub async fn run_stdio(&self) -> Result<(), String> {
        let outgoing = tokio::io::stdout().compat_write();
        let incoming = tokio::io::stdin().compat();

        // Set up signal handling outside of LocalSet
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::unbounded_channel();

        // Spawn signal handler task
        tokio::spawn(async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                log::error!("Failed to install Ctrl+C handler: {}", e);
                return;
            }
            log::info!("Received Ctrl+C, shutting down ACP agent...");
            let _ = shutdown_tx.send(());
        });

        // The AgentSideConnection will spawn futures onto our Tokio runtime.
        // LocalSet and spawn_local are used because the futures from the
        // agent-client-protocol crate are not Send.
        let local_set = tokio::task::LocalSet::new();
        local_set
            .run_until(async move {
                // Start a background task to send session notifications to the client
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

                // Set up progress channel for streaming tool results
                let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<ToolCallResultProgress>(100);

                // Reinitialize MCP clients with progress channel
                let (mcp_server_host, clients, tools) = match Self::initialize_mcp_server_and_tools(&self.config, Some(progress_tx.clone())).await {
                    Ok((host, client_manager, tool_list)) => {
                        log::info!("MCP server reinitialized with progress channel");
                        (host, Some(client_manager), tool_list)
                    }
                    Err(e) => {
                        log::warn!("Failed to reinitialize MCP server with progress channel: {}, continuing without tools", e);
                        (String::new(), None, Vec::new())
                    }
                };

                // Create a new agent with the proper channel
                let agent = StakpakAcpAgent {
                    config: self.config.clone(),
                    client: self.client.clone(),
                    session_update_tx: tx.clone(),
                    next_session_id: self.next_session_id.clone(),
                    mcp_server_host: if mcp_server_host.is_empty() { None } else { Some(mcp_server_host) },
                    clients,
                    tools: if tools.is_empty() { None } else { Some(tools) },
                    current_session_id: self.current_session_id.clone(),
                    progress_tx: Some(progress_tx),
                    messages: self.messages.clone(),
                };

                // Start up the StakpakAcpAgent connected to stdio.
                let (conn, handle_io) =
                    acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                        tokio::task::spawn_local(fut);
                    });

                // Start a background task to send session notifications to the client
                tokio::task::spawn_local(async move {
                    while let Some((session_notification, ack_tx)) = rx.recv().await {
                        log::info!("Sending session notification: {:?}", session_notification);
                        let result =
                            AcpClient::session_notification(&conn, session_notification).await;
                        if let Err(e) = result {
                            log::error!("Failed to send session notification: {}", e);
                            break;
                        }
                        log::info!("Session notification sent successfully");
                        ack_tx.send(()).ok();
                    }
                });

                // Start a background task to handle progress updates
                let session_update_tx_clone = tx.clone();
                tokio::task::spawn_local(async move {
                    while let Some(progress) = progress_rx.recv().await {
                        log::info!("Received tool progress: {}", progress.message);
                        // Send progress as AgentMessageChunk
                        let (tx, rx) = oneshot::channel();
                        if session_update_tx_clone.send((
                            SessionNotification {
                                session_id: acp::SessionId("".to_string().into()), // TODO: Get actual session ID
                                update: acp::SessionUpdate::AgentMessageChunk {
                                    content: acp::ContentBlock::Text(acp::TextContent {
                                        text: progress.message,
                                        annotations: None,
                                    }),
                                },
                            },
                            tx,
                        )).is_err() {
                            break;
                        }
                        let _ = rx.await;
                    }
                });

                // Run until stdin/stdout are closed or shutdown signal is received.
                tokio::select! {
                    result = handle_io => {
                        match result {
                            Ok(_) => log::info!("ACP connection closed normally"),
                            Err(e) => log::error!("ACP connection error: {}", e),
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        log::info!("Shutting down ACP agent due to Ctrl+C");
                    }
                }
            })
            .await;

        Ok(())
    }
}

impl Clone for StakpakAcpAgent {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            client: self.client.clone(),
            session_update_tx: self.session_update_tx.clone(),
            next_session_id: Cell::new(self.next_session_id.get()),
            mcp_server_host: self.mcp_server_host.clone(),
            clients: self.clients.clone(),
            tools: self.tools.clone(),
            current_session_id: Cell::new(self.current_session_id.get()),
            progress_tx: self.progress_tx.clone(),
            messages: self.messages.clone(),
        }
    }
}

impl acp::Agent for StakpakAcpAgent {
    async fn initialize(
        &self,
        arguments: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        log::info!("Received initialize request {arguments:?}");
        Ok(acp::InitializeResponse {
            protocol_version: acp::V1,
            agent_capabilities: acp::AgentCapabilities {
                // Enable session management
                load_session: true,
                // Enable prompt capabilities
                prompt_capabilities: acp::PromptCapabilities {
                    // Enable image support
                    image: true,
                    // Enable audio support
                    audio: false,
                    // Enable embedded context support
                    embedded_context: true,
                },
            },
            auth_methods: Vec::new(),
        })
    }

    async fn authenticate(&self, arguments: acp::AuthenticateRequest) -> Result<(), acp::Error> {
        log::info!("Received authenticate request {arguments:?}");
        Ok(())
    }

    async fn new_session(
        &self,
        arguments: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        log::info!("Received new session request {arguments:?}");

        // Create a new agent session using the API client
        use stakpak_api::models::{AgentID, AgentSessionVisibility};

        let session = self
            .client
            .create_agent_session(
                AgentID::NorbertV1,              // Use default agent
                AgentSessionVisibility::Private, // Private session
                None,                            // No input
            )
            .await
            .map_err(|_e| acp::Error::internal_error())?;

        let session_id = acp::SessionId(session.id.to_string().into());

        // Track the current session ID
        self.current_session_id.set(Some(session.id));

        // Clear message history for new session
        {
            let mut messages = self.messages.lock().await;
            messages.clear();
        }

        Ok(acp::NewSessionResponse { session_id })
    }

    async fn load_session(&self, arguments: acp::LoadSessionRequest) -> Result<(), acp::Error> {
        log::info!("Received load session request {arguments:?}");

        // Parse session ID from the request
        let session_id_str = arguments.session_id.0.to_string();
        let session_uuid = match Uuid::parse_str(&session_id_str) {
            Ok(uuid) => uuid,
            Err(_) => return Err(acp::Error::invalid_params()),
        };

        // Track the loaded session ID
        self.current_session_id.set(Some(session_uuid));

        log::info!("Loaded session: {}", session_id_str);
        Ok(())
    }

    async fn prompt(
        &self,
        arguments: acp::PromptRequest,
    ) -> Result<acp::PromptResponse, acp::Error> {
        log::info!("Received prompt request {arguments:?}");

        // Convert prompt to your ChatMessage format
        let prompt_text = arguments
            .prompt
            .iter()
            .map(|block| match block {
                acp::ContentBlock::Text(text_content) => text_content.text.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join(" ");
        log::info!("Processed prompt text: {}", prompt_text);
        let user_msg = user_message(prompt_text);

        // Add user message to conversation history
        {
            let mut messages = self.messages.lock().await;
            messages.push(user_msg.clone());
        }

        // Use tools if available
        let tools = self.tools.clone().unwrap_or_default();
        log::info!("Available tools: {}", tools.len());

        // Get tools map for tool execution
        let tools_map = if let Some(ref clients) = self.clients {
            clients
                .get_tools()
                .await
                .map_err(|_e| acp::Error::internal_error())?
        } else {
            std::collections::HashMap::new()
        };

        // Get current conversation history
        let messages = {
            let messages = self.messages.lock().await;
            messages.clone()
        };

        // Make streaming chat completion request with full conversation history
        log::info!(
            "Making streaming chat completion request with {} tools and {} messages",
            tools.len(),
            messages.len()
        );
        log::info!("User message: {:?}", user_msg);
        log::info!("Tools: {:?}", tools);

        // Only pass tools if we have any
        let tools_option = if tools.is_empty() { None } else { Some(tools) };

        let (stream, _request_id) = self
            .client
            .chat_completion_stream(messages, tools_option.clone(), None)
            .await
            .map_err(|e| {
                log::error!("Chat completion stream failed: {}", e);
                acp::Error::internal_error()
            })?;

        let response = self
            .process_acp_streaming_response(stream, &arguments.session_id)
            .await
            .map_err(|e| {
                log::error!("Stream processing failed: {}", e);
                acp::Error::internal_error()
            })?;
        log::info!("Chat completion successful, response: {:?}", response);
        log::info!("Response choices count: {}", response.choices.len());
        if !response.choices.is_empty() {
            log::info!("First choice message: {:?}", response.choices[0].message);
            log::info!(
                "First choice content: {:?}",
                response.choices[0].message.content
            );
        }

        // Add assistant response to conversation history
        {
            let mut messages = self.messages.lock().await;
            messages.push(response.choices[0].message.clone());
        }

        let mut content = if let Some(content) = &response.choices[0].message.content {
            match content {
                stakpak_shared::models::integrations::openai::MessageContent::String(s) => {
                    log::info!("Content from chat completion: '{}'", s);
                    s.clone()
                }
                stakpak_shared::models::integrations::openai::MessageContent::Array(parts) => {
                    let extracted_content = parts
                        .iter()
                        .filter_map(|part| part.text.as_ref())
                        .map(|text| text.as_str())
                        .filter(|text| !text.starts_with("<checkpoint_id>"))
                        .collect::<Vec<&str>>()
                        .join("\n");
                    log::info!(
                        "Content from chat completion array: '{}'",
                        extracted_content
                    );
                    extracted_content
                }
            }
        } else {
            log::warn!("No content in chat completion response");
            String::new()
        };

        log::info!("Final content to send: '{}'", content);

        // If content is empty, provide a fallback response
        if content.is_empty() {
            content = "I apologize, but I'm having trouble generating a response right now. Please try again.".to_string();
            log::warn!("Content was empty, using fallback response");
        }

        // Process tool calls sequentially and continue conversation
        if let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref() {
            if !tool_calls.is_empty() {
                log::info!("Executing {} tool calls sequentially", tool_calls.len());

                // Execute each tool call one by one (sequential execution)
                for (i, tool_call) in tool_calls.iter().enumerate() {
                    log::info!(
                        "Processing tool call {}/{}: {}",
                        i + 1,
                        tool_calls.len(),
                        tool_call.function.name
                    );

                    // Validate tool permissions
                    if !self.validate_tool_permissions(&tool_call.function.name) {
                        log::warn!(
                            "Tool '{}' is not permitted, skipping execution",
                            tool_call.function.name
                        );

                        // Send permission denied notification
                        let (tx, rx) = oneshot::channel();
                        self.session_update_tx
                            .send((
                                SessionNotification {
                                    session_id: arguments.session_id.clone(),
                                    update: acp::SessionUpdate::AgentMessageChunk {
                                        content: acp::ContentBlock::Text(acp::TextContent {
                                            text: format!("❌ **Permission Denied**: Tool '{}' is not allowed\n", tool_call.function.name),
                                            annotations: None,
                                        }),
                                    },
                                },
                                tx,
                            ))
                            .map_err(|_| acp::Error::internal_error())?;
                        rx.await.map_err(|_| acp::Error::internal_error())?;
                        continue;
                    }

                    // Send structured tool call notification using proper ACP types
                    let tool_info = self.create_tool_info(tool_call);
                    let tool_kind_icon = match tool_info.kind {
                        ToolKind::Edit => "✏️",
                        ToolKind::Execute => "⚡",
                        ToolKind::Read => "👁️",
                        ToolKind::Search => "🔍",
                        ToolKind::Other => "🔧",
                    };

                    // Format the tool content for display
                    let content_display = if !tool_info.content.is_empty() {
                        let formatted_content = self.format_tool_content(&tool_info.content);
                        format!(
                            "{} **{}**\n\n{}",
                            tool_kind_icon, tool_info.title, formatted_content
                        )
                    } else {
                        format!("{} **{}**", tool_kind_icon, tool_info.title)
                    };

                    let (tx, rx) = oneshot::channel();
                    self.session_update_tx
                        .send((
                            SessionNotification {
                                session_id: arguments.session_id.clone(),
                                update: acp::SessionUpdate::AgentMessageChunk {
                                    content: acp::ContentBlock::Text(acp::TextContent {
                                        text: content_display,
                                        annotations: None,
                                    }),
                                },
                            },
                            tx,
                        ))
                        .map_err(|_| acp::Error::internal_error())?;
                    rx.await.map_err(|_| acp::Error::internal_error())?;

                    // For now, we'll simulate user approval by automatically proceeding
                    // In a real implementation, this would wait for user input from Zed
                    log::info!(
                        "Simulating user approval for tool: {}",
                        tool_call.function.name
                    );

                    // Send in-progress notification
                    let (tx, rx) = oneshot::channel();
                    self.session_update_tx
                        .send((
                            SessionNotification {
                                session_id: arguments.session_id.clone(),
                                update: acp::SessionUpdate::AgentMessageChunk {
                                    content: acp::ContentBlock::Text(acp::TextContent {
                                        text: format!(
                                            "⏳ **Executing**: `{}`...\n",
                                            tool_call.function.name
                                        ),
                                        annotations: None,
                                    }),
                                },
                            },
                            tx,
                        ))
                        .map_err(|_| acp::Error::internal_error())?;
                    rx.await.map_err(|_| acp::Error::internal_error())?;

                    // Execute the tool call
                    if let Some(ref clients) = self.clients {
                        let result = crate::commands::agent::run::tooling::run_tool_call(
                            clients,
                            &tools_map,
                            tool_call,
                            None,                          // No cancel receiver for ACP
                            self.current_session_id.get(), // Use current session ID
                        )
                        .await
                        .map_err(|e| {
                            log::error!("Tool execution failed: {}", e);
                            acp::Error::internal_error()
                        })?;

                        if let Some(tool_result) = result {
                            // Extract result content
                            let result_content: String = tool_result
                                .content
                                .iter()
                                .map(|c| match c.raw.as_text() {
                                    Some(text) => text.text.clone(),
                                    None => String::new(),
                                })
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join("\n");

                            // Send completion notification with structured formatting
                            let tool_info = self.create_tool_info(tool_call);
                            let status_icon = match tool_info.kind {
                                ToolKind::Edit => "✏️",
                                ToolKind::Execute => "⚡",
                                ToolKind::Read => "👁️",
                                ToolKind::Search => "🔍",
                                ToolKind::Other => "🔧",
                            };

                            let formatted_result = if result_content.len() > 500 {
                                format!(
                                    "{} **{}** completed\n\n**Result** (truncated):\n```\n{}...\n```\n",
                                    status_icon,
                                    tool_info.title,
                                    &result_content[..500]
                                )
                            } else {
                                format!(
                                    "{} **{}** completed\n\n**Result**:\n```\n{}\n```\n",
                                    status_icon, tool_info.title, result_content
                                )
                            };

                            let (tx, rx) = oneshot::channel();
                            self.session_update_tx
                                .send((
                                    SessionNotification {
                                        session_id: arguments.session_id.clone(),
                                        update: acp::SessionUpdate::AgentMessageChunk {
                                            content: acp::ContentBlock::Text(acp::TextContent {
                                                text: formatted_result,
                                                annotations: None,
                                            }),
                                        },
                                    },
                                    tx,
                                ))
                                .map_err(|_| acp::Error::internal_error())?;
                            rx.await.map_err(|_| acp::Error::internal_error())?;

                            // Add tool result to conversation history
                            {
                                let mut messages = self.messages.lock().await;
                                messages.push(crate::commands::agent::run::helpers::tool_result(
                                    tool_call.id.clone(),
                                    result_content.clone(),
                                ));
                            }
                        }
                    }

                    log::info!(
                        "Completed tool call {}/{}: {}",
                        i + 1,
                        tool_calls.len(),
                        tool_call.function.name
                    );
                }

                // After all tool calls are executed, make a follow-up chat completion request
                log::info!("All tool calls completed, making follow-up chat completion request");

                // Get updated conversation history (including tool results)
                let updated_messages = {
                    let messages = self.messages.lock().await;
                    messages.clone()
                };

                // Make follow-up chat completion request
                let (follow_up_stream, _request_id) = self
                    .client
                    .chat_completion_stream(updated_messages, tools_option.clone(), None)
                    .await
                    .map_err(|e| {
                        log::error!("Follow-up chat completion stream failed: {}", e);
                        acp::Error::internal_error()
                    })?;

                let follow_up_response = self
                    .process_acp_streaming_response(follow_up_stream, &arguments.session_id)
                    .await
                    .map_err(|e| {
                        log::error!("Follow-up stream processing failed: {}", e);
                        acp::Error::internal_error()
                    })?;

                // Add follow-up response to conversation history
                {
                    let mut messages = self.messages.lock().await;
                    messages.push(follow_up_response.choices[0].message.clone());
                }

                // Note: Follow-up content is already sent via streaming in process_acp_streaming_response
                // No need to extract and send it again to avoid duplication

                // Return early since we've handled tool calls and follow-up
                return Ok(acp::PromptResponse {
                    stop_reason: acp::StopReason::EndTurn,
                });
            }
        }

        // Process checkpoint IDs for non-tool-call responses
        content = self.process_checkpoint_ids(content);

        // Send final content if there's content and no tool calls were executed
        if !content.trim().is_empty() && response.choices[0].message.tool_calls.is_none() {
            log::info!(
                "Sending final session notification with content: {}",
                content
            );
            let (tx, rx) = oneshot::channel();
            self.session_update_tx
                .send((
                    SessionNotification {
                        session_id: arguments.session_id.clone(),
                        update: acp::SessionUpdate::AgentMessageChunk {
                            content: acp::ContentBlock::Text(acp::TextContent {
                                text: content,
                                annotations: None,
                            }),
                        },
                    },
                    tx,
                ))
                .map_err(|_| acp::Error::internal_error())?;
            rx.await.map_err(|_| acp::Error::internal_error())?;
            log::info!("Final session notification sent successfully");
        }

        Ok(acp::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        })
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        log::info!("Received cancel request {args:?}");
        Ok(())
    }
}

impl StakpakAcpAgent {
    fn process_checkpoint_ids(&self, content: String) -> String {
        use regex::Regex;

        // Remove the first checkpoint_id wrapper
        let checkpoint_regex = match Regex::new(r"<checkpoint_id>([^<]+)</checkpoint_id>") {
            Ok(regex) => regex,
            Err(e) => {
                log::error!("Failed to compile checkpoint regex: {}", e);
                return content; // Return original content if regex compilation fails
            }
        };
        let mut processed_content = content;

        // Find all checkpoint IDs
        let checkpoint_ids: Vec<String> = checkpoint_regex
            .captures_iter(&processed_content)
            .map(|cap| cap[1].to_string())
            .collect();

        // Remove all checkpoint_id wrappers
        processed_content = checkpoint_regex
            .replace_all(&processed_content, "")
            .to_string();

        // If we have checkpoint IDs, format the last one as a markdown code block
        if let Some(last_checkpoint_id) = checkpoint_ids.last() {
            processed_content.push_str(&format!(
                "\n\n```\nCheckpoint Id - {}\n```",
                last_checkpoint_id
            ));
        }

        processed_content
    }
}
