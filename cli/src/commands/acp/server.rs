use crate::config::AppConfig;
use crate::commands::agent::run::helpers::user_message;
use crate::commands::agent::run::stream::process_responses_stream;
use stakpak_api::{Client, ClientConfig};
use agent_client_protocol::{self as acp, SessionNotification, Client as AcpClient};
use std::cell::Cell;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

pub struct StakpakAcpAgent {
    config: AppConfig,
    client: Client,
    session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    next_session_id: Cell<u64>,
}

impl StakpakAcpAgent {
    pub async fn new(
        config: AppConfig,
        session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    ) -> Result<Self, String> {
        let api_config: ClientConfig = config.clone().into();
        let client = Client::new(&api_config)
            .map_err(|e| format!("Failed to create client: {}", e))?;
        Ok(Self { 
            config, 
            client,
            session_update_tx,
            next_session_id: Cell::new(0),
        })
    }
    
    pub async fn run_stdio(&self) -> Result<(), String> {
        let outgoing = tokio::io::stdout().compat_write();
        let incoming = tokio::io::stdin().compat();

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
                };
                
                // Start up the StakpakAcpAgent connected to stdio.
                let (conn, handle_io) = acp::AgentSideConnection::new(
                    agent, 
                    outgoing, 
                    incoming, 
                    |fut| {
                        tokio::task::spawn_local(fut);
                    }
                );
                
                // Start a background task to send session notifications to the client
                tokio::task::spawn_local(async move {
                    while let Some((session_notification, ack_tx)) = rx.recv().await {
                        let result = AcpClient::session_notification(&conn, session_notification).await;
                        if let Err(e) = result {
                            log::error!("Failed to send session notification: {}", e);
                            break;
                        }
                        ack_tx.send(()).ok();
                    }
                });
                
                // Run until stdin/stdout are closed.
                handle_io.await
            })
            .await
            .map_err(|e| format!("ACP connection error: {}", e))?;
        
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
        
        let session = self.client
            .create_agent_session(
                AgentID::NorbertV1, // Use default agent
                AgentSessionVisibility::Private, // Private session
                None, // No input
            )
            .await
            .map_err(|_e| acp::Error::internal_error())?;
        
        let session_id = acp::SessionId(session.id.to_string().into());
        
        Ok(acp::NewSessionResponse {
            session_id,
        })
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
        
        // Create a dummy input channel for the streaming function
        let (dummy_input_tx, _dummy_input_rx) = mpsc::channel(100);
        
        // Convert prompt to your ChatMessage format
        let prompt_text = arguments.prompt
            .iter()
            .map(|block| match block {
                acp::ContentBlock::Text(text_content) => text_content.text.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join(" ");
        let user_msg = user_message(prompt_text);
        
        // Use your existing streaming logic
        let stream_result = self.client
            .chat_completion_stream(vec![user_msg], None, None)
            .await
            .map_err(|_e| acp::Error::internal_error())?;
        
        let (stream, _request_id) = stream_result;
        
        // Reuse your process_responses_stream function
        let response = process_responses_stream(stream, &dummy_input_tx).await
            .map_err(|_e| acp::Error::internal_error())?;
        
        // Extract the text content
        let mut content = response.choices[0].message.content
            .as_ref()
            .map(|c| match c {
                stakpak_shared::models::integrations::openai::MessageContent::String(s) => s.clone(),
                _ => String::new(),
            })
            .unwrap_or_default();
        
        // Process checkpoint IDs
        content = self.process_checkpoint_ids(content);
        
        // Send the response as a session notification
        let (tx, rx) = oneshot::channel();
        self.session_update_tx
            .send((
                SessionNotification {
                    session_id: arguments.session_id.clone(),
                    update: acp::SessionUpdate::AgentMessageChunk { 
                        content: acp::ContentBlock::Text(acp::TextContent { 
                            text: content,
                            annotations: None,
                        }) 
                    },
                },
                tx,
            ))
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?;
        
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
        let checkpoint_regex = Regex::new(r"<checkpoint_id>([^<]+)</checkpoint_id>").unwrap();
        let mut processed_content = content;
        
        // Find all checkpoint IDs
        let checkpoint_ids: Vec<String> = checkpoint_regex
            .captures_iter(&processed_content)
            .map(|cap| cap[1].to_string())
            .collect();
        
        // Remove all checkpoint_id wrappers
        processed_content = checkpoint_regex.replace_all(&processed_content, "").to_string();
        
        // If we have checkpoint IDs, format the last one as a markdown code block
        if let Some(last_checkpoint_id) = checkpoint_ids.last() {
            processed_content.push_str(&format!("\n\n```\nCheckpoint Id - {}\n```", last_checkpoint_id));
        }
        
        processed_content
    }
}

