use std::{collections::HashMap, sync::Arc, time::Instant};

use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    channels::Channel, store::GatewayStore, targeting::ChannelTarget, types::OutboundReply,
};

#[derive(Clone)]
pub struct GatewayApiState {
    pub channels: HashMap<String, Arc<dyn Channel>>,
    pub store: Arc<GatewayStore>,
    pub started_at: Instant,
    pub delivery_context_ttl_hours: u64,
    pub auth_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GatewaySendRequest {
    pub channel: String,
    pub target: serde_json::Value,
    pub text: String,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct GatewaySendResponse {
    pub delivered: bool,
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
struct ApiError {
    error: String,
    message: String,
}

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
            get(move |headers: HeaderMap| {
                let state = state.clone();
                async move { sessions_handler(state, headers).await }
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
    let target = match ChannelTarget::parse(&request.channel, &request.target) {
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

    let Some(channel) = state.channels.get(&request.channel) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "channel_not_found".to_string(),
                message: format!("Channel '{}' is not connected", request.channel),
            }),
        )
            .into_response();
    };

    let reply = OutboundReply {
        channel: request.channel.clone().into(),
        peer_id: target.peer_id(),
        chat_type: target.chat_type(),
        text: request.text,
        metadata: target.metadata(),
    };

    if let Err(error) = channel.send(reply).await {
        return (
            StatusCode::BAD_GATEWAY,
            Json(ApiError {
                error: "delivery_failed".to_string(),
                message: error.to_string(),
            }),
        )
            .into_response();
    }

    if let Some(context) = request.context {
        let target_key = target.target_key();
        if let Err(error) = state
            .store
            .set_delivery_context(
                &request.channel,
                &target_key,
                &context,
                state.delivery_context_ttl_hours,
            )
            .await
        {
            warn!(
                channel = %request.channel,
                target_key = %target_key,
                error = %error,
                "failed to persist gateway delivery context after successful send"
            );
        }
    }

    (
        StatusCode::OK,
        Json(GatewaySendResponse { delivered: true }),
    )
        .into_response()
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
