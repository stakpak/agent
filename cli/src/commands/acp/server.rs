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

// TODO:: MAKE SURE THAT TOOL CALL STREAM IS WORKING
// TODO:: ADD ACP AGENT MESSAGE STREAM CANCELLATION
// TODO:: ADD ACP TOOL CALL STREAM CANCELLATION
// TODO:: EXTRACT SCRATCHPAD/CHECKLIST INTO TODOS CHECK LIST TO WORK ON ZED

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
    // Add permission request channel
    permission_request_tx: Option<
        mpsc::UnboundedSender<(
            acp::RequestPermissionRequest,
            oneshot::Sender<acp::RequestPermissionResponse>,
        )>,
    >,
}

impl StakpakAcpAgent {
    // Helper method to send proper ACP tool call notifications
    #[allow(clippy::too_many_arguments)]
    async fn send_tool_call_notification(
        &self,
        session_id: &acp::SessionId,
        tool_call_id: String,
        title: String,
        kind: &acp::ToolKind,
        raw_input: serde_json::Value,
        content: Option<Vec<acp::ToolCallContent>>,
        locations: Option<Vec<acp::ToolCallLocation>>,
    ) -> Result<(), acp::Error> {
        let (tx, rx) = oneshot::channel();
        self.session_update_tx
            .send((
                SessionNotification {
                    session_id: session_id.clone(),
                    update: acp::SessionUpdate::ToolCall(acp::ToolCall {
                        id: acp::ToolCallId(tool_call_id.into()),
                        title,
                        kind: kind.clone(),
                        status: acp::ToolCallStatus::Pending,
                        content: content.unwrap_or_default(),
                        locations: locations.unwrap_or_default(),
                        raw_input: Some(raw_input),
                        raw_output: None,
                    }),
                },
                tx,
            ))
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?;
        Ok(())
    }

    // Helper method to send tool call status updates using proper ACP
    async fn send_tool_call_update(
        &self,
        session_id: &acp::SessionId,
        tool_call_id: String,
        status: acp::ToolCallStatus,
        content: Option<Vec<acp::ToolCallContent>>,
        raw_output: Option<serde_json::Value>,
    ) -> Result<(), acp::Error> {
        let (tx, rx) = oneshot::channel();
        self.session_update_tx
            .send((
                SessionNotification {
                    session_id: session_id.clone(),
                    update: acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate {
                        id: acp::ToolCallId(tool_call_id.into()),
                        fields: acp::ToolCallUpdateFields {
                            status: Some(status),
                            content,
                            raw_output,
                            ..Default::default()
                        },
                    }),
                },
                tx,
            ))
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?;
        Ok(())
    }

    // Helper method to send proper ACP permission request
    async fn send_permission_request(
        &self,
        session_id: &acp::SessionId,
        tool_call_id: String,
        tool_call: &ToolCall,
        tool_title: &str,
    ) -> Result<bool, acp::Error> {
        log::info!(
            "Requesting permission for tool: {} - {}",
            tool_call.function.name,
            tool_title
        );
        log::info!("Tool Call ID: {}", tool_call_id);

        // Create permission options as shown in the image
        let options = vec![
            acp::PermissionOption {
                id: acp::PermissionOptionId("allow".into()),
                name: "Allow".to_string(),
                kind: acp::PermissionOptionKind::AllowOnce,
            },
            acp::PermissionOption {
                id: acp::PermissionOptionId("reject".into()),
                name: "Reject".to_string(),
                kind: acp::PermissionOptionKind::RejectOnce,
            },
        ];

        // Create the permission request
        let permission_request = acp::RequestPermissionRequest {
            session_id: session_id.clone(),
            tool_call: acp::ToolCallUpdate {
                id: acp::ToolCallId(tool_call_id.clone().into()),
                fields: acp::ToolCallUpdateFields {
                    title: Some(tool_title.to_string()),
                    raw_input: Some(
                        serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::Value::Null),
                    ),
                    ..Default::default()
                },
            },
            options,
        };

        // Send the actual permission request if channel is available
        if let Some(ref permission_tx) = self.permission_request_tx {
            let (response_tx, response_rx) = oneshot::channel();

            // Send the permission request
            if permission_tx
                .send((permission_request, response_tx))
                .is_err()
            {
                log::error!("Failed to send permission request");
                return Ok(false);
            }

            // Wait for the response
            match response_rx.await {
                Ok(response) => match response.outcome {
                    acp::RequestPermissionOutcome::Selected { option_id } => {
                        log::info!("User selected permission option: {}", option_id.0);
                        Ok(option_id.0.as_ref() == "allow"
                            || option_id.0.as_ref() == "allow_always")
                    }
                    acp::RequestPermissionOutcome::Cancelled => {
                        log::info!("Permission request was cancelled");
                        Ok(false)
                    }
                },
                Err(_) => {
                    log::error!("Permission request failed");
                    Ok(false)
                }
            }
        } else {
            // Fall back to auto-approve if no permission channel available
            log::warn!("No permission request channel available, auto-approving");
            Ok(true)
        }
    }

    // Helper method to generate appropriate tool title based on tool type and arguments
    fn generate_tool_title(&self, tool_name: &str, raw_input: &serde_json::Value) -> String {
        match tool_name {
            "view" => {
                // Extract path from arguments for view tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Read {}", path)
                } else {
                    "Read".to_string()
                }
            }
            "run_command" => {
                // Extract command from arguments for run_command tool
                if let Some(command) = raw_input.get("command").and_then(|c| c.as_str()) {
                    format!("Run command {}", command)
                } else {
                    "Run command".to_string()
                }
            }
            "create" | "create_file" => {
                // Extract path from arguments for create tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Creating {}", path)
                } else {
                    "Creating".to_string()
                }
            }
            "str_replace" | "edit_file" => {
                // Extract path from arguments for edit tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Editing {}", path)
                } else {
                    "Editing".to_string()
                }
            }
            "delete_file" => {
                // Extract path from arguments for delete tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Deleting {}", path)
                } else {
                    "Deleting".to_string()
                }
            }
            "search_docs" => {
                // Extract query from arguments for search tool
                if let Some(query) = raw_input.get("query").and_then(|q| q.as_str()) {
                    format!("Search docs: {}", query)
                } else {
                    "Search docs".to_string()
                }
            }
            "local_code_search" => {
                // Extract query from arguments for search tool
                if let Some(query) = raw_input.get("query").and_then(|q| q.as_str()) {
                    format!("Search local context: {}", query)
                } else {
                    "Search local context".to_string()
                }
            }
            "read_rulebook" => "Read rulebook".to_string(),
            _ => {
                // Default case: format tool name nicely and add path if available
                let formatted_name = self.format_tool_name(tool_name);
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("{} {}", formatted_name, path)
                } else {
                    formatted_name
                }
            }
        }
    }

    // Helper method to format tool names nicely (capitalize words, remove underscores)
    fn format_tool_name(&self, tool_name: &str) -> String {
        tool_name
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    // Helper method to get appropriate ToolKind based on tool name
    fn get_tool_kind(&self, tool_name: &str) -> acp::ToolKind {
        match tool_name {
            "view" | "read_rulebook" => acp::ToolKind::Read,
            "run_command" => acp::ToolKind::Execute,
            "create" | "create_file" | "str_replace" | "edit_file" => acp::ToolKind::Edit,
            "delete_file" => acp::ToolKind::Delete,
            "search_docs" | "local_code_search" => acp::ToolKind::Search,
            _ => acp::ToolKind::Other,
        }
    }

    // Helper method to determine if a tool should use Diff content type
    fn should_use_diff_content(&self, tool_name: &str) -> bool {
        matches!(
            tool_name,
            "create" | "create_file" | "str_replace" | "edit_file"
        )
    }

    // Helper method to determine if a tool is a file creation tool
    fn is_file_creation_tool(&self, tool_name: &str) -> bool {
        matches!(tool_name, "create" | "create_file")
    }

    // Helper method to determine if a tool should be auto-approved
    fn is_auto_approved_tool(&self, tool_name: &str) -> bool {
        matches!(
            tool_name,
            "view" | "search_docs" | "read_rulebook" | "local_code_search"
        )
    }

    // Helper method to create proper rawInput for tool calls
    fn create_raw_input(&self, raw_input: &serde_json::Value, abs_path: &str) -> serde_json::Value {
        let mut input_obj = serde_json::Map::new();

        // Add abs_path
        input_obj.insert(
            "abs_path".to_string(),
            serde_json::Value::String(abs_path.to_string()),
        );

        // Copy other fields, but rename old_str/new_str to old_string/new_string
        for (key, value) in raw_input.as_object().unwrap_or(&serde_json::Map::new()) {
            match key.as_str() {
                "old_str" => {
                    input_obj.insert("old_string".to_string(), value.clone());
                }
                "new_str" => {
                    input_obj.insert("new_string".to_string(), value.clone());
                }
                "path" => {
                    // Keep path as is, but also add abs_path
                    input_obj.insert("path".to_string(), value.clone());
                }
                _ => {
                    input_obj.insert(key.clone(), value.clone());
                }
            }
        }

        serde_json::Value::Object(input_obj)
    }

    // Helper method to generate unique tool call IDs
    fn generate_tool_call_id(&self) -> String {
        format!(
            "toolu_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        )
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
        // TODO:: MAKE SURE THAT TOOL CALL STREAM IS WORKING
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
            permission_request_tx: None,
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

                // Create permission request channel
                let (permission_tx, mut permission_rx) = mpsc::unbounded_channel::<(acp::RequestPermissionRequest, oneshot::Sender<acp::RequestPermissionResponse>)>();

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
                    permission_request_tx: Some(permission_tx),
                };

                // Start up the StakpakAcpAgent connected to stdio.
                let (conn, handle_io) =
                    acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                        tokio::task::spawn_local(fut);
                    });

                // Wrap connection in Arc for sharing
                let conn_arc = Arc::new(conn);

                // Start a background task to send session notifications to the client
                let conn_for_notifications = conn_arc.clone();
                tokio::task::spawn_local(async move {
                    while let Some((session_notification, ack_tx)) = rx.recv().await {
                        log::info!("Sending session notification: {:?}", session_notification);
                        let result =
                            AcpClient::session_notification(&*conn_for_notifications, session_notification).await;
                        if let Err(e) = result {
                            log::error!("Failed to send session notification: {}", e);
                            break;
                        }
                        log::info!("Session notification sent successfully");
                        ack_tx.send(()).ok();
                    }
                });

                // Start a background task to handle permission requests
                let conn_for_permissions = conn_arc.clone();
                tokio::task::spawn_local(async move {
                    while let Some((permission_request, response_tx)) = permission_rx.recv().await {
                        log::info!("Sending permission request: {:?}", permission_request);
                        match conn_for_permissions.request_permission(permission_request).await {
                            Ok(response) => {
                                log::info!("Permission request response: {:?}", response);
                                let _ = response_tx.send(response);
                            }
                            Err(e) => {
                                log::error!("Permission request failed: {}", e);
                                // Send a default rejection response
                                let _ = response_tx.send(acp::RequestPermissionResponse {
                                    outcome: acp::RequestPermissionOutcome::Cancelled,
                                });
                            }
                        }
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
            permission_request_tx: self.permission_request_tx.clone(),
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
        let mut current_messages = {
            let messages = self.messages.lock().await;
            messages.clone()
        };

        // Check if the initial response has tool calls
        let initial_has_tool_calls = response.choices[0]
            .message
            .tool_calls
            .as_ref()
            .map(|tc| !tc.is_empty())
            .unwrap_or(false);

        if initial_has_tool_calls {
            // Loop until no more tool calls are generated
            loop {
                // Get the latest message from the conversation
                let latest_message = match current_messages.last() {
                    Some(message) => message,
                    None => {
                        log::error!("No messages in conversation history");
                        break;
                    }
                };

                if let Some(tool_calls) = latest_message.tool_calls.as_ref() {
                    if tool_calls.is_empty() {
                        break; // No more tool calls, exit loop
                    }

                    log::info!("Executing {} tool calls sequentially", tool_calls.len());

                    // Execute each tool call one by one (sequential execution)
                    for (i, tool_call) in tool_calls.iter().enumerate() {
                        log::info!(
                            "Starting tool call {}/{}: {}",
                            i + 1,
                            tool_calls.len(),
                            tool_call.function.name
                        );

                        // Generate unique tool call ID
                        let tool_call_id = self.generate_tool_call_id();

                        let raw_input = serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::Value::Null);
                        let tool_title =
                            self.generate_tool_title(&tool_call.function.name, &raw_input);
                        let tool_title_clone = tool_title.clone();
                        let tool_kind = self.get_tool_kind(&tool_call.function.name);

                        // Extract path and diff content for diff content if needed
                        let file_path = raw_input
                            .get("path")
                            .and_then(|p| p.as_str())
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| std::path::PathBuf::from("unknown"));

                        // Extract old_str and new_str for editing tools (from tool arguments)
                        let old_string = raw_input
                            .get("old_str")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string());
                        let new_string = raw_input
                            .get("new_str")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string());

                        // Extract abs_path for rawInput
                        let abs_path = raw_input
                            .get("abs_path")
                            .and_then(|p| p.as_str())
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| file_path.to_string_lossy().to_string());

                        // Validate tool permissions
                        if !self.validate_tool_permissions(&tool_call.function.name) {
                            log::warn!(
                                "Tool '{}' is not permitted, skipping execution",
                                tool_call.function.name
                            );

                            // Send permission denied notification using proper ACP format
                            let proper_raw_input = self.create_raw_input(&raw_input, &abs_path);

                            self.send_tool_call_notification(
                                &arguments.session_id,
                                tool_call_id.clone(),
                                format!("❌ Permission Denied: {}", tool_title_clone),
                                &tool_kind,
                                proper_raw_input,
                                None,
                                None,
                            )
                            .await?;
                            continue;
                        }

                        // Prepare content and locations for diff tools
                        let (content, locations) =
                            if self.should_use_diff_content(&tool_call.function.name) {
                                if self.is_file_creation_tool(&tool_call.function.name) {
                                    // For file creation: old_text = None, new_text = result_content
                                    let diff_content = vec![acp::ToolCallContent::Diff {
                                        diff: acp::Diff {
                                            path: file_path.clone(),
                                            old_text: None,
                                            new_text: "".to_string(), // Will be updated after execution
                                        },
                                    }];
                                    let tool_locations = vec![acp::ToolCallLocation {
                                        path: file_path.clone(),
                                        line: Some(0),
                                    }];
                                    (Some(diff_content), Some(tool_locations))
                                } else {
                                    // For file editing: use extracted old_string and new_string
                                    let diff_content = vec![acp::ToolCallContent::Diff {
                                        diff: acp::Diff {
                                            path: file_path.clone(),
                                            old_text: old_string,
                                            new_text: new_string.unwrap_or_default(),
                                        },
                                    }];
                                    let tool_locations = vec![acp::ToolCallLocation {
                                        path: file_path.clone(),
                                        line: Some(0),
                                    }];
                                    (Some(diff_content), Some(tool_locations))
                                }
                            } else {
                                (None, None)
                            };

                        // Send initial tool call notification (PENDING status) - this matches the first image
                        let proper_raw_input = self.create_raw_input(&raw_input, &abs_path);
                        self.send_tool_call_notification(
                            &arguments.session_id,
                            tool_call_id.clone(),
                            tool_title,
                            &tool_kind,
                            proper_raw_input,
                            content,
                            locations,
                        )
                        .await?;

                        // Check if tool should be auto-approved
                        let permission_granted =
                            if self.is_auto_approved_tool(&tool_call.function.name) {
                                log::info!("Auto-approving tool: {}", tool_call.function.name);
                                true
                            } else {
                                // Request permission for tool execution - this matches the second image
                                self.send_permission_request(
                                    &arguments.session_id,
                                    tool_call_id.clone(),
                                    tool_call,
                                    &tool_title_clone,
                                )
                                .await?
                            };

                        if !permission_granted {
                            // Send rejection notification
                            self.send_tool_call_update(
                                &arguments.session_id,
                                tool_call_id.clone(),
                                acp::ToolCallStatus::Failed,
                                Some(vec![acp::ToolCallContent::Content {
                                    content: acp::ContentBlock::Text(acp::TextContent {
                                        text: "Tool execution rejected by user".to_string(),
                                        annotations: None,
                                    }),
                                }]),
                                None,
                            )
                            .await?;
                            continue;
                        }

                        // Update tool call status to IN_PROGRESS
                        self.send_tool_call_update(
                            &arguments.session_id,
                            tool_call_id.clone(),
                            acp::ToolCallStatus::InProgress,
                            None,
                            None,
                        )
                        .await?;

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

                                // Send completion notification
                                let content =
                                    if self.should_use_diff_content(&tool_call.function.name) {
                                        // For diff tools, we already sent the diff in the initial notification
                                        // Just send a simple completion without additional content
                                        None
                                    } else {
                                        // For non-diff tools, send the result content
                                        Some(vec![acp::ToolCallContent::Content {
                                            content: acp::ContentBlock::Text(acp::TextContent {
                                                text: result_content.to_string(),
                                                annotations: None,
                                            }),
                                        }])
                                    };

                                self.send_tool_call_update(
                                    &arguments.session_id,
                                    tool_call_id.clone(),
                                    acp::ToolCallStatus::Completed,
                                    content,
                                    Some(serde_json::json!({
                                        "result": result_content,
                                        "success": true
                                    })),
                                )
                                .await?;

                                // Add tool result to conversation history
                                {
                                    let mut messages = self.messages.lock().await;
                                    messages.push(
                                        crate::commands::agent::run::helpers::tool_result(
                                            tool_call.id.clone(),
                                            result_content.clone(),
                                        ),
                                    );
                                }
                            } else {
                                // Tool execution failed
                                self.send_tool_call_update(
                                    &arguments.session_id,
                                    tool_call_id.clone(),
                                    acp::ToolCallStatus::Failed,
                                    Some(vec![acp::ToolCallContent::Content {
                                        content: acp::ContentBlock::Text(acp::TextContent {
                                            text: "Tool execution failed - no result returned"
                                                .to_string(),
                                            annotations: None,
                                        }),
                                    }]),
                                    Some(serde_json::json!({
                                        "success": false,
                                        "error": "No result returned"
                                    })),
                                )
                                .await?;
                            }
                        } else {
                            // No MCP clients available
                            self.send_tool_call_update(
                                &arguments.session_id,
                                tool_call_id.clone(),
                                acp::ToolCallStatus::Failed,
                                Some(vec![acp::ToolCallContent::Content {
                                    content: acp::ContentBlock::Text(acp::TextContent {
                                        text: "Tool execution failed - no MCP clients available"
                                            .to_string(),
                                        annotations: None,
                                    }),
                                }]),
                                Some(serde_json::json!({
                                    "success": false,
                                    "error": "No MCP clients available"
                                })),
                            )
                            .await?;
                        }

                        log::info!(
                            "Completed tool call {}/{}: {} - waiting before next tool call",
                            i + 1,
                            tool_calls.len(),
                            tool_call.function.name
                        );

                        // Small delay to ensure sequential processing
                        if i < tool_calls.len() - 1 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    }

                    // After all tool calls are executed, make a follow-up chat completion request
                    log::info!(
                        "All tool calls completed, making follow-up chat completion request"
                    );

                    // Get updated conversation history (including tool results)
                    current_messages = {
                        let messages = self.messages.lock().await;
                        messages.clone()
                    };

                    // Make follow-up chat completion request
                    let (follow_up_stream, _request_id) = self
                        .client
                        .chat_completion_stream(
                            current_messages.clone(),
                            tools_option.clone(),
                            None,
                        )
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

                    // Update current_messages for the next iteration
                    current_messages.push(follow_up_response.choices[0].message.clone());

                    // Continue the loop to check for more tool calls
                    continue;
                } else {
                    // No tool calls in the latest message, exit the loop
                    break;
                }
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
        // TODO:: ASSISTANT MESSAGE STREAM CANCELLATION
        // TODO:: TOOL CALL STREAM CANCELLATION
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
