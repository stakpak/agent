use crate::commands::agent::run::helpers::{system_message, user_message};
use crate::utils::network;
use crate::{commands::agent::run::helpers::convert_tools_map_with_filter, config::AppConfig};
use agent_client_protocol::{self as acp, Client as AcpClient, SessionNotification};
use futures_util::StreamExt;
use stakpak_api::models::ApiStreamError;
use stakpak_api::{Client, ClientConfig};
use stakpak_mcp_client::ClientManager;
use stakpak_mcp_server::{EnabledToolsConfig, MCPServerConfig, ToolMode, start_server};
use stakpak_shared::cert_utils::CertificateChain;
use stakpak_shared::models::integrations::mcp::CallToolResultExt;
use stakpak_shared::models::integrations::openai::{
    ChatCompletionResponse, ChatCompletionStreamResponse, Role, ToolCall, ToolCallResultProgress,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;
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
    active_tool_calls:
        Arc<tokio::sync::Mutex<Vec<stakpak_shared::models::integrations::openai::ToolCall>>>,
    // Store current streaming message for todo extraction
    current_streaming_message: Arc<tokio::sync::Mutex<String>>,
    // Buffer for handling partial XML tags during streaming
    streaming_buffer: Arc<tokio::sync::Mutex<String>>,
    // Channel for native ACP filesystem operations
    fs_operation_tx: Option<mpsc::UnboundedSender<crate::commands::acp::fs_handler::FsOperation>>,
}

impl StakpakAcpAgent {
    pub async fn new(
        config: AppConfig,
        session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
        system_prompt: Option<String>,
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

        // Create cancellation channels
        let (stream_cancel_tx, _) = tokio::sync::broadcast::channel(1);
        let (tool_cancel_tx, _) = tokio::sync::broadcast::channel(1);

        let messages = match system_prompt {
            Some(system_prompt) => vec![system_message(system_prompt)],
            None => Vec::new(),
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
            messages: Arc::new(tokio::sync::Mutex::new(messages)),
            permission_request_tx: None,
            stream_cancel_tx: Some(stream_cancel_tx),
            tool_cancel_tx: Some(tool_cancel_tx),
            active_tool_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            current_streaming_message: Arc::new(tokio::sync::Mutex::new(String::new())),
            streaming_buffer: Arc::new(tokio::sync::Mutex::new(String::new())),
            fs_operation_tx: None,
        })
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
                SessionNotification {
                    meta: None,
                    session_id: session_id.clone(),
                    update: acp::SessionUpdate::ToolCall(acp::ToolCall {
                        meta: None,
                        id: acp::ToolCallId(tool_call_id.into()),
                        title,
                        kind: *kind,
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
                    meta: None,
                    session_id: session_id.clone(),
                    update: acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate {
                        meta: None,
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
                meta: None,
                id: acp::PermissionOptionId("allow".into()),
                name: "Allow".to_string(),
                kind: acp::PermissionOptionKind::AllowOnce,
            },
            acp::PermissionOption {
                meta: None,
                id: acp::PermissionOptionId("reject".into()),
                name: "Reject".to_string(),
                kind: acp::PermissionOptionKind::RejectOnce,
            },
        ];

        // Create the permission request
        let permission_request = acp::RequestPermissionRequest {
            meta: None,
            session_id: session_id.clone(),
            tool_call: acp::ToolCallUpdate {
                meta: None,
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
                        plan_entries.push(acp::PlanEntry {
                            meta: None,
                            content: todo_text,
                            priority: acp::PlanEntryPriority::Medium,
                            status: acp::PlanEntryStatus::Pending,
                        });
                    }
                } else if line.starts_with("- [x]") {
                    let todo_text = line.strip_prefix("- [x]").unwrap_or("").trim().to_string();
                    if !todo_text.is_empty() {
                        plan_entries.push(acp::PlanEntry {
                            meta: None,
                            content: todo_text,
                            priority: acp::PlanEntryPriority::Medium,
                            status: acp::PlanEntryStatus::Completed,
                        });
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
                SessionNotification {
                    meta: None,
                    session_id: session_id.clone(),
                    update: acp::SessionUpdate::Plan(acp::Plan {
                        meta: None,
                        entries: plan_entries,
                    }),
                },
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
        tool_calls: Vec<stakpak_shared::models::integrations::openai::ToolCall>,
        session_id: &acp::SessionId,
        tools_map: &std::collections::HashMap<String, Vec<rmcp::model::Tool>>,
    ) -> Result<Vec<stakpak_shared::models::integrations::openai::ChatMessage>, acp::Error> {
        log::info!(
            "🔧 DEBUG: Starting tool call processing with {} tool calls",
            tool_calls.len()
        );
        for (i, tool_call) in tool_calls.iter().enumerate() {
            log::info!(
                "🔧 DEBUG: Tool call {}: {} (id: {})",
                i,
                tool_call.function.name,
                tool_call.id
            );
        }

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
            let tool_title = self.generate_tool_title(&tool_call.function.name, &raw_input);
            let tool_kind = self.get_tool_kind(&tool_call.function.name);

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

            let (content, locations) = if self.should_use_diff_content(&tool_call.function.name) {
                if self.is_file_creation_tool(&tool_call.function.name) {
                    // For file creation: old_text = None, new_text = result_content
                    let diff_content = vec![acp::ToolCallContent::Diff {
                        diff: acp::Diff {
                            meta: None,
                            path: file_path.clone(),
                            old_text: None,
                            new_text: "".to_string(), // Will be updated after execution
                        },
                    }];
                    let tool_locations = vec![acp::ToolCallLocation {
                        meta: None,
                        path: file_path.clone(),
                        line: Some(0),
                    }];
                    (Some(diff_content), Some(tool_locations))
                } else {
                    // For file editing: use extracted old_string and new_string
                    let diff_content = vec![acp::ToolCallContent::Diff {
                        diff: acp::Diff {
                            meta: None,
                            path: file_path.clone(),
                            old_text: old_string,
                            new_text: new_string.unwrap_or_default(),
                        },
                    }];
                    let tool_locations = vec![acp::ToolCallLocation {
                        meta: None,
                        path: file_path.clone(),
                        line: Some(0),
                    }];
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
            let permission_granted = if self.is_auto_approved_tool(&tool_call.function.name) {
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
                    Some(vec![acp::ToolCallContent::Content {
                        content: acp::ContentBlock::Text(acp::TextContent {
                            meta: None,
                            text: "Tool execution rejected by user".to_string(),
                            annotations: None,
                        }),
                    }]),
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
            let is_view_directory = if tool_call.function.name == "view" {
                Path::new(&abs_path).is_dir()
            } else {
                false
            };

            let is_fs_tool = matches!(
                tool_call.function.name.as_str(),
                "view" | "create" | "str_replace"
            ) && !is_view_directory;

            let result = if is_fs_tool && self.fs_operation_tx.is_some() {
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
                        log::error!("ACP filesystem tool execution failed: {}", e);
                        // Return a more descriptive error instead of generic internal error
                        acp::Error::internal_error().with_data(serde_json::Value::String(format!(
                            "Tool execution failed: {}",
                            e
                        )))
                    })?
            } else if let Some(ref clients) = self.clients {
                log::info!(
                    "🔧 DEBUG: Executing tool call: {} with MCP clients",
                    tool_call.function.name
                );

                // Create cancellation receiver for this tool call
                let tool_cancel_rx = self.tool_cancel_tx.as_ref().map(|tx| tx.subscribe());

                crate::commands::agent::run::tooling::run_tool_call(
                    clients,
                    tools_map,
                    &tool_call,
                    tool_cancel_rx,
                    self.current_session_id.get(),
                )
                .await
                .map_err(|e| {
                    log::error!("MCP tool execution failed: {}", e);
                    acp::Error::internal_error().with_data(serde_json::Value::String(format!(
                        "MCP tool execution failed: {}",
                        e
                    )))
                })?
            } else {
                let error_msg = format!(
                    "No execution method available for tool: {}",
                    tool_call.function.name
                );
                log::error!("{}", error_msg);
                return Err(
                    acp::Error::internal_error().with_data(serde_json::Value::String(error_msg))
                );
            };

            log::info!(
                "🔧 DEBUG: Tool call execution completed for: {}",
                tool_call.function.name
            );

            if let Some(tool_result) = result {
                // Check if the tool call was cancelled
                if CallToolResultExt::get_status(&tool_result)
                    == stakpak_shared::models::integrations::openai::ToolCallResultStatus::Cancelled
                {
                    // Send cancellation notification
                    self.send_tool_call_update(
                        session_id,
                        tool_call_id.clone(),
                        acp::ToolCallStatus::Failed,
                        Some(vec![acp::ToolCallContent::Content {
                            content: acp::ContentBlock::Text(acp::TextContent {
                                meta: None,
                                text: "Tool call cancelled by user".to_string(),
                                annotations: None,
                            }),
                        }]),
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
                let completion_content = if self.should_use_diff_content(&tool_call.function.name) {
                    // For diff tools, we already sent the diff in the initial notification
                    // Just send a simple completion without additional content
                    None
                } else {
                    // For non-diff tools, send the result content
                    Some(vec![acp::ToolCallContent::Content {
                        content: acp::ContentBlock::Text(acp::TextContent {
                            meta: None,
                            text: result_content.clone(),
                            annotations: None,
                        }),
                    }])
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
                    Some(vec![acp::ToolCallContent::Content {
                        content: acp::ContentBlock::Text(acp::TextContent {
                            meta: None,
                            text: "Tool execution failed - no result returned".to_string(),
                            annotations: None,
                        }),
                    }]),
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
                    enabled_tools: EnabledToolsConfig { slack: false },
                    tool_mode: ToolMode::Combined,
                    bind_address,
                    certificate_chain: certificate_chain_for_server,
                    subagent_configs: None,
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

    async fn process_acp_streaming_response_with_cancellation(
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
                                    SessionNotification {
                                        meta: None,
                                        session_id: session_id.clone(),
                                        update: acp::SessionUpdate::AgentMessageChunk {
                                            content: acp::ContentBlock::Text(acp::TextContent {
                                                meta: None,
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

        // Flush any remaining content from the buffer at the end of the stream
        let flushed_content = self.flush_streaming_buffer().await;
        if !flushed_content.trim().is_empty() {
            // Send the flushed content
            let (tx, rx) = oneshot::channel();
            self.session_update_tx
                .send((
                    SessionNotification {
                        meta: None,
                        session_id: session_id.clone(),
                        update: acp::SessionUpdate::AgentMessageChunk {
                            content: acp::ContentBlock::Text(acp::TextContent {
                                meta: None,
                                text: flushed_content,
                                annotations: None,
                            }),
                        },
                    },
                    tx,
                ))
                .map_err(|_| "Failed to send flushed content")?;
            rx.await.map_err(|_| "Failed to await flushed content")?;
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

                // Create filesystem operation channel for native ACP filesystem operations
                let (fs_operation_tx, fs_operation_rx) = mpsc::unbounded_channel::<crate::commands::acp::fs_handler::FsOperation>();

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
                    stream_cancel_tx: self.stream_cancel_tx.clone(),
                    tool_cancel_tx: self.tool_cancel_tx.clone(),
                    active_tool_calls: self.active_tool_calls.clone(),
                    current_streaming_message: self.current_streaming_message.clone(),
                    streaming_buffer: self.streaming_buffer.clone(),
                    fs_operation_tx: Some(fs_operation_tx),
                };

                // Start up the StakpakAcpAgent connected to stdio.
                let (conn, handle_io) =
                    acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                        tokio::task::spawn_local(fut);
                    });

                // Wrap connection in Arc for sharing
                let conn_arc = Arc::new(conn);

                // Spawn filesystem handler for native ACP filesystem operations
                crate::commands::acp::fs_handler::spawn_fs_handler(conn_arc.clone(), fs_operation_rx);

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
                                    meta: None,
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
                                meta: None,
                                session_id: acp::SessionId("".to_string().into()), // TODO: Get actual session ID
                                update: acp::SessionUpdate::AgentMessageChunk {
                                    content: acp::ContentBlock::Text(acp::TextContent {
                                        meta: None,
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
            stream_cancel_tx: self.stream_cancel_tx.clone(),
            tool_cancel_tx: self.tool_cancel_tx.clone(),
            active_tool_calls: self.active_tool_calls.clone(),
            current_streaming_message: self.current_streaming_message.clone(),
            streaming_buffer: self.streaming_buffer.clone(),
            fs_operation_tx: self.fs_operation_tx.clone(),
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
        Ok(acp::InitializeResponse {
            meta: None,
            protocol_version: acp::V1,
            agent_capabilities: acp::AgentCapabilities {
                meta: None,
                mcp_capabilities: acp::McpCapabilities {
                    meta: None,
                    http: true,
                    sse: true,
                },
                // Enable session management
                load_session: true,
                // Enable prompt capabilities
                prompt_capabilities: acp::PromptCapabilities {
                    meta: None,
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

    async fn authenticate(
        &self,
        args: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        log::info!("Received authenticate request {args:?}");
        Ok(acp::AuthenticateResponse { meta: None })
    }

    async fn new_session(
        &self,
        args: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        log::info!("Received new session request {args:?}");

        // Check if we have a valid API key
        if self.config.api_key.is_none() {
            log::error!("API key is missing - authentication required");
            return Err(acp::Error::auth_required().with_data(serde_json::Value::String(
                "Authentication required. Please visit https://github.com/stakpak/agent for more information.".to_string()
            )));
        }

        let temp_session_id = Uuid::new_v4();
        let session_id = acp::SessionId(temp_session_id.to_string().into());

        // Track the current session ID
        self.current_session_id.set(Some(temp_session_id));

        // Clear message history for new session
        {
            let mut messages = self.messages.lock().await;
            //copy system message if exists
            let system_message = messages
                .iter()
                .find(|msg| msg.role == Role::System)
                .cloned();
            messages.clear();
            if let Some(system_message) = system_message {
                messages.push(system_message);
            }
        }

        Ok(acp::NewSessionResponse {
            session_id,
            modes: None,
            meta: None,
        })
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

        log::info!("Loaded session: {}", session_id_str);
        Ok(acp::LoadSessionResponse {
            meta: None,
            modes: None,
        })
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
                acp::Error::internal_error().with_data(serde_json::Value::String(format!(
                    "Chat completion failed: {}",
                    e
                )))
            })?;

        let response = match self
            .process_acp_streaming_response_with_cancellation(stream, &args.session_id)
            .await
        {
            Ok(response) => response,
            Err(e) => {
                if e == "STREAM_CANCELLED" {
                    log::info!("Stream was cancelled by user");
                    return Ok(acp::PromptResponse {
                        meta: None,
                        stop_reason: acp::StopReason::Cancelled,
                    });
                }
                log::error!("Stream processing failed: {}", e);
                return Err(
                    acp::Error::internal_error().with_data(serde_json::Value::String(format!(
                        "Stream processing failed: {}",
                        e
                    ))),
                );
            }
        };
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

        let content = if let Some(content) = &response.choices[0].message.content {
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

        log::info!(
            "🔧 DEBUG: Initial response has tool calls: {}",
            has_tool_calls
        );
        if has_tool_calls && let Some(tool_calls) = response.choices[0].message.tool_calls.as_ref()
        {
            log::info!("🔧 DEBUG: Initial tool calls count: {}", tool_calls.len());
            for (i, tool_call) in tool_calls.iter().enumerate() {
                log::info!(
                    "🔧 DEBUG: Initial tool call {}: {} (id: {})",
                    i,
                    tool_call.function.name,
                    tool_call.id
                );
            }
        }

        // Create cancellation receiver for tool call processing
        let mut tool_cancel_rx = self.tool_cancel_tx.as_ref().map(|tx| tx.subscribe());

        while has_tool_calls {
            log::info!("🔧 DEBUG: Starting tool call processing loop iteration");

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

                return Ok(acp::PromptResponse {
                    meta: None,
                    stop_reason: acp::StopReason::Cancelled,
                });
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
                    .process_tool_calls_with_cancellation(
                        tool_calls.clone(),
                        &args.session_id,
                        &tools_map,
                    )
                    .await
                    .map_err(|e| {
                        log::error!("Tool call processing failed: {}", e);
                        e
                    })?;

                // Check if any tool calls were cancelled in the current processing
                let has_cancelled_tool_calls = tool_results.iter().any(|msg| {
                    if let Some(
                        stakpak_shared::models::integrations::openai::MessageContent::String(text),
                    ) = &msg.content
                    {
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
                    return Ok(acp::PromptResponse {
                        meta: None,
                        stop_reason: acp::StopReason::Cancelled,
                    });
                }

                if has_cancelled_tool_calls {
                    log::info!("Tool calls were cancelled, stopping turn");
                    return Ok(acp::PromptResponse {
                        meta: None,
                        stop_reason: acp::StopReason::Cancelled,
                    });
                }

                // Make follow-up chat completion request after tool calls
                current_messages = {
                    let messages = self.messages.lock().await;
                    messages.clone()
                };

                let (follow_up_stream, _request_id) = self
                    .client
                    .chat_completion_stream(current_messages.clone(), tools_option.clone(), None)
                    .await
                    .map_err(|e| {
                        log::error!("Follow-up chat completion stream failed: {}", e);
                        acp::Error::internal_error().with_data(serde_json::Value::String(format!(
                            "Follow-up chat completion failed: {}",
                            e
                        )))
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
                            return Ok(acp::PromptResponse {
                                meta: None,
                                stop_reason: acp::StopReason::Cancelled,
                            });
                        }
                        log::error!("Follow-up stream processing failed: {}", e);
                        return Err(acp::Error::internal_error().with_data(
                            serde_json::Value::String(format!(
                                "Follow-up stream processing failed: {}",
                                e
                            )),
                        ));
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

        Ok(acp::PromptResponse {
            meta: None,
            stop_reason: acp::StopReason::EndTurn,
        })
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
}
