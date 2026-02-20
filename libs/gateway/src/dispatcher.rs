use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use chrono::Utc;
use stakai::{Message, Role};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::{
    channels::Channel,
    client::{
        MessageType, RunErrorPayload, SendMessageOptions, StakpakClient, ToolCallsProposedPayload,
        ToolDecisionAction, ToolDecisionInput,
    },
    config::ApprovalMode,
    router::{RouterConfig, resolve_routing_key},
    store::{GatewayStore, SessionMapping},
    targeting::target_key_from_inbound,
    types::{DeliveryContext, InboundMessage, OutboundReply},
};

pub struct Dispatcher {
    client: StakpakClient,
    channels: HashMap<String, Arc<dyn Channel>>,
    store: Arc<GatewayStore>,
    router_config: RouterConfig,
    active_runs: Mutex<HashMap<String, ActiveRun>>,
    pending_queues: Mutex<HashMap<String, Vec<QueuedMessage>>>,
    event_cursors: Mutex<HashMap<String, u64>>,
    default_model: Option<String>,
    approval_mode: ApprovalMode,
    approval_allowlist: HashSet<String>,
    title_template: String,
}

#[derive(Debug, Clone)]
struct ActiveRun {
    run_id: String,
    cancel: CancellationToken,
}

#[derive(Debug, Clone)]
struct QueuedMessage {
    inbound: InboundMessage,
    text: String,
}

#[derive(Debug)]
struct RunTaskResult {
    session_id: String,
    run_id: String,
    outcome: RunOutcome,
}

#[derive(Debug)]
enum RunOutcome {
    Completed {
        text: String,
        cursor: Option<u64>,
    },
    Error {
        error: Option<RunErrorPayload>,
        cursor: Option<u64>,
    },
    Cancelled {
        cursor: Option<u64>,
    },
    StreamEnded {
        cursor: Option<u64>,
    },
}

impl Dispatcher {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: StakpakClient,
        channels: HashMap<String, Arc<dyn Channel>>,
        store: Arc<GatewayStore>,
        router_config: RouterConfig,
        default_model: Option<String>,
        approval_mode: ApprovalMode,
        approval_allowlist: Vec<String>,
        title_template: String,
    ) -> Self {
        Self {
            client,
            channels,
            store,
            router_config,
            active_runs: Mutex::new(HashMap::new()),
            pending_queues: Mutex::new(HashMap::new()),
            event_cursors: Mutex::new(HashMap::new()),
            default_model,
            approval_mode,
            approval_allowlist: approval_allowlist.into_iter().collect(),
            title_template,
        }
    }

    pub async fn run(
        self: Arc<Self>,
        mut inbound_rx: mpsc::Receiver<InboundMessage>,
        cancel: CancellationToken,
    ) {
        let (run_tx, mut run_rx) = mpsc::channel::<RunTaskResult>(128);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    self.cancel_all_runs();
                    break;
                }
                maybe_inbound = inbound_rx.recv() => {
                    let Some(inbound) = maybe_inbound else {
                        break;
                    };
                    if let Err(error) = self.handle_inbound(inbound, run_tx.clone()).await {
                        error!(error = %error, "failed to handle inbound message");
                    }
                }
                maybe_result = run_rx.recv() => {
                    let Some(result) = maybe_result else {
                        continue;
                    };
                    if let Err(error) = self.handle_run_result(result, run_tx.clone()).await {
                        error!(error = %error, "failed to handle run result");
                    }
                }
            }
        }
    }

    async fn handle_inbound(
        self: &Arc<Self>,
        inbound: InboundMessage,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        let routing_key = resolve_routing_key(
            &self.router_config,
            &inbound.channel,
            &inbound.peer_id,
            &inbound.chat_type,
        );

        let target_key = target_key_from_inbound(&inbound);
        let enriched_text = match self
            .store
            .pop_delivery_context(&inbound.channel.0, &target_key)
            .await
        {
            Ok(Some(context)) => enrich_with_context(&context, &inbound.text),
            Ok(None) => inbound.text.clone(),
            Err(error) => {
                warn!(error = %error, "failed to pop delivery context");
                inbound.text.clone()
            }
        };

        let maybe_mapping = self
            .store
            .get(&routing_key)
            .await
            .map_err(|error| format!("failed to get mapping: {error}"))?;

        let mapping = if let Some(mapping) = maybe_mapping {
            let delivery = self.delivery_context_from_inbound(&inbound);
            if let Err(error) = self.store.update_delivery(&routing_key, &delivery).await {
                warn!(error = %error, "failed to update delivery context");
            }
            mapping
        } else {
            let title = self.render_title(&inbound);
            let created = self
                .client
                .create_session(&title)
                .await
                .map_err(|error| format!("create session failed: {error}"))?;

            let now = Utc::now().timestamp_millis();
            let mapping = SessionMapping {
                session_id: created.id.to_string(),
                title,
                delivery: self.delivery_context_from_inbound(&inbound),
                created_at: now,
            };

            self.store
                .set(&routing_key, &mapping)
                .await
                .map_err(|error| format!("failed to persist mapping: {error}"))?;

            mapping
        };

        let queued = QueuedMessage {
            inbound,
            text: enriched_text,
        };

        if self.is_run_active(&mapping.session_id) {
            self.enqueue_message(mapping.session_id.clone(), queued)?;
            return Ok(());
        }

        self.start_run(mapping.session_id, queued, run_tx).await
    }

    async fn handle_run_result(
        self: &Arc<Self>,
        result: RunTaskResult,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        self.remove_active_run(&result.session_id, &result.run_id);

        let cursor = match &result.outcome {
            RunOutcome::Completed { cursor, .. }
            | RunOutcome::Error { cursor, .. }
            | RunOutcome::Cancelled { cursor }
            | RunOutcome::StreamEnded { cursor } => *cursor,
        };

        if let Some(cursor) = cursor {
            self.set_cursor(&result.session_id, cursor)?;
        }

        match result.outcome {
            RunOutcome::Completed { text, .. } => {
                let text = if text.trim().is_empty() {
                    self.fetch_latest_assistant_text(&result.session_id)
                        .await
                        .unwrap_or_default()
                } else {
                    text
                };

                if !text.trim().is_empty() {
                    self.deliver_reply(&result.session_id, text).await;
                }
            }
            RunOutcome::Error { error, .. } => {
                let message = error
                    .and_then(|value| value.error)
                    .unwrap_or_else(|| "Agent run failed".to_string());
                self.deliver_reply(&result.session_id, format!("⚠️ {message}"))
                    .await;
            }
            RunOutcome::Cancelled { .. } | RunOutcome::StreamEnded { .. } => {}
        }

        self.drain_queue(&result.session_id, run_tx).await
    }

    async fn start_run(
        self: &Arc<Self>,
        session_id: String,
        queued: QueuedMessage,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        let message = Message::new(Role::User, queued.text.clone());
        let response = self
            .client
            .send_messages(
                &session_id,
                vec![message],
                SendMessageOptions {
                    model: self.default_model.clone(),
                    message_type: MessageType::Message,
                    run_id: None,
                    sandbox: None,
                },
            )
            .await;

        let response = match response {
            Ok(response) => response,
            Err(crate::client::ClientError::Conflict) => {
                self.enqueue_message(session_id, queued)?;
                return Ok(());
            }
            Err(error) => return Err(format!("send message failed: {error}")),
        };

        let run_id = response.run_id.to_string();
        let cancel = CancellationToken::new();

        {
            let mut guard = self
                .active_runs
                .lock()
                .map_err(|_| "failed to lock active_runs".to_string())?;
            guard.insert(
                session_id.clone(),
                ActiveRun {
                    run_id: run_id.clone(),
                    cancel: cancel.clone(),
                },
            );
        }

        let client = self.client.clone();
        let session_id_for_task = session_id.clone();
        let run_id_for_task = run_id.clone();
        let approval_mode = self.approval_mode.clone();
        let approval_allowlist = self.approval_allowlist.clone();
        let last_event_id = self.get_cursor(&session_id)?;

        tokio::spawn(async move {
            let outcome = consume_run_events(
                client,
                session_id_for_task.clone(),
                run_id_for_task.clone(),
                last_event_id,
                approval_mode,
                approval_allowlist,
                cancel,
            )
            .await;

            if let Err(error) = run_tx
                .send(RunTaskResult {
                    session_id: session_id_for_task,
                    run_id: run_id_for_task,
                    outcome,
                })
                .await
            {
                error!(error = %error, "failed to send run outcome");
            }
        });

        Ok(())
    }

    async fn drain_queue(
        self: &Arc<Self>,
        session_id: &str,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        let queue = {
            let mut guard = self
                .pending_queues
                .lock()
                .map_err(|_| "failed to lock pending_queues".to_string())?;
            guard.remove(session_id).unwrap_or_default()
        };

        if queue.is_empty() {
            return Ok(());
        }

        let combined_text = queue
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        if let Some(latest) = queue.last() {
            let routing_key = resolve_routing_key(
                &self.router_config,
                &latest.inbound.channel,
                &latest.inbound.peer_id,
                &latest.inbound.chat_type,
            );
            let delivery = self.delivery_context_from_inbound(&latest.inbound);
            if let Err(error) = self.store.update_delivery(&routing_key, &delivery).await {
                warn!(error = %error, "failed to refresh delivery context from queue");
            }
        }

        let Some(latest) = queue.last() else {
            return Ok(());
        };

        self.start_run(
            session_id.to_string(),
            QueuedMessage {
                inbound: latest.inbound.clone(),
                text: combined_text,
            },
            run_tx,
        )
        .await
    }

    async fn deliver_reply(&self, session_id: &str, text: String) {
        let mapping = match self.store.find_by_session_id(session_id).await {
            Ok(Some((_routing_key, mapping))) => mapping,
            Ok(None) => return,
            Err(error) => {
                warn!(error = %error, "failed to find delivery mapping");
                return;
            }
        };

        let Some(channel) = self.channels.get(&mapping.delivery.channel.0) else {
            warn!(channel = %mapping.delivery.channel.0, "channel not connected");
            return;
        };

        let reply = OutboundReply {
            channel: mapping.delivery.channel,
            peer_id: mapping.delivery.peer_id,
            chat_type: mapping.delivery.chat_type,
            text,
            metadata: mapping.delivery.channel_meta,
        };

        if let Err(error) = channel.send(reply).await {
            warn!(error = %error, "failed to send channel reply");
        }
    }

    async fn fetch_latest_assistant_text(&self, session_id: &str) -> Option<String> {
        let response = self.client.get_messages(session_id, 20, 0).await.ok()?;
        response
            .messages
            .into_iter()
            .rev()
            .find(|message| message.role == Role::Assistant)
            .and_then(|message| message.text())
    }

    fn render_title(&self, inbound: &InboundMessage) -> String {
        let chat_type = match inbound.chat_type {
            crate::types::ChatType::Direct => "dm",
            crate::types::ChatType::Group { .. } => "group",
            crate::types::ChatType::Thread { .. } => "thread",
        };
        let chat_id = match &inbound.chat_type {
            crate::types::ChatType::Direct => inbound.peer_id.0.clone(),
            crate::types::ChatType::Group { id } => id.clone(),
            crate::types::ChatType::Thread { group_id, .. } => group_id.clone(),
        };

        self.title_template
            .replace("{channel}", &inbound.channel.0)
            .replace("{peer}", &inbound.peer_id.0)
            .replace("{chat_type}", chat_type)
            .replace("{chat_id}", &chat_id)
    }

    fn delivery_context_from_inbound(&self, inbound: &InboundMessage) -> DeliveryContext {
        DeliveryContext {
            channel: inbound.channel.clone(),
            peer_id: inbound.peer_id.clone(),
            chat_type: inbound.chat_type.clone(),
            channel_meta: inbound.metadata.clone(),
            updated_at: Utc::now().timestamp_millis(),
        }
    }

    fn is_run_active(&self, session_id: &str) -> bool {
        self.active_runs
            .lock()
            .ok()
            .and_then(|guard| guard.get(session_id).cloned())
            .is_some()
    }

    fn enqueue_message(&self, session_id: String, message: QueuedMessage) -> Result<(), String> {
        let mut guard = self
            .pending_queues
            .lock()
            .map_err(|_| "failed to lock pending_queues".to_string())?;

        guard.entry(session_id).or_default().push(message);
        Ok(())
    }

    fn remove_active_run(&self, session_id: &str, run_id: &str) {
        if let Ok(mut guard) = self.active_runs.lock()
            && let Some(active) = guard.get(session_id)
            && active.run_id == run_id
        {
            guard.remove(session_id);
        }
    }

    fn cancel_all_runs(&self) {
        if let Ok(guard) = self.active_runs.lock() {
            for active in guard.values() {
                active.cancel.cancel();
            }
        }
    }

    fn get_cursor(&self, session_id: &str) -> Result<Option<u64>, String> {
        let guard = self
            .event_cursors
            .lock()
            .map_err(|_| "failed to lock event_cursors".to_string())?;
        Ok(guard.get(session_id).copied())
    }

    fn set_cursor(&self, session_id: &str, cursor: u64) -> Result<(), String> {
        let mut guard = self
            .event_cursors
            .lock()
            .map_err(|_| "failed to lock event_cursors".to_string())?;
        let current = guard.get(session_id).copied();
        let next = current.map_or(cursor, |value| value.max(cursor));
        guard.insert(session_id.to_string(), next);
        Ok(())
    }
}

async fn consume_run_events(
    client: StakpakClient,
    session_id: String,
    run_id: String,
    last_event_id: Option<u64>,
    approval_mode: ApprovalMode,
    approval_allowlist: HashSet<String>,
    cancel: CancellationToken,
) -> RunOutcome {
    let mut stream = match client.subscribe_events(&session_id, last_event_id).await {
        Ok(stream) => stream,
        Err(error) => {
            return RunOutcome::Error {
                error: Some(RunErrorPayload {
                    run_id: Uuid::parse_str(&run_id).ok(),
                    error: Some(format!("failed to subscribe to events: {error}")),
                }),
                cursor: last_event_id,
            };
        }
    };

    let mut accumulated_text = String::new();
    let mut cursor = last_event_id;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return RunOutcome::Cancelled { cursor };
            }
            next = stream.next_event() => {
                let event = match next {
                    Ok(Some(event)) => event,
                    Ok(None) => return RunOutcome::StreamEnded { cursor },
                    Err(error) => {
                        return RunOutcome::Error {
                            error: Some(RunErrorPayload {
                                run_id: Uuid::parse_str(&run_id).ok(),
                                error: Some(format!("stream read failed: {error}")),
                            }),
                            cursor,
                        };
                    }
                };

                if let Some(id) = event.event_id_u64 {
                    cursor = Some(cursor.map_or(id, |value| value.max(id)));
                }

                if event.run_id().as_deref() != Some(run_id.as_str()) {
                    continue;
                }

                match event.event_type.as_str() {
                    "text_delta" => {
                        if let Some(delta) = event.as_text_delta() {
                            accumulated_text.push_str(&delta);
                        }
                    }
                    "tool_calls_proposed" => {
                        if let Some(proposed) = event.as_tool_calls_proposed() {
                            let decisions = build_tool_decisions(
                                proposed,
                                &approval_mode,
                                &approval_allowlist,
                            );
                            if let Err(error) = client
                                .resolve_tools(&session_id, &run_id, decisions)
                                .await
                            {
                                return RunOutcome::Error {
                                    error: Some(RunErrorPayload {
                                        run_id: Uuid::parse_str(&run_id).ok(),
                                        error: Some(format!("resolve_tools failed: {error}")),
                                    }),
                                    cursor,
                                };
                            }
                        }
                    }
                    "run_completed" => {
                        return RunOutcome::Completed {
                            text: accumulated_text,
                            cursor,
                        };
                    }
                    "run_error" => {
                        return RunOutcome::Error {
                            error: event.as_run_error(),
                            cursor,
                        };
                    }
                    _ => {}
                }
            }
        }
    }
}

fn build_tool_decisions(
    proposed: ToolCallsProposedPayload,
    approval_mode: &ApprovalMode,
    approval_allowlist: &HashSet<String>,
) -> HashMap<String, ToolDecisionInput> {
    proposed
        .tool_calls
        .into_iter()
        .map(|tool_call| {
            let action = match approval_mode {
                ApprovalMode::AllowAll => ToolDecisionAction::Accept,
                ApprovalMode::DenyAll => ToolDecisionAction::Reject,
                ApprovalMode::Allowlist => {
                    if approval_allowlist.contains(&tool_call.name) {
                        ToolDecisionAction::Accept
                    } else {
                        ToolDecisionAction::Reject
                    }
                }
            };

            (
                tool_call.id,
                ToolDecisionInput {
                    action,
                    content: None,
                },
            )
        })
        .collect()
}

fn enrich_with_context(context: &serde_json::Value, user_text: &str) -> String {
    let mut enriched =
        String::from("The user is replying to a previous notification.\n\n--- Watch Context ---\n");

    if let Some(trigger) = context.get("trigger").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Trigger: {trigger}\n"));
    }

    if let Some(status) = context.get("status").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Status: {status}\n"));
    }

    if let Some(summary) = context.get("summary").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Summary: {summary}\n"));
    }

    if let Some(check_output) = context.get("check_output").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Check output: {check_output}\n"));
    }

    enriched.push_str("---\n\n");
    enriched.push_str(&format!("User message: {user_text}"));
    enriched
}

use uuid::Uuid;
