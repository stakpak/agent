use crate::{
    message_bridge,
    sandbox::{SandboxConfig, SandboxedMcpServer},
    state::AppState,
    types::SessionHandle,
};
use async_trait::async_trait;
use rmcp::model::{
    CallToolRequestParam, CancelledNotification, CancelledNotificationMethod,
    CancelledNotificationParam, ServerResult,
};
use serde_json::json;
use stakai::Message;
use stakpak_agent_core::{
    AgentCommand, AgentConfig, AgentEvent, AgentHook, AgentRunContext, CheckpointEnvelopeV1,
    CompactionConfig, ContextConfig, PassthroughCompactionEngine, ProposedToolCall, RetryConfig,
    ToolExecutionResult, ToolExecutor, run_agent,
};
use stakpak_api::CreateCheckpointRequest;
use stakpak_mcp_client::McpClient;
use stakpak_shared::utils::sanitize_text_output;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_TURNS: usize = 64;
const CHECKPOINT_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
pub(crate) const ACTIVE_MODEL_METADATA_KEY: &str = "active_model";

pub fn build_run_context(session_id: Uuid, run_id: Uuid) -> AgentRunContext {
    AgentRunContext { run_id, session_id }
}

pub fn build_checkpoint_envelope(
    run_id: Uuid,
    messages: Vec<stakai::Message>,
    metadata: serde_json::Value,
) -> CheckpointEnvelopeV1 {
    CheckpointEnvelopeV1::new(Some(run_id), messages, metadata)
}

pub fn spawn_session_actor(
    state: AppState,
    session_id: Uuid,
    run_id: Uuid,
    model: stakai::Model,
    user_message: Message,
    sandbox_config: Option<SandboxConfig>,
) -> Result<SessionHandle, String> {
    let (command_tx, command_rx) = mpsc::channel(128);
    let cancel = CancellationToken::new();

    let handle = SessionHandle::new(command_tx, cancel.clone());

    let state_for_task = state.clone();
    tokio::spawn(async move {
        let actor_result = run_session_actor(
            state_for_task.clone(),
            session_id,
            run_id,
            model,
            user_message,
            command_rx,
            cancel,
            sandbox_config,
        )
        .await;

        let finish_result = actor_result.map(|_| ());
        let _ = state_for_task
            .run_manager
            .mark_run_finished(session_id, run_id, finish_result)
            .await;
    });

    Ok(handle)
}

#[allow(clippy::too_many_arguments)]
async fn run_session_actor(
    state: AppState,
    session_id: Uuid,
    run_id: Uuid,
    model: stakai::Model,
    user_message: Message,
    command_rx: mpsc::Receiver<AgentCommand>,
    cancel: CancellationToken,
    sandbox_config: Option<SandboxConfig>,
) -> Result<(), String> {
    let active_checkpoint = state
        .session_store
        .get_active_checkpoint(session_id)
        .await
        .ok();
    let parent_checkpoint_id = active_checkpoint.as_ref().map(|checkpoint| checkpoint.id);

    let initial_messages = match state.checkpoint_store.load_latest(session_id).await {
        Ok(Some(envelope)) => envelope.messages,
        Ok(None) => active_checkpoint
            .map(|checkpoint| message_bridge::chat_to_stakai(checkpoint.state.messages))
            .unwrap_or_default(),
        Err(error) => {
            return Err(format!("Failed to load checkpoint envelope: {error}"));
        }
    };

    let mut baseline_messages = initial_messages.clone();
    baseline_messages.push(user_message.clone());

    let checkpoint_runtime = Arc::new(CheckpointRuntime::new(
        state.clone(),
        session_id,
        run_id,
        model.clone(),
        parent_checkpoint_id,
        baseline_messages,
    ));

    checkpoint_runtime
        .persist_snapshot()
        .await
        .map_err(|error| format!("Failed to persist baseline checkpoint: {error}"))?;

    let periodic_checkpoint_cancel = CancellationToken::new();
    let periodic_checkpoint_runtime = checkpoint_runtime.clone();
    let periodic_checkpoint_cancel_task = periodic_checkpoint_cancel.clone();
    let periodic_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(CHECKPOINT_FLUSH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = periodic_checkpoint_cancel_task.cancelled() => break,
                _ = interval.tick() => {
                    let _ = periodic_checkpoint_runtime.persist_snapshot().await;
                }
            }
        }
    });

    let (core_event_tx, mut core_event_rx) = mpsc::channel::<AgentEvent>(256);

    let event_state = state.clone();
    let event_forwarder = tokio::spawn(async move {
        while let Some(event) = core_event_rx.recv().await {
            handle_core_event(&event_state, session_id, run_id, event).await;
        }
    });

    // If sandbox is requested, spawn a sandboxed MCP server for this session.
    // Otherwise, use the shared in-process MCP client.
    let sandbox = if let Some(sandbox_config) = sandbox_config {
        tracing::info!(session_id = %session_id, image = %sandbox_config.image, "Spawning sandbox container for session");
        Some(
            SandboxedMcpServer::spawn(&sandbox_config)
                .await
                .map_err(|e| format!("Failed to start sandbox for session {session_id}: {e}"))?,
        )
    } else {
        None
    };

    let (run_tools, tool_executor): (Vec<stakai::Tool>, Box<dyn ToolExecutor + Send + Sync>) =
        if let Some(ref sandbox) = sandbox {
            (
                sandbox.tools.clone(),
                Box::new(SandboxedToolExecutor {
                    mcp_client: sandbox.client.clone(),
                }),
            )
        } else {
            (
                state.current_mcp_tools().await,
                Box::new(ServerToolExecutor {
                    state: state.clone(),
                }),
            )
        };

    let agent_config = AgentConfig {
        model,
        system_prompt: String::new(),
        max_turns: MAX_TURNS,
        max_output_tokens: 0,
        provider_options: None,
        tool_approval: state.tool_approval_policy.clone(),
        context: ContextConfig::default(),
        retry: RetryConfig::default(),
        compaction: CompactionConfig::default(),
        tools: run_tools,
    };

    let hooks: Vec<Box<dyn AgentHook>> = vec![Box::new(ServerCheckpointHook {
        checkpoint_runtime: checkpoint_runtime.clone(),
    })];

    let compactor = PassthroughCompactionEngine;
    let run_context = build_run_context(session_id, run_id);

    let run_result = run_agent(
        run_context,
        state.inference.as_ref(),
        &agent_config,
        initial_messages,
        user_message,
        tool_executor.as_ref(),
        &hooks,
        core_event_tx,
        command_rx,
        cancel,
        &compactor,
    )
    .await;

    periodic_checkpoint_cancel.cancel();
    let _ = periodic_task.await;

    // Shut down sandbox container if one was started
    if let Some(sandbox) = sandbox {
        sandbox.shutdown().await;
    }

    state.clear_pending_tools(session_id, run_id).await;

    match &run_result {
        Ok(result) => {
            checkpoint_runtime.update_messages(&result.messages).await;
            checkpoint_runtime
                .persist_snapshot()
                .await
                .map_err(|error| format!("Failed to persist terminal checkpoint: {error}"))?;
        }
        Err(_) => {
            let _ = checkpoint_runtime.persist_snapshot().await;
        }
    }

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), event_forwarder).await;

    run_result
        .map(|_| ())
        .map_err(|error| format!("Agent run failed: {error}"))
}

async fn handle_core_event(state: &AppState, session_id: Uuid, run_id: Uuid, event: AgentEvent) {
    match &event {
        AgentEvent::ToolCallsProposed { tool_calls, .. } => {
            state
                .set_pending_tools(session_id, run_id, tool_calls.clone())
                .await;
        }
        AgentEvent::TurnCompleted { .. }
        | AgentEvent::RunCompleted { .. }
        | AgentEvent::RunError { .. } => {
            state.clear_pending_tools(session_id, run_id).await;
        }
        _ => {}
    }

    state.events.publish(session_id, Some(run_id), event).await;
}

#[derive(Clone)]
struct ServerToolExecutor {
    state: AppState,
}

#[async_trait]
impl ToolExecutor for ServerToolExecutor {
    async fn execute_tool_call(
        &self,
        run: &AgentRunContext,
        tool_call: &ProposedToolCall,
        cancel: &CancellationToken,
    ) -> Result<ToolExecutionResult, stakpak_agent_core::AgentError> {
        Ok(execute_mcp_tool_call(&self.state, run.session_id, run.run_id, tool_call, cancel).await)
    }
}

/// Tool executor that routes calls through a per-session sandboxed MCP client.
#[derive(Clone)]
struct SandboxedToolExecutor {
    mcp_client: Arc<McpClient>,
}

#[async_trait]
impl ToolExecutor for SandboxedToolExecutor {
    async fn execute_tool_call(
        &self,
        run: &AgentRunContext,
        tool_call: &ProposedToolCall,
        cancel: &CancellationToken,
    ) -> Result<ToolExecutionResult, stakpak_agent_core::AgentError> {
        Ok(execute_mcp_tool_call_with_client(
            &self.mcp_client,
            run.session_id,
            run.run_id,
            tool_call,
            cancel,
        )
        .await)
    }
}

struct CheckpointRuntime {
    state: AppState,
    session_id: Uuid,
    run_id: Uuid,
    active_model: stakai::Model,
    inner: Mutex<CheckpointRuntimeInner>,
}

struct CheckpointRuntimeInner {
    parent_checkpoint_id: Option<Uuid>,
    latest_messages: Vec<Message>,
    last_persisted_signature: Option<String>,
    dirty: bool,
}

impl CheckpointRuntime {
    fn new(
        state: AppState,
        session_id: Uuid,
        run_id: Uuid,
        active_model: stakai::Model,
        parent_checkpoint_id: Option<Uuid>,
        latest_messages: Vec<Message>,
    ) -> Self {
        Self {
            state,
            session_id,
            run_id,
            active_model,
            inner: Mutex::new(CheckpointRuntimeInner {
                parent_checkpoint_id,
                latest_messages,
                last_persisted_signature: None,
                dirty: true,
            }),
        }
    }

    async fn update_messages(&self, messages: &[Message]) {
        let mut guard = self.inner.lock().await;
        guard.latest_messages = messages.to_vec();
        guard.dirty = true;
    }

    async fn persist_snapshot(&self) -> Result<Uuid, String> {
        let mut guard = self.inner.lock().await;
        self.persist_if_needed(&mut guard).await
    }

    async fn persist_if_needed(&self, guard: &mut CheckpointRuntimeInner) -> Result<Uuid, String> {
        if !guard.dirty
            && let Some(checkpoint_id) = guard.parent_checkpoint_id
        {
            return Ok(checkpoint_id);
        }

        let signature = checkpoint_signature(&guard.latest_messages)?;
        let changed = guard.last_persisted_signature.as_deref() != Some(signature.as_str());
        let should_persist = guard.parent_checkpoint_id.is_none() || (guard.dirty && changed);

        if !should_persist {
            guard.dirty = false;
            if let Some(checkpoint_id) = guard.parent_checkpoint_id {
                return Ok(checkpoint_id);
            }
        }

        let checkpoint_id = persist_checkpoint(
            &self.state,
            self.session_id,
            self.run_id,
            &self.active_model,
            guard.parent_checkpoint_id,
            &guard.latest_messages,
        )
        .await?;

        guard.parent_checkpoint_id = Some(checkpoint_id);
        guard.last_persisted_signature = Some(signature);
        guard.dirty = false;

        Ok(checkpoint_id)
    }
}

struct ServerCheckpointHook {
    checkpoint_runtime: Arc<CheckpointRuntime>,
}

#[async_trait]
impl AgentHook for ServerCheckpointHook {
    async fn before_inference(
        &self,
        _run: &AgentRunContext,
        messages: &[Message],
        _model: &stakai::Model,
    ) -> Result<(), stakpak_agent_core::AgentError> {
        self.checkpoint_runtime.update_messages(messages).await;
        Ok(())
    }

    async fn after_inference(
        &self,
        _run: &AgentRunContext,
        messages: &[Message],
        _model: &stakai::Model,
    ) -> Result<(), stakpak_agent_core::AgentError> {
        self.checkpoint_runtime.update_messages(messages).await;
        Ok(())
    }

    async fn after_tool_execution(
        &self,
        _run: &AgentRunContext,
        _tool_call: &ProposedToolCall,
        messages: &[Message],
    ) -> Result<(), stakpak_agent_core::AgentError> {
        self.checkpoint_runtime.update_messages(messages).await;
        Ok(())
    }

    async fn on_error(
        &self,
        _run: &AgentRunContext,
        _error: &stakpak_agent_core::AgentError,
        messages: &[Message],
    ) -> Result<(), stakpak_agent_core::AgentError> {
        self.checkpoint_runtime.update_messages(messages).await;
        let _ = self.checkpoint_runtime.persist_snapshot().await;
        Ok(())
    }
}

async fn execute_mcp_tool_call(
    state: &AppState,
    session_id: Uuid,
    run_id: Uuid,
    tool_call: &ProposedToolCall,
    cancel: &CancellationToken,
) -> ToolExecutionResult {
    let Some(mcp_client) = state.mcp_client.as_ref() else {
        return ToolExecutionResult::Completed {
            result: "MCP client is not initialized".to_string(),
            is_error: true,
        };
    };

    execute_mcp_tool_call_with_client(mcp_client, session_id, run_id, tool_call, cancel).await
}

async fn execute_mcp_tool_call_with_client(
    mcp_client: &McpClient,
    session_id: Uuid,
    run_id: Uuid,
    tool_call: &ProposedToolCall,
    cancel: &CancellationToken,
) -> ToolExecutionResult {
    let metadata = Some(serde_json::Map::from_iter([
        (
            "session_id".to_string(),
            serde_json::Value::String(session_id.to_string()),
        ),
        (
            "run_id".to_string(),
            serde_json::Value::String(run_id.to_string()),
        ),
        (
            "tool_call_id".to_string(),
            serde_json::Value::String(tool_call.id.clone()),
        ),
    ]));

    let arguments = match &tool_call.arguments {
        serde_json::Value::Object(map) => Some(map.clone()),
        serde_json::Value::Null => None,
        other => Some(serde_json::Map::from_iter([(
            "input".to_string(),
            other.clone(),
        )])),
    };

    let request_handle = match stakpak_mcp_client::call_tool(
        mcp_client,
        CallToolRequestParam {
            name: tool_call.name.clone().into(),
            arguments,
        },
        metadata,
    )
    .await
    {
        Ok(handle) => handle,
        Err(error) => {
            return ToolExecutionResult::Completed {
                result: format!("MCP tool call failed: {error}"),
                is_error: true,
            };
        }
    };

    let peer_for_cancel = request_handle.peer.clone();
    let request_id = request_handle.id.clone();

    tokio::select! {
        _ = cancel.cancelled() => {
            let notification = CancelledNotification {
                method: CancelledNotificationMethod,
                params: CancelledNotificationParam {
                    request_id,
                    reason: Some("user cancel".to_string()),
                },
                extensions: Default::default(),
            };

            let _ = peer_for_cancel.send_notification(notification.into()).await;
            ToolExecutionResult::Cancelled
        }
        server_result = request_handle.await_response() => {
            match server_result {
                Ok(ServerResult::CallToolResult(result)) => {
                    ToolExecutionResult::Completed {
                        result: render_call_tool_result(&result),
                        is_error: result.is_error.unwrap_or(false),
                    }
                }
                Ok(_) => ToolExecutionResult::Completed {
                    result: "Unexpected MCP response type".to_string(),
                    is_error: true,
                },
                Err(error) => ToolExecutionResult::Completed {
                    result: format!("MCP tool execution error: {error}"),
                    is_error: true,
                },
            }
        }
    }
}

fn render_call_tool_result(result: &rmcp::model::CallToolResult) -> String {
    let rendered = result
        .content
        .iter()
        .filter_map(|content| content.raw.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n");

    if !rendered.is_empty() {
        return sanitize_text_output(&rendered);
    }

    if result.content.is_empty() {
        return "<empty tool result>".to_string();
    }

    "<non-text tool result omitted for safety>".to_string()
}

fn checkpoint_signature(messages: &[Message]) -> Result<String, String> {
    serde_json::to_string(messages)
        .map_err(|error| format!("Failed to serialize checkpoint messages: {error}"))
}

async fn persist_checkpoint(
    state: &AppState,
    session_id: Uuid,
    run_id: Uuid,
    active_model: &stakai::Model,
    parent_id: Option<Uuid>,
    messages: &[Message],
) -> Result<Uuid, String> {
    // TODO(ahmed): Migrate server/session checkpoint storage to `Vec<stakai::Message>` directly
    // and remove the ChatMessage adapter conversion (`message_bridge::stakai_to_chat`).
    let mut request = CreateCheckpointRequest::new(message_bridge::stakai_to_chat(messages));

    if let Some(parent_id) = parent_id {
        request = request.with_parent(parent_id);
    }

    let checkpoint = state
        .session_store
        .create_checkpoint(session_id, &request)
        .await
        .map_err(|error| error.to_string())?;

    let envelope = build_checkpoint_envelope(
        run_id,
        messages.to_vec(),
        json!({
            "session_id": session_id.to_string(),
            "checkpoint_id": checkpoint.id.to_string(),
            (ACTIVE_MODEL_METADATA_KEY): format!("{}/{}", active_model.provider, active_model.id),
        }),
    );

    state
        .checkpoint_store
        .save_latest(session_id, &envelope)
        .await
        .map_err(|error| {
            format!(
                "Failed to persist checkpoint envelope for session {}: {}",
                session_id, error
            )
        })?;

    Ok(checkpoint.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolResult, Content};
    use serde_json::json;
    use stakai::{Message, Role};

    #[test]
    fn run_id_is_not_regenerated_when_building_run_context() {
        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        let run_context = build_run_context(session_id, run_id);

        assert_eq!(run_context.session_id, session_id);
        assert_eq!(run_context.run_id, run_id);
    }

    #[test]
    fn checkpoint_envelope_carries_same_run_id() {
        let run_id = Uuid::new_v4();
        let envelope = build_checkpoint_envelope(
            run_id,
            vec![Message::new(Role::User, "hello")],
            json!({"turn": 1}),
        );

        assert_eq!(envelope.run_id, Some(run_id));
    }

    #[test]
    fn render_call_tool_result_sanitizes_text_blocks() {
        let result = CallToolResult::success(vec![Content::text("ok\u{0007}done")]);

        assert_eq!(render_call_tool_result(&result), "okdone");
    }

    #[test]
    fn render_call_tool_result_omits_non_text_blocks() {
        let result = CallToolResult::success(vec![Content::image("dGVzdA==", "image/png")]);

        assert_eq!(
            render_call_tool_result(&result),
            "<non-text tool result omitted for safety>"
        );
    }

    #[test]
    fn checkpoint_signature_changes_when_messages_change() {
        let messages_a = vec![Message::new(Role::User, "hello")];
        let messages_b = vec![
            Message::new(Role::User, "hello"),
            Message::new(Role::Assistant, "hi"),
        ];

        let sig_a = checkpoint_signature(&messages_a)
            .unwrap_or_else(|error| panic!("signature failed: {error}"));
        let sig_b = checkpoint_signature(&messages_b)
            .unwrap_or_else(|error| panic!("signature failed: {error}"));

        assert_ne!(sig_a, sig_b);
    }
}
