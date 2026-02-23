use std::{collections::HashMap, sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use tracing::warn;

use crate::{
    channels::Channel,
    client::StakpakClient,
    dispatcher::Dispatcher,
    router::{RouterConfig, resolve_routing_key},
    store::{GatewayStore, SessionMapping},
    targeting::{ChannelTarget, render_title_template},
    types::{DeliveryContext, InboundMessage, OutboundReply},
};

#[derive(Clone)]
pub struct GatewayApiState {
    pub channels: HashMap<String, Arc<dyn Channel>>,
    pub store: Arc<GatewayStore>,
    pub started_at: Instant,
    pub delivery_context_ttl_hours: u64,
    pub auth_token: Option<String>,
    pub client: StakpakClient,
    pub dispatcher: Arc<Dispatcher>,
    pub router_config: RouterConfig,
    pub title_template: String,
    pub inbound_tx: Arc<RwLock<Option<mpsc::Sender<InboundMessage>>>>,
}

#[derive(Debug, Deserialize)]
pub struct GatewaySendRequest {
    pub channel: String,
    pub target: serde_json::Value,
    pub text: String,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
    #[serde(default)]
    pub interactive: Option<InteractiveOptions>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CallerContextInput {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InteractiveOptions {
    pub prompt: String,
    #[serde(default)]
    pub caller_context: Vec<CallerContextInput>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub sandbox: Option<bool>,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GatewaySendResponse {
    pub delivered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GatewayChannelStatus {
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct GatewayChannelsResponse {
    pub channels: Vec<GatewayChannelStatus>,
}

#[derive(Debug, Serialize)]
pub struct GatewayStatusResponse {
    pub status: String,
    pub channels: usize,
    pub active_sessions: usize,
    pub uptime_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct GatewaySessionItem {
    pub routing_key: String,
    pub session_id: String,
    pub channel: String,
    pub target_key: String,
    pub title: String,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct GatewaySessionsResponse {
    pub sessions: Vec<GatewaySessionItem>,
}

#[derive(Debug, Serialize)]
pub struct GatewaySessionStatusResponse {
    pub session_id: String,
    pub active: bool,
    pub title: String,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
    message: String,
}

const MAX_INTERACTIVE_PROMPT_BYTES: usize = 32 * 1024;
const MAX_INTERACTIVE_CALLER_CONTEXT_ITEMS: usize = 50;
const MAX_INTERACTIVE_CALLER_CONTEXT_NAME_BYTES: usize = 256;
const MAX_INTERACTIVE_CALLER_CONTEXT_PRIORITY_BYTES: usize = 32;
const MAX_INTERACTIVE_CALLER_CONTEXT_ITEM_BYTES: usize = 10 * 1024;
const MAX_INTERACTIVE_CALLER_CONTEXT_TOTAL_BYTES: usize = 100 * 1024;

pub fn router(state: Arc<GatewayApiState>) -> Router {
    Router::new()
        .route(
            "/send",
            post({
                let state = state.clone();
                move |headers: HeaderMap, Json(request): Json<GatewaySendRequest>| {
                    let state = state.clone();
                    async move { send_handler(state, headers, request).await }
                }
            }),
        )
        .route(
            "/channels",
            get({
                let state = state.clone();
                move |headers: HeaderMap| {
                    let state = state.clone();
                    async move { channels_handler(state, headers).await }
                }
            }),
        )
        .route(
            "/status",
            get({
                let state = state.clone();
                move || {
                    let state = state.clone();
                    async move { status_handler(state).await }
                }
            }),
        )
        .route(
            "/sessions",
            get({
                let state = state.clone();
                move |headers: HeaderMap| {
                    let state = state.clone();
                    async move { sessions_handler(state, headers).await }
                }
            }),
        )
        .route(
            "/sessions/{session_id}",
            get(move |headers: HeaderMap, Path(session_id): Path<String>| {
                let state = state.clone();
                async move { session_status_handler(state, headers, session_id).await }
            }),
        )
}

fn require_auth(state: &GatewayApiState, headers: &HeaderMap) -> Option<axum::response::Response> {
    let expected = state.auth_token.as_deref()?;

    let provided = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if provided == Some(expected) {
        return None;
    }

    Some(
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                error: "unauthorized".to_string(),
                message: "Missing or invalid bearer token".to_string(),
            }),
        )
            .into_response(),
    )
}

async fn send_handler(
    state: Arc<GatewayApiState>,
    headers: HeaderMap,
    request: GatewaySendRequest,
) -> impl IntoResponse {
    if let Some(response) = require_auth(&state, &headers) {
        return response;
    }

    let GatewaySendRequest {
        channel: request_channel,
        target: request_target,
        text: request_text,
        context: request_context,
        interactive,
    } = request;

    let target = match ChannelTarget::parse(&request_channel, &request_target) {
        Ok(target) => target,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "invalid_target".to_string(),
                    message: error.to_string(),
                }),
            )
                .into_response();
        }
    };

    let Some(channel) = state.channels.get(&request_channel) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "channel_not_found".to_string(),
                message: format!("Channel '{}' is not connected", request_channel),
            }),
        )
            .into_response();
    };

    if let Some(interactive_request) = interactive.as_ref() {
        if state.auth_token.is_none() {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error: "interactive_auth_required".to_string(),
                    message: "interactive sends require gateway auth token configuration"
                        .to_string(),
                }),
            )
                .into_response();
        }

        if let Err(error) = validate_interactive_options(interactive_request) {
            return (StatusCode::BAD_REQUEST, Json(error)).into_response();
        }
    }

    let mut effective_target = target.clone();

    let first_reply = OutboundReply {
        channel: request_channel.clone().into(),
        peer_id: target.peer_id(),
        chat_type: target.chat_type(),
        text: request_text,
        metadata: target.metadata(),
    };

    let receipt = match channel.send_with_receipt(first_reply).await {
        Ok(receipt) => receipt,
        Err(error) => {
            warn!(
                channel = %request_channel,
                error = %error,
                "failed initial channel delivery"
            );
            return (
                StatusCode::BAD_GATEWAY,
                Json(ApiError {
                    error: "delivery_failed".to_string(),
                    message: "Failed to deliver message to channel".to_string(),
                }),
            )
                .into_response();
        }
    };

    if effective_target.thread_id().is_none() && receipt.thread_id.is_some() {
        effective_target = effective_target.with_thread_id(receipt.thread_id.clone());
    }

    if let Some(interactive) = interactive {
        if let Some(check_output) = request_context
            .as_ref()
            .and_then(extract_check_output)
            .filter(|value| !value.trim().is_empty())
        {
            let check_reply = OutboundReply {
                channel: request_channel.clone().into(),
                peer_id: effective_target.peer_id(),
                chat_type: effective_target.chat_type(),
                text: format!("Check output:\n{}", check_output.trim()),
                metadata: effective_target.metadata(),
            };

            if let Err(error) = channel.send(check_reply).await {
                warn!(
                    channel = %request_channel,
                    error = %error,
                    "failed to post interactive check output"
                );
            }
        }

        let title = interactive
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                render_title(&state.title_template, &request_channel, &effective_target)
            });

        let channel_id = crate::types::ChannelId::from(request_channel.clone());
        let peer_id = effective_target.peer_id();
        let chat_type = effective_target.chat_type();
        let now = Utc::now().timestamp_millis();

        let delivery = DeliveryContext {
            channel: channel_id.clone(),
            peer_id: peer_id.clone(),
            chat_type: chat_type.clone(),
            channel_meta: effective_target.metadata(),
            updated_at: now,
        };

        let routing_key =
            resolve_routing_key(&state.router_config, &channel_id, &peer_id, &chat_type);

        let (session_id, created_new_session) = match state.store.get(&routing_key).await {
            Ok(Some(existing)) => {
                if let Err(error) = state.store.update_delivery(&routing_key, &delivery).await {
                    warn!(
                        channel = %request_channel,
                        routing_key = %routing_key,
                        error = %error,
                        "failed to refresh existing interactive delivery mapping"
                    );
                }
                (existing.session_id, false)
            }
            Ok(None) => {
                let created = match state.client.create_session(&title).await {
                    Ok(value) => value,
                    Err(error) => {
                        warn!(
                            channel = %request_channel,
                            error = %error,
                            "failed to create interactive session"
                        );
                        send_interactive_failure_notice(
                            channel,
                            &request_channel,
                            &effective_target,
                            "Failed to create agent session",
                        )
                        .await;
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(ApiError {
                                error: "session_create_failed".to_string(),
                                message: "Failed to create interactive session".to_string(),
                            }),
                        )
                            .into_response();
                    }
                };

                let created_session_id = created.id.to_string();
                let mapping = SessionMapping {
                    session_id: created_session_id.clone(),
                    title,
                    delivery: delivery.clone(),
                    created_at: now,
                };

                if let Err(error) = state.store.set(&routing_key, &mapping).await {
                    warn!(
                        channel = %request_channel,
                        routing_key = %routing_key,
                        error = %error,
                        "failed to persist interactive session mapping"
                    );
                    let _ = state.client.delete_session(&created_session_id).await;
                    send_interactive_failure_notice(
                        channel,
                        &request_channel,
                        &effective_target,
                        "Failed to persist interactive session mapping",
                    )
                    .await;
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(ApiError {
                            error: "session_persist_failed".to_string(),
                            message: "Failed to persist interactive session mapping".to_string(),
                        }),
                    )
                        .into_response();
                }

                (created_session_id, true)
            }
            Err(error) => {
                warn!(
                    channel = %request_channel,
                    routing_key = %routing_key,
                    error = %error,
                    "failed to read interactive routing mapping"
                );
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(ApiError {
                        error: "session_lookup_failed".to_string(),
                        message: "Failed to resolve interactive session routing".to_string(),
                    }),
                )
                    .into_response();
            }
        };

        let prompt = build_interactive_prompt(&interactive, request_context.as_ref());

        let mut inbound_metadata = delivery.channel_meta.clone();
        if let Some(metadata) = inbound_metadata.as_object_mut() {
            metadata.insert(
                "gateway_run_options".to_string(),
                serde_json::json!({
                    "model": interactive.model,
                    "sandbox": interactive.sandbox,
                    "timeout": interactive.timeout,
                }),
            );
        }

        let inbound = InboundMessage {
            channel: channel_id,
            peer_id,
            chat_type,
            text: prompt,
            media: Vec::new(),
            metadata: inbound_metadata,
            timestamp: Utc::now(),
        };

        let inbound_tx = state.inbound_tx.read().await.clone();
        let Some(inbound_tx) = inbound_tx else {
            if created_new_session {
                let _ = state.store.delete(&routing_key).await;
                let _ = state.client.delete_session(&session_id).await;
            }
            send_interactive_failure_notice(
                channel,
                &request_channel,
                &effective_target,
                "Gateway runtime is not ready to start interactive sessions",
            )
            .await;
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiError {
                    error: "dispatcher_unavailable".to_string(),
                    message: "gateway runtime is not ready to start interactive sessions"
                        .to_string(),
                }),
            )
                .into_response();
        };

        if let Err(error) = inbound_tx.send(inbound).await {
            if created_new_session {
                let _ = state.store.delete(&routing_key).await;
                let _ = state.client.delete_session(&session_id).await;
            }
            warn!(
                channel = %request_channel,
                session_id = %session_id,
                error = %error,
                "failed to enqueue interactive inbound message"
            );
            send_interactive_failure_notice(
                channel,
                &request_channel,
                &effective_target,
                "Failed to start interactive agent run",
            )
            .await;
            return (
                StatusCode::BAD_GATEWAY,
                Json(ApiError {
                    error: "interactive_start_failed".to_string(),
                    message: "Failed to start interactive agent run".to_string(),
                }),
            )
                .into_response();
        }

        if let Some(context) = request_context.clone() {
            let target_key = effective_target.target_key();
            if let Err(error) = state
                .store
                .set_delivery_context(
                    &request_channel,
                    &target_key,
                    &context,
                    state.delivery_context_ttl_hours,
                )
                .await
            {
                warn!(
                    channel = %request_channel,
                    target_key = %target_key,
                    error = %error,
                    "failed to persist gateway delivery context after interactive start"
                );
            }
        }

        return (
            StatusCode::OK,
            Json(GatewaySendResponse {
                delivered: true,
                session_id: Some(session_id),
                thread_id: effective_target.thread_id(),
            }),
        )
            .into_response();
    }

    if let Some(context) = request_context {
        let target_key = effective_target.target_key();
        if let Err(error) = state
            .store
            .set_delivery_context(
                &request_channel,
                &target_key,
                &context,
                state.delivery_context_ttl_hours,
            )
            .await
        {
            warn!(
                channel = %request_channel,
                target_key = %target_key,
                error = %error,
                "failed to persist gateway delivery context after successful send"
            );
        }
    }

    (
        StatusCode::OK,
        Json(GatewaySendResponse {
            delivered: true,
            session_id: None,
            thread_id: effective_target.thread_id(),
        }),
    )
        .into_response()
}

async fn send_interactive_failure_notice(
    channel: &Arc<dyn Channel>,
    channel_id: &str,
    target: &ChannelTarget,
    message: &str,
) {
    let reply = OutboundReply {
        channel: channel_id.to_string().into(),
        peer_id: target.peer_id(),
        chat_type: target.chat_type(),
        text: format!("⚠️ {message}. Please try again."),
        metadata: target.metadata(),
    };

    if let Err(error) = channel.send(reply).await {
        warn!(
            channel = %channel_id,
            error = %error,
            "failed to deliver interactive failure notice"
        );
    }
}

fn validate_interactive_options(interactive: &InteractiveOptions) -> Result<(), ApiError> {
    if interactive.prompt.trim().is_empty() {
        return Err(ApiError {
            error: "invalid_interactive_prompt".to_string(),
            message: "interactive.prompt must not be empty".to_string(),
        });
    }

    if interactive.prompt.len() > MAX_INTERACTIVE_PROMPT_BYTES {
        return Err(ApiError {
            error: "invalid_interactive_prompt".to_string(),
            message: format!(
                "interactive.prompt exceeds {} bytes",
                MAX_INTERACTIVE_PROMPT_BYTES
            ),
        });
    }

    if interactive.timeout == Some(0) {
        return Err(ApiError {
            error: "invalid_interactive_timeout".to_string(),
            message: "interactive.timeout must be greater than 0".to_string(),
        });
    }

    if interactive.caller_context.len() > MAX_INTERACTIVE_CALLER_CONTEXT_ITEMS {
        return Err(ApiError {
            error: "invalid_interactive_caller_context".to_string(),
            message: format!(
                "interactive.caller_context supports up to {} items",
                MAX_INTERACTIVE_CALLER_CONTEXT_ITEMS
            ),
        });
    }

    let mut total_payload_bytes = interactive.prompt.len();

    for item in &interactive.caller_context {
        let name = item.name.trim();
        if name.is_empty() {
            return Err(ApiError {
                error: "invalid_interactive_caller_context".to_string(),
                message: "interactive.caller_context item name must not be empty".to_string(),
            });
        }

        let name_bytes = item.name.len();
        if name_bytes > MAX_INTERACTIVE_CALLER_CONTEXT_NAME_BYTES {
            return Err(ApiError {
                error: "invalid_interactive_caller_context".to_string(),
                message: format!(
                    "interactive.caller_context item name '{}' exceeds {} bytes",
                    item.name, MAX_INTERACTIVE_CALLER_CONTEXT_NAME_BYTES
                ),
            });
        }

        let priority_bytes = item.priority.as_ref().map_or(0, String::len);
        if priority_bytes > MAX_INTERACTIVE_CALLER_CONTEXT_PRIORITY_BYTES {
            return Err(ApiError {
                error: "invalid_interactive_caller_context".to_string(),
                message: format!(
                    "interactive.caller_context item '{}' priority exceeds {} bytes",
                    item.name, MAX_INTERACTIVE_CALLER_CONTEXT_PRIORITY_BYTES
                ),
            });
        }

        let content_bytes = item.content.len();
        if content_bytes > MAX_INTERACTIVE_CALLER_CONTEXT_ITEM_BYTES {
            return Err(ApiError {
                error: "invalid_interactive_caller_context".to_string(),
                message: format!(
                    "interactive.caller_context item '{}' exceeds {} bytes",
                    item.name, MAX_INTERACTIVE_CALLER_CONTEXT_ITEM_BYTES
                ),
            });
        }

        total_payload_bytes = total_payload_bytes
            .saturating_add(name_bytes)
            .saturating_add(priority_bytes)
            .saturating_add(content_bytes);
        if total_payload_bytes > MAX_INTERACTIVE_CALLER_CONTEXT_TOTAL_BYTES {
            return Err(ApiError {
                error: "invalid_interactive_caller_context".to_string(),
                message: format!(
                    "interactive payload exceeds {} bytes",
                    MAX_INTERACTIVE_CALLER_CONTEXT_TOTAL_BYTES
                ),
            });
        }
    }

    Ok(())
}

fn extract_check_output(context: &serde_json::Value) -> Option<String> {
    context
        .get("check_output")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn build_interactive_prompt(
    interactive: &InteractiveOptions,
    context: Option<&serde_json::Value>,
) -> String {
    let mut context_lines = Vec::new();

    for item in &interactive.caller_context {
        let priority = item.priority.as_deref().unwrap_or("normal");
        context_lines.push(format!("- {} [{}]: {}", item.name, priority, item.content));
    }

    if let Some(model) = interactive.model.as_deref()
        && !model.trim().is_empty()
    {
        context_lines.push(format!("- runtime_model [high]: {}", model.trim()));
    }

    if let Some(sandbox) = interactive.sandbox {
        context_lines.push(format!("- runtime_sandbox [high]: {sandbox}"));
    }

    if let Some(timeout) = interactive.timeout {
        context_lines.push(format!("- runtime_timeout_seconds [high]: {timeout}"));
    }

    if let Some(check_output) = context.and_then(extract_check_output)
        && !check_output.trim().is_empty()
    {
        context_lines.push(format!("- check_output [high]: {}", check_output.trim()));
    }

    if context_lines.is_empty() {
        return interactive.prompt.clone();
    }

    let mut text = interactive.prompt.clone();
    text.push_str("\n\n--- Caller Context ---\n");
    text.push_str(&context_lines.join("\n"));
    text.push('\n');
    text.push_str("---");
    text
}

fn render_title(template: &str, channel: &str, target: &ChannelTarget) -> String {
    let peer_id = target.peer_id().0;
    let chat_type = target.chat_type();
    render_title_template(template, channel, &peer_id, &chat_type)
}

async fn channels_handler(state: Arc<GatewayApiState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(response) = require_auth(&state, &headers) {
        return response;
    }
    let channels = state
        .channels
        .values()
        .map(|channel| GatewayChannelStatus {
            id: channel.id().0.clone(),
            name: channel.display_name().to_string(),
            status: "connected".to_string(),
        })
        .collect();

    (StatusCode::OK, Json(GatewayChannelsResponse { channels })).into_response()
}

async fn status_handler(state: Arc<GatewayApiState>) -> impl IntoResponse {
    let active_sessions = state
        .store
        .list(10_000)
        .await
        .map(|rows| rows.len())
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(GatewayStatusResponse {
            status: "ok".to_string(),
            channels: state.channels.len(),
            active_sessions,
            uptime_seconds: state.started_at.elapsed().as_secs(),
        }),
    )
}

async fn sessions_handler(state: Arc<GatewayApiState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(response) = require_auth(&state, &headers) {
        return response;
    }
    let sessions = state
        .store
        .list(1000)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(routing_key, mapping)| GatewaySessionItem {
            routing_key,
            session_id: mapping.session_id,
            channel: mapping.delivery.channel.0.clone(),
            target_key: crate::targeting::target_key_from_channel_chat(
                &mapping.delivery.channel,
                &mapping.delivery.chat_type,
                &mapping.delivery.peer_id,
            ),
            title: mapping.title,
            updated_at: mapping.delivery.updated_at,
        })
        .collect();

    (StatusCode::OK, Json(GatewaySessionsResponse { sessions })).into_response()
}

async fn session_status_handler(
    state: Arc<GatewayApiState>,
    headers: HeaderMap,
    session_id: String,
) -> impl IntoResponse {
    if let Some(response) = require_auth(&state, &headers) {
        return response;
    }

    let mapping = match state.store.find_by_session_id(&session_id).await {
        Ok(Some((_, mapping))) => mapping,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "session_not_found".to_string(),
                    message: format!("Session '{}' was not found", session_id),
                }),
            )
                .into_response();
        }
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(ApiError {
                    error: "session_lookup_failed".to_string(),
                    message: error.to_string(),
                }),
            )
                .into_response();
        }
    };

    let active = state.dispatcher.is_run_active(&session_id);

    (
        StatusCode::OK,
        Json(GatewaySessionStatusResponse {
            session_id,
            active,
            title: mapping.title,
            updated_at: mapping.delivery.updated_at,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        CallerContextInput, GatewayApiState, GatewaySendRequest, InteractiveOptions,
        build_interactive_prompt, extract_check_output, render_title, send_handler,
        validate_interactive_options,
    };
    use crate::channels::{Channel, ChannelTestResult};
    use crate::client::StakpakClient;
    use crate::config::ApprovalMode;
    use crate::dispatcher::Dispatcher;
    use crate::router::RouterConfig;
    use crate::store::GatewayStore;
    use crate::targeting::ChannelTarget;
    use crate::types::{ChannelId, InboundMessage, OutboundReply};
    use anyhow::Result;
    use async_trait::async_trait;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    use tokio::sync::{RwLock, mpsc};
    use tokio_util::sync::CancellationToken;

    struct MockChannel {
        id: ChannelId,
        send_count: Arc<AtomicUsize>,
    }

    impl MockChannel {
        fn new(id: &str, send_count: Arc<AtomicUsize>) -> Self {
            Self {
                id: ChannelId::from(id),
                send_count,
            }
        }
    }

    #[async_trait]
    impl Channel for MockChannel {
        fn id(&self) -> &ChannelId {
            &self.id
        }

        fn display_name(&self) -> &str {
            "Mock Channel"
        }

        async fn start(
            &self,
            _inbound_tx: mpsc::Sender<InboundMessage>,
            _cancel: CancellationToken,
        ) -> Result<()> {
            Ok(())
        }

        async fn send(&self, _reply: OutboundReply) -> Result<()> {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn test(&self) -> Result<ChannelTestResult> {
            Ok(ChannelTestResult {
                channel: self.id.0.clone(),
                identity: "mock".to_string(),
                details: "ok".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn interactive_request_without_auth_is_rejected_before_delivery() {
        let send_count = Arc::new(AtomicUsize::new(0));
        let channel_impl = Arc::new(MockChannel::new("slack", send_count.clone()));

        let mut channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();
        channels.insert("slack".to_string(), channel_impl);

        let store = Arc::new(
            GatewayStore::open_in_memory()
                .await
                .expect("failed to open in-memory gateway store"),
        );

        let client = StakpakClient::new("http://127.0.0.1:3999".to_string(), "".to_string());
        let dispatcher = Arc::new(Dispatcher::new(
            client.clone(),
            channels.clone(),
            store.clone(),
            RouterConfig::default(),
            None,
            ApprovalMode::AllowAll,
            Vec::new(),
            "{channel}:{chat_type}:{chat_id}".to_string(),
        ));

        let state = Arc::new(GatewayApiState {
            channels,
            store,
            started_at: Instant::now(),
            delivery_context_ttl_hours: 4,
            auth_token: None,
            client,
            dispatcher,
            router_config: RouterConfig::default(),
            title_template: "{channel}:{chat_type}:{chat_id}".to_string(),
            inbound_tx: Arc::new(RwLock::new(None)),
        });

        let request = GatewaySendRequest {
            channel: "slack".to_string(),
            target: serde_json::json!({"channel": "C123"}),
            text: "hello".to_string(),
            context: None,
            interactive: Some(InteractiveOptions {
                prompt: "run".to_string(),
                caller_context: Vec::new(),
                model: None,
                sandbox: None,
                timeout: None,
                title: None,
            }),
        };

        let response = send_handler(state, HeaderMap::new(), request)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(send_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn extract_check_output_reads_context_field() {
        let context = serde_json::json!({"check_output": "disk at 91%"});
        assert_eq!(
            extract_check_output(&context).as_deref(),
            Some("disk at 91%")
        );
    }

    #[test]
    fn build_interactive_prompt_includes_runtime_options() {
        let options = InteractiveOptions {
            prompt: "Investigate alert".to_string(),
            caller_context: vec![CallerContextInput {
                name: "schedule".to_string(),
                content: "disk-cleanup".to_string(),
                priority: Some("high".to_string()),
            }],
            model: Some("claude-sonnet".to_string()),
            sandbox: Some(true),
            timeout: Some(120),
            title: None,
        };

        let prompt = build_interactive_prompt(
            &options,
            Some(&serde_json::json!({"check_output": "disk=91%"})),
        );

        assert!(prompt.contains("runtime_model"));
        assert!(prompt.contains("runtime_sandbox"));
        assert!(prompt.contains("runtime_timeout_seconds"));
        assert!(prompt.contains("check_output"));
    }

    #[test]
    fn render_title_uses_target_fields() {
        let target = ChannelTarget::Slack {
            channel: "C123".to_string(),
            thread_ts: Some("1700000000.000100".to_string()),
        };

        let title = render_title("{channel}:{chat_type}:{chat_id}", "slack", &target);
        assert_eq!(title, "slack:thread:C123");
    }

    #[test]
    fn build_interactive_prompt_includes_check_output_without_caller_context() {
        let options = InteractiveOptions {
            prompt: "Investigate alert".to_string(),
            caller_context: Vec::new(),
            model: None,
            sandbox: None,
            timeout: None,
            title: None,
        };

        let prompt = build_interactive_prompt(
            &options,
            Some(&serde_json::json!({"check_output": "disk=91%"})),
        );

        assert!(prompt.contains("check_output"));
    }

    #[test]
    fn validate_interactive_options_rejects_empty_prompt() {
        let options = InteractiveOptions {
            prompt: "   ".to_string(),
            caller_context: Vec::new(),
            model: None,
            sandbox: None,
            timeout: None,
            title: None,
        };

        let result = validate_interactive_options(&options);
        assert!(result.is_err());
    }

    #[test]
    fn validate_interactive_options_rejects_large_caller_context_item() {
        let options = InteractiveOptions {
            prompt: "Investigate alert".to_string(),
            caller_context: vec![CallerContextInput {
                name: "check_output".to_string(),
                content: "x".repeat((10 * 1024) + 1),
                priority: Some("high".to_string()),
            }],
            model: None,
            sandbox: None,
            timeout: None,
            title: None,
        };

        let result = validate_interactive_options(&options);
        assert!(result.is_err());
    }

    #[test]
    fn validate_interactive_options_rejects_large_prompt() {
        let options = InteractiveOptions {
            prompt: "x".repeat((32 * 1024) + 1),
            caller_context: Vec::new(),
            model: None,
            sandbox: None,
            timeout: None,
            title: None,
        };

        let result = validate_interactive_options(&options);
        assert!(result.is_err());
    }

    #[test]
    fn validate_interactive_options_rejects_empty_caller_context_name() {
        let options = InteractiveOptions {
            prompt: "Investigate alert".to_string(),
            caller_context: vec![CallerContextInput {
                name: "   ".to_string(),
                content: "something".to_string(),
                priority: Some("high".to_string()),
            }],
            model: None,
            sandbox: None,
            timeout: None,
            title: None,
        };

        let result = validate_interactive_options(&options);
        assert!(result.is_err());
    }

    #[test]
    fn validate_interactive_options_rejects_large_priority() {
        let options = InteractiveOptions {
            prompt: "Investigate alert".to_string(),
            caller_context: vec![CallerContextInput {
                name: "schedule".to_string(),
                content: "run now".to_string(),
                priority: Some("x".repeat(33)),
            }],
            model: None,
            sandbox: None,
            timeout: None,
            title: None,
        };

        let result = validate_interactive_options(&options);
        assert!(result.is_err());
    }
}
