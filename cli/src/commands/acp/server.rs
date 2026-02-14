use crate::commands::agent::run::helpers::{system_message, user_message};
use crate::commands::agent::run::stream::ToolCallAccumulator;
use crate::config::AppConfig;
use agent_client_protocol::{
    self as acp, Client as AcpClient, ModelInfo, SessionModelState, SessionNotification,
    SetSessionModelRequest, SetSessionModelResponse,
};
use futures_util::StreamExt;
use stakpak_api::models::ApiStreamError;
use stakpak_api::storage::CreateSessionRequest;
use stakpak_api::{AgentClient, AgentClientConfig, AgentProvider, StakpakConfig};
use stakpak_api::{Model, ModelLimit};
use stakpak_mcp_client::McpClient;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::{
    ChatCompletionChoice, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage,
    FinishReason, MessageContent, Role, Tool, ToolCall, ToolCallResultProgress,
    ToolCallResultStatus,
};
use stakpak_shared::models::llm::LLMTokenUsage;
use std::cell::Cell;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use uuid::Uuid;

pub struct StakpakAcpAgent {
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    client: Arc<tokio::sync::RwLock<Arc<dyn AgentProvider>>>,
    /// Default model to use for chat completions
    model: Arc<tokio::sync::RwLock<Model>>,
    session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    next_session_id: Cell<u64>,
    mcp_client: Option<Arc<McpClient>>,
    mcp_tools: Vec<rmcp::model::Tool>,
    tools: Option<Vec<Tool>>,
    current_session_id: Cell<Option<Uuid>>,
    progress_tx: Option<mpsc::Sender<ToolCallResultProgress>>,
    // Add persistent message history for conversation context
    messages: Arc<tokio::sync::Mutex<Vec<ChatMessage>>>,
    // Add permission request channel
    permission_request_tx: Option<
        mpsc::UnboundedSender<(
            acp::RequestPermissionRequest,
            oneshot::Sender<acp::RequestPermissionResponse>,
        )>,
    >,
    // Add cancellation channels for streaming and tool calls
    stream_cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    tool_cancel_tx: Option<tokio::sync::broadcast::Sender<()>>,
    // Track active tool calls for cancellation
    active_tool_calls: Arc<tokio::sync::Mutex<Vec<ToolCall>>>,
    // Store current streaming message for todo extraction
    current_streaming_message: Arc<tokio::sync::Mutex<String>>,
    // Buffer for handling partial XML tags during streaming
    streaming_buffer: Arc<tokio::sync::Mutex<String>>,
    // Channel for native ACP filesystem operations
    fs_operation_tx: Option<mpsc::UnboundedSender<crate::commands::acp::fs_handler::FsOperation>>,
    // Capabilities advertised by the client during initialization
    client_capabilities: Arc<tokio::sync::Mutex<acp::ClientCapabilities>>,
}

impl StakpakAcpAgent {
    /// Convert internal Model to ACP ModelInfo
    fn model_to_acp_model_info(model: &Model) -> ModelInfo {
        ModelInfo::new(model.id.clone(), model.name.clone())
            .description(format!("Provider: {}", model.provider))
    }

    /// Get available models as ACP SessionModelState
    async fn get_session_model_state(&self) -> SessionModelState {
        let client = self.client.read().await;
        let current_model = self.model.read().await;

        let available_models = client.list_models().await;
        log::debug!(
            "Available models for ACP: {} models, current: {}",
            available_models.len(),
            current_model.id
        );

        let acp_models: Vec<ModelInfo> = available_models
            .iter()
            .map(Self::model_to_acp_model_info)
            .collect();

        // Ensure currentModelId matches one of the availableModels
        // If the current model isn't in the list, use the first available model
        let current_model_id = if available_models.iter().any(|m| m.id == current_model.id) {
            current_model.id.clone()
        } else if let Some(first_model) = available_models.first() {
            log::debug!(
                "Current model '{}' not in available models, using '{}'",
                current_model.id,
                first_model.id
            );
            first_model.id.clone()
        } else {
            // Fallback if no models available
            current_model.id.clone()
        };

        SessionModelState::new(current_model_id, acp_models)
    }

    pub async fn new(
        config: AppConfig,
        session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
        system_prompt: Option<String>,
    ) -> Result<Self, String> {
        // Create unified AgentClient
        let client: Arc<dyn AgentProvider> = {
            let stakpak_api_key = config.get_stakpak_api_key();
            if stakpak_api_key.is_none() {
                log::warn!("No Stakpak API key found. Running in local mode.");
            }

            // Use credential resolution with auth.toml fallback chain
            let stakpak = stakpak_api_key.map(|api_key| StakpakConfig {
                api_key,
                api_endpoint: config.api_endpoint.clone(),
            });

            let client = AgentClient::new(AgentClientConfig {
                stakpak,
                providers: config.get_llm_provider_config(),
                eco_model: config.eco_model.clone(),
                recovery_model: config.recovery_model.clone(),
                smart_model: config.smart_model.clone(),
                store_path: None,
                hook_registry: None,
            })
            .await
            .map_err(|e| format!("Failed to create agent client: {}", e))?;
            Arc::new(client)
        };

        // Get default model - use smart_model from config or first available model
        let model = if let Some(smart_model_str) = &config.smart_model {
            // Parse the smart_model string to determine provider
            let provider = if smart_model_str.starts_with("anthropic/")
                || smart_model_str.contains("claude")
            {
                "anthropic"
            } else if smart_model_str.starts_with("openai/") || smart_model_str.contains("gpt") {
                "openai"
            } else if smart_model_str.starts_with("google/") || smart_model_str.contains("gemini") {
                "google"
            } else {
                "stakpak"
            };
            Model::custom(smart_model_str.clone(), provider)
        } else {
            // Use first available model from client
            let models = client.list_models().await;
            models.into_iter().next().unwrap_or_else(|| {
                // Fallback default: Claude Opus via Stakpak
                Model::new(
                    "anthropic/claude-opus-4-5",
                    "Claude Opus 4.5",
                    "stakpak",
                    true,
                    None,
                    ModelLimit::default(),
                )
            })
        };

        // Initialize MCP client and tools (optional for ACP)
        let (mcp_client, mcp_tools, tools) =
            match Self::initialize_mcp_server_and_tools(&config).await {
                Ok(result) => {
                    log::info!("MCP client initialized successfully");
                    // Hold shutdown handles to keep servers alive
                    // They'll be dropped when this agent is dropped, which is fine
                    // since new() is only called once at startup and run_stdio reinitializes
                    let _server_shutdown = result.server_shutdown_tx;
                    let _proxy_shutdown = result.proxy_shutdown_tx;
                    (Some(result.client), result.mcp_tools, result.tools)
                }
                Err(e) => {
                    log::warn!(
                        "Failed to initialize MCP client: {}, continuing without tools",
                        e
                    );
                    (None, Vec::new(), Vec::new())
                }
            };

        // Create cancellation channels
        let (stream_cancel_tx, _) = tokio::sync::broadcast::channel(1);
        let (tool_cancel_tx, _) = tokio::sync::broadcast::channel(1);

        let messages = match system_prompt {
            Some(system_prompt) => vec![system_message(system_prompt)],
            None => Vec::new(),
        };

        Ok(Self {
            config: Arc::new(tokio::sync::RwLock::new(config)),
            client: Arc::new(tokio::sync::RwLock::new(client)),
            model: Arc::new(tokio::sync::RwLock::new(model)),
            session_update_tx,
            next_session_id: Cell::new(0),
            mcp_client,
            mcp_tools,
            tools: if tools.is_empty() { None } else { Some(tools) },
            current_session_id: Cell::new(None),
            progress_tx: None,
            messages: Arc::new(tokio::sync::Mutex::new(messages)),
            permission_request_tx: None,
            stream_cancel_tx: Some(stream_cancel_tx),
            tool_cancel_tx: Some(tool_cancel_tx),
            active_tool_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            current_streaming_message: Arc::new(tokio::sync::Mutex::new(String::new())),
            streaming_buffer: Arc::new(tokio::sync::Mutex::new(String::new())),
            fs_operation_tx: None,
            client_capabilities: Arc::new(tokio::sync::Mutex::new(
                acp::ClientCapabilities::default(),
            )),
        })
    }

    async fn client_fs_capabilities(&self) -> (bool, bool) {
        let caps = self.client_capabilities.lock().await;
        (caps.fs.read_text_file, caps.fs.write_text_file)
    }

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
                SessionNotification::new(
                    session_id.clone(),
                    acp::SessionUpdate::ToolCall(
                        acp::ToolCall::new(acp::ToolCallId::new(tool_call_id), title)
                            .kind(*kind)
                            .status(acp::ToolCallStatus::Pending)
                            .content(content.unwrap_or_default())
                            .locations(locations.unwrap_or_default())
                            .raw_input(raw_input),
                    ),
                ),
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
                SessionNotification::new(
                    session_id.clone(),
                    acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                        acp::ToolCallId::new(tool_call_id),
                        acp::ToolCallUpdateFields::new()
                            .status(status)
                            .content(content)
                            .raw_output(raw_output),
                    )),
                ),
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
            acp::PermissionOption::new(
                acp::PermissionOptionId::new("allow"),
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::new("reject"),
                "Reject",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ];

        // Create the permission request
        let permission_request = acp::RequestPermissionRequest::new(
            session_id.clone(),
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new(tool_call_id.clone()),
                acp::ToolCallUpdateFields::new()
                    .title(tool_title.to_string())
                    .raw_input(
                        serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::Value::Null),
                    ),
            ),
            options,
        );

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
                    acp::RequestPermissionOutcome::Selected(outcome) => {
                        log::info!("User selected permission option: {}", outcome.option_id.0);
                        Ok(outcome.option_id.0.as_ref() == "allow"
                            || outcome.option_id.0.as_ref() == "allow_always")
                    }
                    acp::RequestPermissionOutcome::Cancelled => {
                        log::info!("Permission request was cancelled");
                        Ok(false)
                    }
                    _ => {
                        log::warn!("Unknown permission outcome");
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
        use super::tool_names;
        match tool_name {
            tool_names::VIEW => {
                // Extract path from arguments for view tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Read {}", path)
                } else {
                    "Read".to_string()
                }
            }
            tool_names::RUN_COMMAND => {
                // Extract command from arguments for run_command tool
                if let Some(command) = raw_input.get("command").and_then(|c| c.as_str()) {
                    format!("Run command {}", command)
                } else {
                    "Run command".to_string()
                }
            }
            tool_names::CREATE | tool_names::CREATE_FILE => {
                // Extract path from arguments for create tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Creating {}", path)
                } else {
                    "Creating".to_string()
                }
            }
            tool_names::STR_REPLACE | tool_names::EDIT_FILE => {
                // Extract path from arguments for edit tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Editing {}", path)
                } else {
                    "Editing".to_string()
                }
            }
            tool_names::DELETE_FILE => {
                // Extract path from arguments for delete tool
                if let Some(path) = raw_input.get("path").and_then(|p| p.as_str()) {
                    format!("Deleting {}", path)
                } else {
                    "Deleting".to_string()
                }
            }
            tool_names::SEARCH_DOCS => {
                // Extract query from arguments for search tool
                if let Some(query) = raw_input.get("query").and_then(|q| q.as_str()) {
                    format!("Search docs: {}", query)
                } else {
                    "Search docs".to_string()
                }
            }
            tool_names::LOCAL_CODE_SEARCH => {
                // Extract query from arguments for search tool
                if let Some(query) = raw_input.get("query").and_then(|q| q.as_str()) {
                    format!("Search local context: {}", query)
                } else {
                    "Search local context".to_string()
                }
            }
            tool_names::READ_RULEBOOK => "Read rulebook".to_string(),
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
        use super::tool_names;
        if tool_names::is_fs_file_read(tool_name) || tool_name == tool_names::READ_RULEBOOK {
            acp::ToolKind::Read
        } else if tool_names::is_fs_file_write(tool_name) {
            acp::ToolKind::Edit
        } else if tool_name == tool_names::RUN_COMMAND {
            acp::ToolKind::Execute
        } else if tool_name == tool_names::DELETE_FILE {
            acp::ToolKind::Delete
        } else if tool_name == tool_names::SEARCH_DOCS || tool_name == tool_names::LOCAL_CODE_SEARCH
        {
            acp::ToolKind::Search
        } else {
            acp::ToolKind::Other
        }
    }

    // Helper method to determine if a tool should use Diff content type
    fn should_use_diff_content(&self, tool_name: &str) -> bool {
        super::tool_names::is_fs_file_write(tool_name)
    }

    // Helper method to determine if a tool is a file creation tool
    fn is_file_creation_tool(&self, tool_name: &str) -> bool {
        tool_name == super::tool_names::CREATE || tool_name == super::tool_names::CREATE_FILE
    }

    // Helper method to determine if a tool should be auto-approved
    fn is_auto_approved_tool(&self, tool_name: &str) -> bool {
        super::tool_names::is_auto_approved(tool_name)
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

    // Helper method to extract todos from the current streaming message
    fn extract_todos(&self) -> (Vec<String>, Vec<String>) {
        let current_message = {
            let message = self.current_streaming_message.try_lock();
            match message {
                Ok(msg) => msg.clone(),
                Err(_) => return (Vec::new(), Vec::new()), // Return empty if lock fails
            }
        };

        if current_message.trim().is_empty() {
            return (Vec::new(), Vec::new());
        }

        let mut todos = Vec::new();
        let mut completed_todos = Vec::new();

        // Extract todos from XML format: <scratchpad><todo>...</todo></scratchpad>
        if let Some(todo_content) = self.extract_todos_from_xml(&current_message) {
            // Parse the todo content line by line
            for line in todo_content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Check for markdown-style todos: - [ ] or - [x]
                if line.starts_with("- [ ]") {
                    let todo_text = line.strip_prefix("- [ ]").unwrap_or("").trim().to_string();
                    if !todo_text.is_empty() {
                        todos.push(todo_text);
                    }
                } else if line.starts_with("- [x]") {
                    let todo_text = line.strip_prefix("- [x]").unwrap_or("").trim().to_string();
                    if !todo_text.is_empty() {
                        completed_todos.push(todo_text);
                    }
                }
            }
        }

        (todos, completed_todos)
    }

    // Helper method to extract todo content from XML format
    fn extract_todos_from_xml(&self, message: &str) -> Option<String> {
        // Look for <todo>...</todo> pattern using case-insensitive matching
        let message_lower = message.to_lowercase();
        if let Some(start) = message_lower.find("<todo>")
            && let Some(end) = message_lower[start..].find("</todo>")
        {
            let todo_start = start + 6; // Length of "<todo>"
            let todo_end = start + end;
            return Some(message[todo_start..todo_end].trim().to_string());
        }

        None
    }

    // Helper method to extract todos and convert them to ACP plan entries
    fn extract_todos_as_plan_entries(&self, message: &str) -> Vec<acp::PlanEntry> {
        let mut plan_entries = Vec::new();

        if let Some(todo_content) = self.extract_todos_from_xml(message) {
            // Parse the todo content line by line
            for line in todo_content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Check for markdown-style todos: - [ ] or - [x]
                if line.starts_with("- [ ]") {
                    let todo_text = line.strip_prefix("- [ ]").unwrap_or("").trim().to_string();
                    if !todo_text.is_empty() {
                        plan_entries.push(acp::PlanEntry::new(
                            todo_text,
                            acp::PlanEntryPriority::Medium,
                            acp::PlanEntryStatus::Pending,
                        ));
                    }
                } else if line.starts_with("- [x]") {
                    let todo_text = line.strip_prefix("- [x]").unwrap_or("").trim().to_string();
                    if !todo_text.is_empty() {
                        plan_entries.push(acp::PlanEntry::new(
                            todo_text,
                            acp::PlanEntryPriority::Medium,
                            acp::PlanEntryStatus::Completed,
                        ));
                    }
                }
            }
        }

        plan_entries
    }

    // Helper method to send agent plan session update
    async fn send_agent_plan(
        &self,
        session_id: &acp::SessionId,
        plan_entries: Vec<acp::PlanEntry>,
    ) -> Result<(), acp::Error> {
        if plan_entries.is_empty() {
            return Ok(());
        }

        let entries_count = plan_entries.len();
        let (tx, rx) = oneshot::channel();
        self.session_update_tx
            .send((
                SessionNotification::new(
                    session_id.clone(),
                    acp::SessionUpdate::Plan(acp::Plan::new(plan_entries)),
                ),
                tx,
            ))
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?;

        log::info!("Sent agent plan with {} entries", entries_count);
        Ok(())
    }

    // Process streaming content with buffering to handle partial XML tags
    async fn process_streaming_content(
        &self,
        content: &str,
        checkpoint_regex: &Option<regex::Regex>,
    ) -> String {
        // First, filter out checkpoint IDs from the incoming content
        let filtered_content = if content.contains("<checkpoint_id>") {
            if let Some(regex) = checkpoint_regex {
                regex.replace_all(content, "").to_string()
            } else {
                content
                    .replace("<checkpoint_id>", "")
                    .replace("</checkpoint_id>", "")
            }
        } else {
            content.to_string()
        };

        // Use buffering to handle partial XML tags
        let (ready_content, held_back) = {
            let mut buffer = self.streaming_buffer.lock().await;
            buffer.push_str(&filtered_content);

            // Extract content that's safe to process (doesn't end with partial XML tag)
            self.extract_safe_content(&buffer)
        };

        // Update buffer with held back content
        {
            let mut buffer = self.streaming_buffer.lock().await;
            *buffer = held_back;
        }

        // Use pattern-based conversion for the 4 specific tags
        crate::commands::acp::utils::process_all_xml_patterns(&ready_content)
    }

    // Extract content that's safe to process, holding back potential partial XML tags
    fn extract_safe_content(&self, buffer: &str) -> (String, String) {
        // Define the XML tags we need to watch for
        let xml_tags = [
            "<scratchpad>",
            "<todo>",
            "<local_context>",
            "<rulebooks>",
            "</scratchpad>",
            "</todo>",
            "</local_context>",
            "</rulebooks>",
        ];

        // Find the last '<' character
        if let Some(last_lt_pos) = buffer.rfind('<') {
            let remaining = &buffer[last_lt_pos..];

            // If the remaining part contains '>', it's a complete tag - process everything
            if remaining.contains('>') {
                return (buffer.to_string(), String::new());
            }

            // Check if this could be the start of any XML tag (partial match)
            // Only hold back if it's actually a partial match of our specific tags
            let is_partial_match = xml_tags
                .iter()
                .any(|tag| remaining.len() < tag.len() && tag.starts_with(remaining));

            if is_partial_match {
                // Hold back only the potential partial tag
                let safe_content = buffer[..last_lt_pos].to_string();
                let held_back = remaining.to_string();
                return (safe_content, held_back);
            } else {
                // Not a partial match of our tags, process everything
                return (buffer.to_string(), String::new());
            }
        }

        // No '<' found, process everything
        (buffer.to_string(), String::new())
    }

    // Flush any remaining content from the buffer (called at end of stream)
    async fn flush_streaming_buffer(&self) -> String {
        let buffer_content = {
            let mut buffer = self.streaming_buffer.lock().await;
            let content = buffer.clone();
            buffer.clear();
            content
        };

        if !buffer_content.is_empty() {
            // Process any remaining content
            crate::commands::acp::utils::process_all_xml_patterns(&buffer_content)
        } else {
            String::new()
        }
    }

    // Process tool calls with cancellation support
    async fn process_tool_calls_with_cancellation(
        &self,
        tool_calls: Vec<ToolCall>,
        session_id: &acp::SessionId,
    ) -> Result<Vec<ChatMessage>, acp::Error> {
        log::info!("Processing {} tool calls", tool_calls.len());

        let mut tool_calls_queue = tool_calls;
        let mut results = Vec::new();

        // Create cancellation receiver for tool calls
        let mut cancel_rx = self.tool_cancel_tx.as_ref().map(|tx| tx.subscribe());

        while !tool_calls_queue.is_empty() {
            // Check for cancellation before processing each tool call
            if let Some(cancel_rx) = &mut cancel_rx {
                // Use try_recv to check for cancellation without blocking
                if cancel_rx.try_recv().is_ok() {
                    log::info!("Tool call processing cancelled");
                    // Add cancellation messages for remaining tool calls
                    for tool_call in tool_calls_queue {
                        results.push(crate::commands::agent::run::helpers::tool_result(
                            tool_call.id.clone(),
                            "TOOL_CALL_CANCELLED".to_string(),
                        ));
                    }
                    return Ok(results);
                }
            }

            let tool_call = tool_calls_queue.remove(0);
            let tool_call_id = self.generate_tool_call_id();

            log::info!(
                "🔧 DEBUG: Processing tool call: {} (original_id: {}, new_id: {})",
                tool_call.function.name,
                tool_call.id,
                tool_call_id
            );

            // Track active tool call for cancellation
            {
                let mut active_tool_calls = self.active_tool_calls.lock().await;
                active_tool_calls.push(tool_call.clone());
            }
            let raw_input = serde_json::from_str(&tool_call.function.arguments)
                .unwrap_or(serde_json::Value::Null);
            let stripped_name =
                crate::commands::acp::utils::strip_tool_name(&tool_call.function.name);
            let tool_title = self.generate_tool_title(stripped_name, &raw_input);
            let tool_kind = self.get_tool_kind(stripped_name);

            // Prepare content and locations for diff tools
            let file_path = raw_input
                .get("path")
                .and_then(|p| p.as_str())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("unknown"));

            // Extract old_str and new_str for editing tools
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

            let (content, locations) = if self.should_use_diff_content(stripped_name) {
                if self.is_file_creation_tool(stripped_name) {
                    // For file creation: old_text = None, new_text = result_content
                    let diff_content = vec![acp::ToolCallContent::Diff(acp::Diff::new(
                        file_path.clone(),
                        "",
                    ))];
                    let tool_locations =
                        vec![acp::ToolCallLocation::new(file_path.clone()).line(0u32)];
                    (Some(diff_content), Some(tool_locations))
                } else {
                    // For file editing: use extracted old_string and new_string
                    let diff_content = vec![acp::ToolCallContent::Diff(
                        acp::Diff::new(file_path.clone(), new_string.unwrap_or_default())
                            .old_text(old_string),
                    )];
                    let tool_locations =
                        vec![acp::ToolCallLocation::new(file_path.clone()).line(0u32)];
                    (Some(diff_content), Some(tool_locations))
                }
            } else {
                (None, None)
            };

            // Send tool call notification
            let proper_raw_input = self.create_raw_input(&raw_input, &abs_path);
            self.send_tool_call_notification(
                session_id,
                tool_call_id.clone(),
                tool_title.clone(),
                &tool_kind,
                proper_raw_input,
                content,
                locations,
            )
            .await?;

            // Check permissions
            let permission_granted = if self.is_auto_approved_tool(stripped_name) {
                true
            } else {
                self.send_permission_request(
                    session_id,
                    tool_call_id.clone(),
                    &tool_call,
                    &tool_title,
                )
                .await?
            };

            if !permission_granted {
                // Send rejection notification
                self.send_tool_call_update(
                    session_id,
                    tool_call_id.clone(),
                    acp::ToolCallStatus::Failed,
                    Some(vec![acp::ToolCallContent::Content(acp::Content::new(
                        acp::ContentBlock::Text(acp::TextContent::new(
                            "Tool execution rejected by user",
                        )),
                    ))]),
                    None,
                )
                .await?;

                // Add rejection message to conversation history (like interactive mode)
                results.push(crate::commands::agent::run::helpers::tool_result(
                    tool_call.id.clone(),
                    "TOOL_CALL_REJECTED".to_string(),
                ));

                // Continue to next tool call (the rejected one is already removed from queue)
                continue;
            }

            // Update status to in progress
            self.send_tool_call_update(
                session_id,
                tool_call_id.clone(),
                acp::ToolCallStatus::InProgress,
                None,
                None,
            )
            .await?;

            // Check if this is a filesystem tool that should use native ACP
            // Decide if this should be handled by native ACP FS. Avoid read_text_file for directories.
            let is_view_directory = if stripped_name == super::tool_names::VIEW {
                Path::new(&abs_path).is_dir()
            } else {
                false
            };

            let is_read_tool =
                super::tool_names::is_fs_file_read(stripped_name) && !is_view_directory;
            let is_write_tool = super::tool_names::is_fs_file_write(stripped_name);

            // Delegate fs operations to the client so it can access unsaved editor
            // state and track modifications. Per ACP spec, both read and write
            // require the client to advertise the corresponding capability.
            let (client_reads, client_writes) = self.client_fs_capabilities().await;
            let should_delegate = self.fs_operation_tx.is_some()
                && ((is_read_tool && client_reads) || (is_write_tool && client_writes));

            let result = if should_delegate {
                log::info!(
                    "🔧 DEBUG: Executing filesystem tool via native ACP: {}",
                    tool_call.function.name
                );

                // Execute using native ACP filesystem protocol
                let fs_tx = self
                    .fs_operation_tx
                    .as_ref()
                    .ok_or_else(acp::Error::internal_error)?;
                crate::commands::acp::fs_handler::execute_acp_fs_tool(fs_tx, &tool_call, session_id)
                    .await
                    .map_err(|e| {
                        log::error!("ACP filesystem tool execution failed: {e}");
                        acp::Error::internal_error().data(format!("Tool execution failed: {e}"))
                    })?
            } else if let Some(ref mcp_client) = self.mcp_client {
                log::info!(
                    "Executing tool call: {} with MCP client",
                    tool_call.function.name
                );

                // Create cancellation receiver for this tool call
                let tool_cancel_rx = self.tool_cancel_tx.as_ref().map(|tx| tx.subscribe());

                crate::commands::agent::run::tooling::run_tool_call(
                    mcp_client,
                    &self.mcp_tools,
                    &tool_call,
                    tool_cancel_rx,
                    self.current_session_id.get(),
                    Some(self.model.read().await.id.clone()),
                )
                .await
                .map_err(|e| {
                    log::error!("MCP tool execution failed: {e}");
                    acp::Error::internal_error().data(format!("MCP tool execution failed: {e}"))
                })?
            } else {
                let error_msg = format!(
                    "No execution method available for tool: {}",
                    tool_call.function.name
                );
                log::error!("{error_msg}");
                return Err(acp::Error::internal_error().data(error_msg));
            };

            if let Some(tool_result) = result {
                // Check if the tool call was cancelled
                if CallToolResultExt::get_status(&tool_result) == ToolCallResultStatus::Cancelled {
                    // Send cancellation notification
                    self.send_tool_call_update(
                        session_id,
                        tool_call_id.clone(),
                        acp::ToolCallStatus::Failed,
                        Some(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "Tool call cancelled by user",
                            )),
                        ))]),
                        Some(serde_json::json!({
                            "success": false,
                            "cancelled": true
                        })),
                    )
                    .await?;

                    // Add cancellation message to conversation history
                    results.push(crate::commands::agent::run::helpers::tool_result(
                        tool_call.id.clone(),
                        "TOOL_CALL_CANCELLED".to_string(),
                    ));

                    // Remove cancelled tool call from active list
                    {
                        let mut active_tool_calls = self.active_tool_calls.lock().await;
                        active_tool_calls.retain(|tc| tc.id != tool_call.id);
                    }

                    // Stop processing remaining tool calls
                    return Ok(results);
                }

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
                let completion_content = if self.should_use_diff_content(stripped_name) {
                    // For diff tools, we already sent the diff in the initial notification
                    // Just send a simple completion without additional content
                    None
                } else {
                    // For non-diff tools, send the result content
                    Some(vec![acp::ToolCallContent::Content(acp::Content::new(
                        acp::ContentBlock::Text(acp::TextContent::new(result_content.clone())),
                    ))])
                };

                self.send_tool_call_update(
                    session_id,
                    tool_call_id.clone(),
                    acp::ToolCallStatus::Completed,
                    completion_content,
                    Some(serde_json::json!({
                        "result": result_content,
                        "success": true
                    })),
                )
                .await?;

                // Add tool result to conversation history
                results.push(crate::commands::agent::run::helpers::tool_result(
                    tool_call.id.clone(),
                    result_content,
                ));

                // Remove completed tool call from active list
                {
                    let mut active_tool_calls = self.active_tool_calls.lock().await;
                    active_tool_calls.retain(|tc| tc.id != tool_call.id);
                }

                // Check for cancellation after tool execution
                if let Some(tx) = &self.tool_cancel_tx {
                    let mut fresh_cancel_rx = tx.subscribe();
                    if fresh_cancel_rx.try_recv().is_ok() {
                        log::info!("Tool call processing cancelled after execution");
                        // Add cancellation messages for remaining tool calls
                        for remaining_tool_call in tool_calls_queue {
                            results.push(crate::commands::agent::run::helpers::tool_result(
                                remaining_tool_call.id.clone(),
                                "TOOL_CALL_CANCELLED".to_string(),
                            ));
                        }
                        return Ok(results);
                    }
                }
            } else {
                // Tool execution failed - send failure notification
                self.send_tool_call_update(
                    session_id,
                    tool_call_id.clone(),
                    acp::ToolCallStatus::Failed,
                    Some(vec![acp::ToolCallContent::Content(acp::Content::new(
                        acp::ContentBlock::Text(acp::TextContent::new(
                            "Tool execution failed - no result returned",
                        )),
                    ))]),
                    Some(serde_json::json!({
                        "success": false,
                        "error": "No result returned"
                    })),
                )
                .await?;

                // Add failure message to conversation history
                results.push(crate::commands::agent::run::helpers::tool_result(
                    tool_call.id.clone(),
                    "Tool execution failed - no result returned".to_string(),
                ));

                // Remove failed tool call from active list
                {
                    let mut active_tool_calls = self.active_tool_calls.lock().await;
                    active_tool_calls.retain(|tc| tc.id != tool_call.id);
                }
            }
        }

        Ok(results)
    }

    pub async fn initialize_mcp_server_and_tools(
        config: &AppConfig,
    ) -> Result<crate::commands::agent::run::mcp_init::McpInitResult, String> {
        use crate::commands::agent::run::mcp_init::{
            McpInitConfig, initialize_mcp_server_and_tools,
        };

        let mcp_config = McpInitConfig::default();

        initialize_mcp_server_and_tools(config, mcp_config, None).await
    }

    async fn process_acp_streaming_response_with_cancellation(
        &self,
        stream: impl futures_util::Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>>,
        session_id: &acp::SessionId,
    ) -> Result<ChatCompletionResponse, String> {
        let mut stream = Box::pin(stream);
        let current_model = self.model.read().await;

        let mut chat_completion_response = ChatCompletionResponse {
            id: "".to_string(),
            object: "".to_string(),
            created: 0,
            model: current_model.id.clone(),
            choices: vec![],
            usage: LLMTokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                prompt_tokens_details: None,
            },
            system_fingerprint: None,
            metadata: None,
        };

        let mut chat_message = ChatMessage {
            role: Role::Assistant,
            content: None,
            name: None,
            tool_calls: None,
            tool_call_id: None,
            usage: None,
            ..Default::default()
        };

        let mut tool_call_accumulator = ToolCallAccumulator::new();

        // Compile regex once outside the loop
        let checkpoint_regex = regex::Regex::new(r"<checkpoint_id>.*?</checkpoint_id>").ok();

        // Create cancellation receiver
        let mut cancel_rx = self.stream_cancel_tx.as_ref().map(|tx| tx.subscribe());

        // Clear the current streaming message and buffer at the start
        {
            let mut current_message = self.current_streaming_message.lock().await;
            *current_message = String::new();
        }
        {
            let mut buffer = self.streaming_buffer.lock().await;
            *buffer = String::new();
        }

        loop {
            // Race between stream processing and cancellation
            let result = if let Some(ref mut cancel_rx) = cancel_rx {
                tokio::select! {
                    response = stream.next() => response,
                    _ = cancel_rx.recv() => {
                        log::info!("Stream processing cancelled");
                        return Err("STREAM_CANCELLED".to_string());
                    }
                }
            } else {
                stream.next().await
            };

            let response = match result {
                Some(response) => response,
                None => break, // Stream ended
            };

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
                        usage: LLMTokenUsage {
                            prompt_tokens: 0,
                            completion_tokens: 0,
                            total_tokens: 0,
                            prompt_tokens_details: None,
                        },
                        system_fingerprint: None,
                        metadata: None,
                    };

                    if let Some(content) = &delta.content {
                        chat_message.content =
                            Some(MessageContent::String(match chat_message.content {
                                Some(MessageContent::String(old_content)) => old_content + content,
                                _ => content.clone(),
                            }));

                        // Accumulate the raw content in the current streaming message BEFORE filtering
                        {
                            let mut current_message = self.current_streaming_message.lock().await;
                            current_message.push_str(content);
                        }

                        // Extract and send agent plan from current streaming message
                        let current_message = {
                            let message = self.current_streaming_message.lock().await;
                            message.clone()
                        };
                        let plan_entries = self.extract_todos_as_plan_entries(&current_message);
                        if !plan_entries.is_empty()
                            && let Err(e) = self.send_agent_plan(session_id, plan_entries).await
                        {
                            log::warn!("Failed to send agent plan during streaming: {}", e);
                            // Don't fail the streaming if plan sending fails
                        }

                        // Process streaming content with buffering for partial XML tags
                        let filtered_content = self
                            .process_streaming_content(content, &checkpoint_regex)
                            .await;

                        // Only send non-empty content after filtering
                        if !filtered_content.trim().is_empty() {
                            // Send streaming chunk to ACP client
                            let (tx, rx) = oneshot::channel();
                            self.session_update_tx
                                .send((
                                    SessionNotification::new(
                                        session_id.clone(),
                                        acp::SessionUpdate::AgentMessageChunk(
                                            acp::ContentChunk::new(acp::ContentBlock::Text(
                                                acp::TextContent::new(filtered_content),
                                            )),
                                        ),
                                    ),
                                    tx,
                                ))
                                .map_err(|_| "Failed to send streaming chunk")?;
                            rx.await.map_err(|_| "Failed to await streaming chunk")?;
                        }
                    }

                    // Handle tool calls streaming
                    if let Some(tool_calls) = &delta.tool_calls {
                        for delta_tool_call in tool_calls {
                            tool_call_accumulator.process_delta(delta_tool_call);
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("Stream error: {:?}", e));
                }
            }
        }

        // Flush any remaining content from the buffer at the end of the stream
        let flushed_content = self.flush_streaming_buffer().await;
        if !flushed_content.trim().is_empty() {
            // Send the flushed content
            let (tx, rx) = oneshot::channel();
            self.session_update_tx
                .send((
                    SessionNotification::new(
                        session_id.clone(),
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                            acp::ContentBlock::Text(acp::TextContent::new(flushed_content)),
                        )),
                    ),
                    tx,
                ))
                .map_err(|_| "Failed to send flushed content")?;
            rx.await.map_err(|_| "Failed to await flushed content")?;
        }

        // Get accumulated tool calls (already filtered for empty IDs)
        let final_tool_calls = tool_call_accumulator.into_tool_calls();
        chat_message.tool_calls = if final_tool_calls.is_empty() {
            None
        } else {
            Some(final_tool_calls)
        };

        chat_completion_response.choices.push(ChatCompletionChoice {
            index: 0,
            message: chat_message.clone(),
            finish_reason: FinishReason::Stop,
            logprobs: None,
        });

        Ok(chat_completion_response)
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
                let (progress_tx, mut progress_rx) =
                    tokio::sync::mpsc::channel::<ToolCallResultProgress>(100);

                // Reinitialize MCP client with progress channel (in-process server + proxy)
                let config_snapshot = self.config.read().await.clone();
                let (mcp_client, mcp_tools, tools, _mcp_server_shutdown, _mcp_proxy_shutdown) =
                    match Self::initialize_mcp_server_and_tools(&config_snapshot).await {
                        Ok(result) => {
                            log::info!("MCP client reinitialized in run_stdio");
                            (
                                Some(result.client),
                                result.mcp_tools,
                                result.tools,
                                Some(result.server_shutdown_tx),
                                Some(result.proxy_shutdown_tx),
                            )
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to reinitialize MCP client: {}, continuing without tools",
                                e
                            );
                            (None, Vec::new(), Vec::new(), None, None)
                        }
                    };

                // Create permission request channel
                let (permission_tx, mut permission_rx) = mpsc::unbounded_channel::<(
                    acp::RequestPermissionRequest,
                    oneshot::Sender<acp::RequestPermissionResponse>,
                )>();

                // Create filesystem operation channel for native ACP filesystem operations
                let (fs_operation_tx, fs_operation_rx) =
                    mpsc::unbounded_channel::<crate::commands::acp::fs_handler::FsOperation>();

                // Create a new agent with the proper channel
                let agent = StakpakAcpAgent {
                    config: self.config.clone(),
                    client: self.client.clone(),
                    model: self.model.clone(),
                    session_update_tx: tx.clone(),
                    next_session_id: self.next_session_id.clone(),
                    mcp_client,
                    mcp_tools,
                    tools: if tools.is_empty() { None } else { Some(tools) },
                    current_session_id: self.current_session_id.clone(),
                    progress_tx: Some(progress_tx),
                    messages: self.messages.clone(),
                    permission_request_tx: Some(permission_tx),
                    stream_cancel_tx: self.stream_cancel_tx.clone(),
                    tool_cancel_tx: self.tool_cancel_tx.clone(),
                    active_tool_calls: self.active_tool_calls.clone(),
                    current_streaming_message: self.current_streaming_message.clone(),
                    streaming_buffer: self.streaming_buffer.clone(),
                    fs_operation_tx: Some(fs_operation_tx),
                    client_capabilities: self.client_capabilities.clone(),
                };

                // Start up the StakpakAcpAgent connected to stdio.
                let (conn, handle_io) =
                    acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                        tokio::task::spawn_local(fut);
                    });

                // Wrap connection in Arc for sharing
                let conn_arc = Arc::new(conn);

                // Spawn filesystem handler for native ACP filesystem operations
                crate::commands::acp::fs_handler::spawn_fs_handler(
                    conn_arc.clone(),
                    fs_operation_rx,
                );

                // Start a background task to send session notifications to the client
                let conn_for_notifications = conn_arc.clone();
                tokio::task::spawn_local(async move {
                    while let Some((session_notification, ack_tx)) = rx.recv().await {
                        log::info!("Sending session notification: {:?}", session_notification);
                        let result = AcpClient::session_notification(
                            &*conn_for_notifications,
                            session_notification,
                        )
                        .await;
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
                        match conn_for_permissions
                            .request_permission(permission_request)
                            .await
                        {
                            Ok(response) => {
                                log::info!("Permission request response: {:?}", response);
                                let _ = response_tx.send(response);
                            }
                            Err(e) => {
                                log::error!("Permission request failed: {}", e);
                                // Send a default rejection response
                                let _ = response_tx.send(acp::RequestPermissionResponse::new(
                                    acp::RequestPermissionOutcome::Cancelled,
                                ));
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
                        if session_update_tx_clone
                            .send((
                                SessionNotification::new(
                                    acp::SessionId::new(""), // TODO: Get actual session ID
                                    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                                        acp::ContentBlock::Text(acp::TextContent::new(
                                            progress.message,
                                        )),
                                    )),
                                ),
                                tx,
                            ))
                            .is_err()
                        {
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
            model: self.model.clone(),
            session_update_tx: self.session_update_tx.clone(),
            next_session_id: Cell::new(self.next_session_id.get()),
            mcp_client: self.mcp_client.clone(),
            mcp_tools: self.mcp_tools.clone(),
            tools: self.tools.clone(),
            current_session_id: Cell::new(self.current_session_id.get()),
            progress_tx: self.progress_tx.clone(),
            messages: self.messages.clone(),
            permission_request_tx: self.permission_request_tx.clone(),
            stream_cancel_tx: self.stream_cancel_tx.clone(),
            tool_cancel_tx: self.tool_cancel_tx.clone(),
            active_tool_calls: self.active_tool_calls.clone(),
            current_streaming_message: self.current_streaming_message.clone(),
            streaming_buffer: self.streaming_buffer.clone(),
            fs_operation_tx: self.fs_operation_tx.clone(),
            client_capabilities: self.client_capabilities.clone(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for StakpakAcpAgent {
    async fn initialize(
        &self,
        args: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        log::info!("Received initialize request {args:?}");

        // Store client capabilities for later use
        {
            let mut caps = self.client_capabilities.lock().await;
            *caps = args.client_capabilities.clone();
        }

        // If no API key, provide an auth method for browser-based authentication
        // This implements ACP Agent Auth - the agent handles the OAuth-like flow internally
        let auth_methods = if self.config.read().await.api_key.is_none() {
            vec![acp::AuthMethod::new(
                acp::AuthMethodId::new("stakpak"),
                "Login to Stakpak",
            )
            .description("Authenticate via browser to get your Stakpak API key. A browser window will open for you to sign in.")]
        } else {
            Vec::new()
        };

        // Get version from Cargo.toml at compile time
        let version = env!("CARGO_PKG_VERSION");

        Ok(acp::InitializeResponse::new(acp::ProtocolVersion::V1)
            .agent_info(acp::Implementation::new("stakpak", version).title("Stakpak Agent"))
            .agent_capabilities(
                acp::AgentCapabilities::new()
                    .mcp_capabilities(acp::McpCapabilities::new().http(true).sse(true))
                    .load_session(true)
                    .prompt_capabilities(
                        acp::PromptCapabilities::new()
                            .image(true)
                            .audio(false)
                            .embedded_context(true),
                    ),
            )
            .auth_methods(auth_methods))
    }

    async fn authenticate(
        &self,
        args: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        log::info!("Received authenticate request {args:?}");

        let method_id = args.method_id.0.to_string();

        // Handle Stakpak authentication via browser redirect (ACP Agent Auth)
        if method_id == "stakpak" {
            log::info!("Stakpak auth method selected, initiating browser-based authentication");

            // Perform browser-based authentication
            let api_key = crate::apikey_auth::authenticate_with_browser_redirect()
                .await
                .map_err(|e| {
                    log::error!("Browser authentication failed: {}", e);
                    acp::Error::auth_required().data(e)
                })?;

            // Validate the API key format
            if !api_key.starts_with("stkpk_api") {
                log::error!("Invalid API key format received");
                return Err(acp::Error::auth_required().data("Invalid API key format".to_string()));
            }

            // Save the API key to config (both disk and in-memory)
            {
                let mut config = self.config.write().await;
                config.api_key = Some(api_key.clone());
                config.save().map_err(|e| {
                    log::error!("Failed to save API key to config: {}", e);
                    acp::Error::internal_error().data(format!("Failed to save config: {}", e))
                })?;
            }

            // Rebuild the AgentClient with the new API key so subsequent
            // requests use the authenticated Stakpak provider
            {
                let config = self.config.read().await;
                let stakpak = Some(StakpakConfig {
                    api_key: api_key.clone(),
                    api_endpoint: config.api_endpoint.clone(),
                });
                let new_client = AgentClient::new(AgentClientConfig {
                    stakpak,
                    providers: config.get_llm_provider_config(),
                    eco_model: config.eco_model.clone(),
                    recovery_model: config.recovery_model.clone(),
                    smart_model: config.smart_model.clone(),
                    store_path: None,
                    hook_registry: None,
                })
                .await
                .map_err(|e| {
                    log::error!("Failed to rebuild agent client after auth: {}", e);
                    acp::Error::internal_error().data(format!("Failed to rebuild client: {}", e))
                })?;

                let mut client = self.client.write().await;
                *client = Arc::new(new_client);
            }

            log::info!("Authentication successful, API key saved and client rebuilt");
            return Ok(acp::AuthenticateResponse::new());
        }

        // Legacy support: check for STAKPAK_API_KEY environment variable
        if method_id == "github" {
            log::info!(
                "Legacy github auth method selected, checking for STAKPAK_API_KEY environment variable"
            );
            match std::env::var("STAKPAK_API_KEY") {
                Ok(_api_key) => {
                    log::info!("STAKPAK_API_KEY found in environment");
                    return Ok(acp::AuthenticateResponse::new());
                }
                Err(_) => {
                    log::error!("STAKPAK_API_KEY environment variable is not set");
                    return Err(
                        acp::Error::auth_required().data("STAKPAK_API_KEY is not set. Use the 'stakpak' auth method for browser-based authentication.".to_string())
                    );
                }
            }
        }

        // Unknown auth method
        log::error!("Unknown authentication method: {}", method_id);
        Err(acp::Error::invalid_params().data(format!("Unknown auth method: {}", method_id)))
    }

    async fn new_session(
        &self,
        args: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        log::info!("Received new session request {args:?}");

        // Check if we have a valid API key
        // In-memory config is now always up-to-date since authenticate() updates it directly
        let config = self.config.read().await;
        let has_api_key = config.api_key.is_some() || std::env::var("STAKPAK_API_KEY").is_ok() || {
            // Try to reload config from disk as a fallback (e.g., key set externally)
            match crate::config::AppConfig::load(&config.profile_name, None::<&str>) {
                Ok(fresh_config) => {
                    if fresh_config.api_key.is_some() {
                        log::info!("Found API key in refreshed config from disk");
                        true
                    } else {
                        false
                    }
                }
                Err(_) => false,
            }
        };
        drop(config);

        if !has_api_key {
            log::error!("API key is missing - authentication required");
            return Err(acp::Error::auth_required().data(
                "Authentication required. Use the 'stakpak' auth method to authenticate via browser.".to_string()
            ));
        }

        // Clear message history for new session and keep system message
        let system_message = {
            let mut messages = self.messages.lock().await;
            let system_message = messages
                .iter()
                .find(|msg| msg.role == Role::System)
                .cloned();
            messages.clear();
            if let Some(ref sys_msg) = system_message {
                messages.push(sys_msg.clone());
            }
            system_message
        };

        // Create a cloud session to get a real session ID
        let client = self.client.read().await.clone();
        let initial_messages = if let Some(sys_msg) = system_message {
            vec![sys_msg]
        } else {
            vec![crate::commands::agent::run::helpers::user_message(
                "New session".to_string(),
            )]
        };

        let cwd = args.cwd.to_str().map(|s| s.to_string()).unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        });

        // Use project folder name as session title
        let title = std::path::Path::new(&cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| format!("ACP: {}", n))
            .unwrap_or_else(|| "ACP Session".to_string());

        let session_request = CreateSessionRequest::new(title, initial_messages).with_cwd(cwd);

        let cloud_session = client.create_session(&session_request).await.map_err(|e| {
            log::error!("Failed to create cloud session: {}", e);
            acp::Error::internal_error().data(format!("Failed to create session: {}", e))
        })?;

        let session_id = acp::SessionId::new(cloud_session.session_id.to_string());

        // Track the current session ID (now using the cloud session ID)
        self.current_session_id.set(Some(cloud_session.session_id));

        log::info!("Created cloud session: {}", cloud_session.session_id);

        // Get available models for model selection
        let model_state = self.get_session_model_state().await;

        Ok(acp::NewSessionResponse::new(session_id).models(model_state))
    }

    async fn load_session(
        &self,
        args: acp::LoadSessionRequest,
    ) -> Result<acp::LoadSessionResponse, acp::Error> {
        log::info!("Received load session request {args:?}");

        // Parse session ID from the request
        let session_id_str = args.session_id.0.to_string();
        let session_uuid = match Uuid::parse_str(&session_id_str) {
            Ok(uuid) => uuid,
            Err(_) => return Err(acp::Error::invalid_params()),
        };

        // Track the loaded session ID
        self.current_session_id.set(Some(session_uuid));

        // Get available models for model selection
        let model_state = self.get_session_model_state().await;

        log::info!("Loaded session: {}", session_id_str);
        Ok(acp::LoadSessionResponse::new().models(model_state))
    }

    async fn prompt(&self, args: acp::PromptRequest) -> Result<acp::PromptResponse, acp::Error> {
        log::info!("Received prompt request {args:?}");

        // Convert prompt to your ChatMessage format
        let prompt_text = args
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

        let client = self.client.read().await.clone();
        let model = self.model.read().await.clone();
        let session_id = self.current_session_id.get();
        let (stream, _request_id) = client
            .chat_completion_stream(
                model,
                messages,
                tools_option.clone(),
                None,
                session_id,
                None,
            )
            .await
            .map_err(|e| {
                log::error!("Chat completion stream failed: {e}");
                acp::Error::internal_error().data(format!("Chat completion failed: {e}"))
            })?;

        let response = match self
            .process_acp_streaming_response_with_cancellation(stream, &args.session_id)
            .await
        {
            Ok(response) => response,
            Err(e) => {
                if e == "STREAM_CANCELLED" {
                    log::info!("Stream was cancelled by user");
                    return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
                }
                log::error!("Stream processing failed: {e}");
                return Err(
                    acp::Error::internal_error().data(format!("Stream processing failed: {e}"))
                );
            }
        };
        log::info!(
            "Chat completion successful, response choices: {}",
            response.choices.len()
        );
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

        let content = if let Some(content) = &response.choices[0].message.content {
            match content {
                MessageContent::String(s) => {
                    log::info!("Content from chat completion: '{}'", s);
                    s.clone()
                }
                MessageContent::Array(parts) => {
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
            log::warn!("Content was empty, using fallback response");
            // Note: Fallback content would be sent during streaming if needed
        }

        // Process tool calls in a loop like interactive mode
        let mut current_messages = {
            let messages = self.messages.lock().await;
            messages.clone()
        };

        // Check if the initial response has tool calls
        let mut has_tool_calls = response.choices[0]
            .message
            .tool_calls
            .as_ref()
            .map(|tc| !tc.is_empty())
            .unwrap_or(false);

        log::info!("Initial response has tool calls: {}", has_tool_calls);

        // Create cancellation receiver for tool call processing
        let mut tool_cancel_rx = self.tool_cancel_tx.as_ref().map(|tx| tx.subscribe());

        while has_tool_calls {
            if let Some(ref mut cancel_rx) = tool_cancel_rx
                && cancel_rx.try_recv().is_ok()
            {
                log::info!("Tool call processing cancelled by user");
                // Add cancellation messages for any active tool calls
                let active_tool_calls = {
                    let mut active_tool_calls = self.active_tool_calls.lock().await;
                    let tool_calls = active_tool_calls.clone();
                    active_tool_calls.clear();
                    tool_calls
                };

                for tool_call in active_tool_calls {
                    {
                        let mut messages = self.messages.lock().await;
                        messages.push(crate::commands::agent::run::helpers::tool_result(
                            tool_call.id.clone(),
                            "TOOL_CALL_CANCELLED".to_string(),
                        ));
                    }
                }

                return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
            }
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

                log::info!("Processing {} tool calls", tool_calls.len());

                // Process tool calls with cancellation support
                let tool_results = self
                    .process_tool_calls_with_cancellation(tool_calls.clone(), &args.session_id)
                    .await
                    .map_err(|e| {
                        log::error!("Tool call processing failed: {}", e);
                        e
                    })?;

                // Check if any tool calls were cancelled in the current processing
                let has_cancelled_tool_calls = tool_results.iter().any(|msg| {
                    if let Some(MessageContent::String(text)) = &msg.content {
                        text.contains("TOOL_CALL_CANCELLED")
                    } else {
                        false
                    }
                });

                // Add tool results to conversation history
                {
                    let mut messages = self.messages.lock().await;
                    messages.extend(tool_results);
                }

                // Check for cancellation after tool call processing
                if let Some(ref mut cancel_rx) = tool_cancel_rx
                    && cancel_rx.try_recv().is_ok()
                {
                    log::info!("Tool call processing cancelled after tool execution");
                    return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
                }

                if has_cancelled_tool_calls {
                    log::info!("Tool calls were cancelled, stopping turn");
                    return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
                }

                // Make follow-up chat completion request after tool calls
                current_messages = {
                    let messages = self.messages.lock().await;
                    messages.clone()
                };

                let client = self.client.read().await.clone();
                let model = self.model.read().await.clone();
                let session_id = self.current_session_id.get();
                let (follow_up_stream, _request_id) = client
                    .chat_completion_stream(
                        model,
                        current_messages.clone(),
                        tools_option.clone(),
                        None,
                        session_id,
                        None,
                    )
                    .await
                    .map_err(|e| {
                        log::error!("Follow-up chat completion stream failed: {e}");
                        acp::Error::internal_error()
                            .data(format!("Follow-up chat completion failed: {e}"))
                    })?;

                let follow_up_response = match self
                    .process_acp_streaming_response_with_cancellation(
                        follow_up_stream,
                        &args.session_id,
                    )
                    .await
                {
                    Ok(response) => response,
                    Err(e) => {
                        if e == "STREAM_CANCELLED" {
                            log::info!("Follow-up stream was cancelled by user");
                            return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
                        }
                        log::error!("Follow-up stream processing failed: {e}");
                        return Err(acp::Error::internal_error()
                            .data(format!("Follow-up stream processing failed: {e}")));
                    }
                };

                // Add follow-up response to conversation history
                {
                    let mut messages = self.messages.lock().await;
                    messages.push(follow_up_response.choices[0].message.clone());
                }

                // Update current_messages for the next iteration
                current_messages.push(follow_up_response.choices[0].message.clone());

                // Check if the follow-up response has more tool calls
                has_tool_calls = follow_up_response.choices[0]
                    .message
                    .tool_calls
                    .as_ref()
                    .map(|tc| !tc.is_empty())
                    .unwrap_or(false);

                log::info!("Follow-up response has tool calls: {}", has_tool_calls);
            } else {
                // No tool calls in the latest message, exit the loop
                break;
            }
        }

        // Note: Content is already sent during streaming, no need to send again
        // This eliminates the redundant message sending issue

        // Extract todos from the current streaming message (for logging purposes)
        let (todos, completed_todos) = self.extract_todos();
        if !todos.is_empty() || !completed_todos.is_empty() {
            log::info!(
                "Final todo extraction: {} pending, {} completed",
                todos.len(),
                completed_todos.len()
            );
        }

        Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        log::info!("Received cancel request {args:?}");

        // Cancel streaming if channel is available
        if let Some(tx) = &self.stream_cancel_tx {
            if let Err(e) = tx.send(()) {
                log::warn!("Failed to send stream cancellation signal: {}", e);
            } else {
                log::info!("Stream cancellation signal sent");
            }
        }

        // Cancel tool execution if channel is available
        if let Some(tx) = &self.tool_cancel_tx {
            if let Err(e) = tx.send(()) {
                log::warn!("Failed to send tool cancellation signal: {}", e);
            } else {
                log::info!("Tool cancellation signal sent");
            }
        }

        // Cancel all active tool calls and add cancellation messages
        let active_tool_calls = {
            let mut active_tool_calls = self.active_tool_calls.lock().await;
            let tool_calls = active_tool_calls.clone();
            active_tool_calls.clear(); // Clear the active list
            tool_calls
        };

        let tool_calls_count = active_tool_calls.len();

        // Add cancellation messages for each active tool call
        for tool_call in active_tool_calls {
            log::info!("Cancelling tool call: {}", tool_call.function.name);

            // Add cancellation message to conversation history (like rejection logic)
            {
                let mut messages = self.messages.lock().await;
                messages.push(crate::commands::agent::run::helpers::tool_result(
                    tool_call.id.clone(),
                    "TOOL_CALL_CANCELLED".to_string(),
                ));
            }
        }

        if tool_calls_count > 0 {
            log::info!("Cancelled {} active tool calls", tool_calls_count);
        }

        Ok(())
    }

    async fn set_session_model(
        &self,
        args: SetSessionModelRequest,
    ) -> Result<SetSessionModelResponse, acp::Error> {
        log::info!("Received set_session_model request: {:?}", args);

        let model_id_str = args.model_id.0.to_string();

        // Get available models from the client
        let client = self.client.read().await;
        let available_models = client.list_models().await;

        // Find the requested model
        let selected_model = available_models
            .into_iter()
            .find(|m| m.id == model_id_str)
            .ok_or_else(|| {
                log::error!("Model not found: {}", model_id_str);
                acp::Error::invalid_params().data(format!("Model not found: {}", model_id_str))
            })?;

        // Update the current model
        {
            let mut model = self.model.write().await;
            *model = selected_model.clone();
        }

        log::info!(
            "Model switched to: {} ({})",
            selected_model.name,
            selected_model.id
        );

        Ok(SetSessionModelResponse::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::acp::tool_names;
    use stakpak_api::ModelLimit;
    use test_case::test_case;

    // Per ACP spec, agents MUST check client capabilities before delegating fs operations.
    // Both readTextFile and writeTextFile default to false - delegation requires explicit opt-in.
    //
    // Columns: tool_name, client_reads, client_writes, expected_delegate
    #[test_case("view",   true,  true,  true;  "read tool, client reads: delegate")]
    #[test_case("view",   true,  false, true;  "read tool, client reads (no writes): delegate")]
    #[test_case("view",   false, true,  false; "read tool, client no reads: fallback")]
    #[test_case("view",   false, false, false; "read tool, client no fs: fallback")]
    #[test_case("create", true,  true,  true;  "write tool, client writes: delegate")]
    #[test_case("create", false, true,  true;  "write tool, client writes (no reads): delegate")]
    #[test_case("create", true,  false, false; "write tool, client no writes: fallback")]
    #[test_case("create", false, false, false; "write tool, client no fs: fallback")]
    fn test_fs_delegation_respects_client_capabilities(
        tool_name: &str,
        client_reads: bool,
        client_writes: bool,
        expected: bool,
    ) {
        let is_read_tool = tool_names::is_fs_file_read(tool_name);
        let is_write_tool = tool_names::is_fs_file_write(tool_name);

        let should_delegate = (is_read_tool && client_reads) || (is_write_tool && client_writes);

        assert_eq!(
            should_delegate, expected,
            "tool={} (read={}, write={}), caps(r={}, w={}) => delegate={}",
            tool_name, is_read_tool, is_write_tool, client_reads, client_writes, should_delegate
        );
    }

    // Model selection tests

    #[test]
    fn test_model_to_acp_model_info_basic() {
        let model = Model::new(
            "claude-sonnet-4-5-20250514",
            "Claude Sonnet 4.5",
            "anthropic",
            true,
            None,
            ModelLimit::default(),
        );

        let model_info = StakpakAcpAgent::model_to_acp_model_info(&model);

        assert_eq!(model_info.model_id.0.as_ref(), "claude-sonnet-4-5-20250514");
        assert_eq!(model_info.name, "Claude Sonnet 4.5");
        assert!(
            model_info
                .description
                .as_ref()
                .unwrap()
                .contains("anthropic")
        );
    }

    #[test]
    fn test_model_to_acp_model_info_custom_model() {
        let model = Model::custom("gpt-4o", "openai");

        let model_info = StakpakAcpAgent::model_to_acp_model_info(&model);

        assert_eq!(model_info.model_id.0.as_ref(), "gpt-4o");
        // Custom models use ID as name
        assert_eq!(model_info.name, "gpt-4o");
        assert!(model_info.description.as_ref().unwrap().contains("openai"));
    }

    #[test]
    fn test_model_to_acp_model_info_with_provider_prefix() {
        let model = Model::new(
            "anthropic/claude-opus-4-5",
            "Claude Opus 4.5",
            "stakpak",
            true,
            None,
            ModelLimit::default(),
        );

        let model_info = StakpakAcpAgent::model_to_acp_model_info(&model);

        assert_eq!(model_info.model_id.0.as_ref(), "anthropic/claude-opus-4-5");
        assert_eq!(model_info.name, "Claude Opus 4.5");
        assert!(model_info.description.as_ref().unwrap().contains("stakpak"));
    }

    #[test]
    fn test_session_model_state_creation() {
        let models = [
            Model::new(
                "claude-sonnet-4-5",
                "Claude Sonnet 4.5",
                "anthropic",
                true,
                None,
                ModelLimit::default(),
            ),
            Model::new(
                "gpt-4o",
                "GPT-4o",
                "openai",
                false,
                None,
                ModelLimit::default(),
            ),
        ];

        let acp_models: Vec<ModelInfo> = models
            .iter()
            .map(StakpakAcpAgent::model_to_acp_model_info)
            .collect();

        let state = SessionModelState::new("claude-sonnet-4-5", acp_models);

        assert_eq!(state.current_model_id.0.as_ref(), "claude-sonnet-4-5");
        assert_eq!(state.available_models.len(), 2);
        assert_eq!(
            state.available_models[0].model_id.0.as_ref(),
            "claude-sonnet-4-5"
        );
        assert_eq!(state.available_models[1].model_id.0.as_ref(), "gpt-4o");
    }

    #[test]
    fn test_find_model_by_id() {
        let models = [
            Model::new(
                "claude-sonnet-4-5",
                "Claude Sonnet 4.5",
                "anthropic",
                true,
                None,
                ModelLimit::default(),
            ),
            Model::new(
                "gpt-4o",
                "GPT-4o",
                "openai",
                false,
                None,
                ModelLimit::default(),
            ),
            Model::new(
                "gemini-2.0-flash",
                "Gemini 2.0 Flash",
                "google",
                false,
                None,
                ModelLimit::default(),
            ),
        ];

        // Find existing model
        let found = models.iter().find(|m| m.id == "gpt-4o");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "GPT-4o");

        // Find non-existing model
        let not_found = models.iter().find(|m| m.id == "non-existent-model");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_model_selection_with_provider_prefixed_ids() {
        // Models with stakpak provider prefix format
        let models = [
            Model::new(
                "anthropic/claude-sonnet-4-5-20250514",
                "Claude Sonnet 4.5",
                "stakpak",
                true,
                None,
                ModelLimit::default(),
            ),
            Model::new(
                "openai/gpt-4o",
                "GPT-4o",
                "stakpak",
                false,
                None,
                ModelLimit::default(),
            ),
        ];

        let model_id = "anthropic/claude-sonnet-4-5-20250514";
        let found = models.iter().find(|m| m.id == model_id);

        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Claude Sonnet 4.5");
        assert_eq!(found.unwrap().provider, "stakpak");
    }

    #[test]
    fn test_new_session_response_serialization_with_models() {
        let models = [
            ModelInfo::new("anthropic/claude-sonnet-4-5", "Claude Sonnet 4.5")
                .description("Provider: stakpak".to_string()),
            ModelInfo::new("openai/gpt-4o", "GPT-4o").description("Provider: stakpak".to_string()),
        ];

        let model_state = SessionModelState::new("anthropic/claude-sonnet-4-5", models.to_vec());

        let response = acp::NewSessionResponse::new(acp::SessionId::new("test-session-123"))
            .models(model_state);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("NewSessionResponse JSON:\n{}", json);

        // Verify the JSON contains models
        assert!(
            json.contains("\"models\""),
            "JSON should contain models field"
        );
        assert!(
            json.contains("\"currentModelId\""),
            "JSON should contain currentModelId"
        );
        assert!(
            json.contains("\"availableModels\""),
            "JSON should contain availableModels"
        );
        assert!(
            json.contains("\"anthropic/claude-sonnet-4-5\""),
            "JSON should contain model ID"
        );
        assert!(
            json.contains("\"Claude Sonnet 4.5\""),
            "JSON should contain model name"
        );
    }

    #[test]
    fn test_current_model_must_match_available_models() {
        // Simulate the logic from get_session_model_state
        let available_models = [
            Model::new(
                "anthropic/claude-sonnet-4-5-20250929",
                "Claude Sonnet 4.5",
                "stakpak",
                true,
                None,
                ModelLimit::default(),
            ),
            Model::new(
                "openai/gpt-4o",
                "GPT-4o",
                "stakpak",
                false,
                None,
                ModelLimit::default(),
            ),
        ];

        // Case 1: Current model matches available
        let current_model_id = "anthropic/claude-sonnet-4-5-20250929";
        let resolved_id = if available_models.iter().any(|m| m.id == current_model_id) {
            current_model_id.to_string()
        } else if let Some(first) = available_models.first() {
            first.id.clone()
        } else {
            current_model_id.to_string()
        };
        assert_eq!(resolved_id, "anthropic/claude-sonnet-4-5-20250929");

        // Case 2: Current model doesn't match - should fallback to first
        let current_model_id = "some-unknown-model";
        let resolved_id = if available_models.iter().any(|m| m.id == current_model_id) {
            current_model_id.to_string()
        } else if let Some(first) = available_models.first() {
            first.id.clone()
        } else {
            current_model_id.to_string()
        };
        assert_eq!(resolved_id, "anthropic/claude-sonnet-4-5-20250929");
    }
}
