use crate::commands::agent::run::helpers::user_message;
use crate::utils::network;
use crate::{commands::agent::run::helpers::convert_tools_map_with_filter, config::AppConfig};
use agent_client_protocol::{self as acp, Client as AcpClient, SessionNotification};
use stakpak_api::{Client, ClientConfig};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::{MCPServerConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::CertificateChain;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

pub struct StakpakAcpAgent {
    config: AppConfig,
    client: Client,
    session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    next_session_id: Cell<u64>,
    mcp_server_host: Option<String>,
    clients: Option<Arc<ClientManager>>,
    tools: Option<Vec<stakpak_shared::models::integrations::openai::Tool>>,
}

impl StakpakAcpAgent {
    async fn initialize_mcp_server_and_tools(
        config: &AppConfig,
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
                None, // No progress channel for ACP
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

    pub async fn new(
        config: AppConfig,
        session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    ) -> Result<Self, String> {
        let api_config: ClientConfig = config.clone().into();
        let client =
            Client::new(&api_config).map_err(|e| format!("Failed to create client: {}", e))?;

        // Initialize MCP server and tools (optional for ACP)
        let (mcp_server_host, clients, tools) =
            match Self::initialize_mcp_server_and_tools(&config).await {
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

                // Create a new agent with the proper channel
                let agent = StakpakAcpAgent {
                    config: self.config.clone(),
                    client: self.client.clone(),
                    session_update_tx: tx.clone(),
                    next_session_id: self.next_session_id.clone(),
                    mcp_server_host: self.mcp_server_host.clone(),
                    clients: self.clients.clone(),
                    tools: self.tools.clone(),
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
            agent_capabilities: acp::AgentCapabilities::default(),
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

        Ok(acp::NewSessionResponse { session_id })
    }

    async fn load_session(&self, arguments: acp::LoadSessionRequest) -> Result<(), acp::Error> {
        log::info!("Received load session request {arguments:?}");
        Err(acp::Error::method_not_found())
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

        // Make chat completion request (not streaming for ACP)
        log::info!("Making chat completion request with {} tools", tools.len());
        log::info!("User message: {:?}", user_msg);
        log::info!("Tools: {:?}", tools);

        // Only pass tools if we have any
        let tools_option = if tools.is_empty() { None } else { Some(tools) };

        let response = self
            .client
            .chat_completion(vec![user_msg], tools_option)
            .await
            .map_err(|e| {
                log::error!("Chat completion failed: {}", e);
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

        // Check if there are tool calls to execute
        if let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref() {
            if !tool_calls.is_empty() {
                log::info!("Executing {} tool calls", tool_calls.len());

                // Execute all tool calls
                for tool_call in tool_calls.iter() {
                    log::info!("Executing tool: {}", tool_call.function.name);

                    // Execute the tool call
                    if let Some(ref clients) = self.clients {
                        let result = crate::commands::agent::run::tooling::run_tool_call(
                            clients, &tools_map, tool_call,
                            None, // No cancel receiver for ACP
                            None, // No session ID for ACP
                        )
                        .await
                        .map_err(|e| {
                            log::error!("Tool execution failed: {}", e);
                            acp::Error::internal_error()
                        })?;

                        if let Some(tool_result) = result {
                            // Add tool result to content
                            content.push_str(&format!(
                                "\n\nTool '{}' executed successfully.",
                                tool_call.function.name
                            ));

                            // Add tool result content if available
                            for result_content in &tool_result.content {
                                if let Some(text_content) = result_content.raw.as_text() {
                                    content.push_str(&format!("\n{}", text_content.text));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Process checkpoint IDs
        content = self.process_checkpoint_ids(content);

        // Send the response as a session notification
        log::info!("Sending session notification with content: {}", content);
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
        log::info!("Session notification sent successfully");

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
