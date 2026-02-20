use crate::{
    auth::{AuthConfig, require_bearer},
    idempotency::{IdempotencyRequest, LookupResult, StoredResponse},
    message_bridge,
    session_actor::{ACTIVE_MODEL_METADATA_KEY, spawn_session_actor},
    state::AppState,
    types::SessionRuntimeState,
};
use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response, sse::Event, sse::KeepAlive, sse::Sse},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use stakpak_agent_core::{AgentCommand, AgentEvent, ToolDecision};
use stakpak_api::{
    ListSessionsQuery, SessionStatus, StorageCreateSessionRequest, StorageUpdateSessionRequest,
};
use std::{collections::HashMap, convert::Infallible, time::Duration};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_seconds: u64,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    error: String,
    code: String,
    request_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum RunState {
    Idle,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Serialize)]
struct RunStatusDto {
    state: RunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct SessionDto {
    id: Uuid,
    title: String,
    cwd: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    run_status: RunStatusDto,
}

#[derive(Debug, Serialize)]
struct SessionsResponse {
    sessions: Vec<SessionDto>,
    total: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct CreateSessionBody {
    title: String,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct UpdateSessionBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    visibility: Option<stakpak_api::SessionVisibility>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum SessionMessageType {
    #[default]
    Message,
    Steering,
    FollowUp,
}

#[derive(Debug, Deserialize, Serialize)]
struct SessionMessageRequest {
    message: stakai::Message,
    #[serde(default)]
    r#type: SessionMessageType,
    #[serde(default)]
    run_id: Option<Uuid>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    sandbox: Option<bool>,
}

#[derive(Debug, Serialize)]
struct SessionMessageResponse {
    run_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct ListSessionsParams {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListMessagesParams {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

#[derive(Debug, Serialize)]
struct SessionMessagesResponse {
    messages: Vec<stakai::Message>,
    total: usize,
}

#[derive(Debug, Serialize)]
struct SessionDetailResponse {
    session: SessionDto,
    config: ConfigResponse,
}

#[derive(Debug, Serialize)]
struct PendingToolsResponse {
    run_id: Option<Uuid>,
    tool_calls: Vec<stakpak_agent_core::ProposedToolCall>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
enum DecisionAction {
    Accept,
    Reject,
    CustomResult,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct DecisionInput {
    action: DecisionAction,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ToolDecisionRequest {
    run_id: Uuid,
    #[serde(flatten)]
    decision: DecisionInput,
}

#[derive(Debug, Deserialize, Serialize)]
struct ToolDecisionsRequest {
    run_id: Uuid,
    decisions: HashMap<String, DecisionInput>,
}

#[derive(Debug, Serialize)]
struct ToolDecisionResponse {
    accepted: bool,
    run_id: Uuid,
}

#[derive(Debug, Deserialize, Serialize)]
struct CancelRequest {
    run_id: Uuid,
}

#[derive(Debug, Serialize)]
struct CancelResponse {
    cancelled: bool,
    run_id: Uuid,
}

#[derive(Debug, Deserialize, Serialize)]
struct ModelSwitchRequest {
    run_id: Uuid,
    model: String,
}

#[derive(Debug, Serialize)]
struct ModelSwitchResponse {
    accepted: bool,
    run_id: Uuid,
    model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum AutoApproveMode {
    None,
    All,
    Custom,
}

#[derive(Debug, Serialize)]
struct ConfigResponse {
    default_model: Option<String>,
    auto_approve_mode: AutoApproveMode,
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    models: Vec<stakai::Model>,
}

pub fn router(state: AppState, auth: AuthConfig) -> Router {
    public_router()
        .merge(protected_router(auth))
        .with_state(state)
}

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/v1/health", get(health_handler))
        .route("/v1/openapi.json", get(openapi_handler))
}

pub fn protected_router(auth: AuthConfig) -> Router<AppState> {
    Router::new()
        .route(
            "/v1/sessions",
            get(list_sessions_handler).post(create_session_handler),
        )
        .route(
            "/v1/sessions/{id}",
            get(get_session_handler)
                .patch(update_session_handler)
                .delete(delete_session_handler),
        )
        .route(
            "/v1/sessions/{id}/messages",
            post(sessions_message_handler).get(get_session_messages_handler),
        )
        .route("/v1/sessions/{id}/events", get(session_events_handler))
        .route(
            "/v1/sessions/{id}/tools/pending",
            get(pending_tools_handler),
        )
        .route(
            "/v1/sessions/{id}/tools/{tool_call_id}/decision",
            post(tool_decision_handler),
        )
        .route(
            "/v1/sessions/{id}/tools/decisions",
            post(tool_decisions_handler),
        )
        .route(
            "/v1/sessions/{id}/tools/resolve",
            post(tool_decisions_handler),
        )
        .route("/v1/sessions/{id}/cancel", post(cancel_handler))
        .route("/v1/sessions/{id}/model", post(model_switch_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/config", get(config_handler))
        .route_layer(middleware::from_fn_with_state(auth, require_bearer))
}

async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.uptime_seconds(),
    })
}

async fn openapi_handler() -> Json<utoipa::openapi::OpenApi> {
    Json(crate::openapi::generate_openapi())
}

async fn list_sessions_handler(
    State(state): State<AppState>,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<SessionsResponse>, Response> {
    let mut query = ListSessionsQuery::new();

    if let Some(limit) = params.limit {
        query = query.with_limit(limit);
    }
    if let Some(offset) = params.offset {
        query = query.with_offset(offset);
    }
    if let Some(search) = params.search {
        query = query.with_search(search);
    }
    if let Some(status) = params.status {
        query.status = parse_status_param(&status);
    }

    let result = state
        .session_store
        .list_sessions(&query)
        .await
        .map_err(storage_error)?;

    let mut sessions = Vec::with_capacity(result.sessions.len());
    for summary in result.sessions {
        let run_status = state.run_manager.state(summary.id).await;
        sessions.push(SessionDto {
            id: summary.id,
            title: summary.title,
            cwd: summary.cwd,
            created_at: summary.created_at,
            updated_at: summary.updated_at,
            run_status: map_run_status(run_status),
        });
    }

    Ok(Json(SessionsResponse {
        total: sessions.len(),
        sessions,
    }))
}

async fn create_session_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionBody>,
) -> Result<Response, Response> {
    let idempotency = prepare_idempotency(
        &state,
        &headers,
        "POST",
        "/v1/sessions".to_string(),
        serde_json::to_value(&body).unwrap_or_else(|_| json!({})),
    )
    .await?;

    if let Some(replayed) = idempotency_replay_response(idempotency.lookup_result.clone()) {
        return Ok(replayed);
    }

    let mut request = StorageCreateSessionRequest::new(body.title, Vec::new());
    if let Some(cwd) = body.cwd {
        request = request.with_cwd(cwd);
    }

    let created = state
        .session_store
        .create_session(&request)
        .await
        .map_err(storage_error)?;

    let session = state
        .session_store
        .get_session(created.session_id)
        .await
        .map_err(storage_error)?;

    let payload = SessionDto {
        id: session.id,
        title: session.title,
        cwd: session.cwd,
        created_at: session.created_at,
        updated_at: session.updated_at,
        run_status: map_run_status(SessionRuntimeState::Idle),
    };

    save_idempotency_response(&state, idempotency.request, StatusCode::CREATED, &payload).await;

    Ok((StatusCode::CREATED, Json(payload)).into_response())
}

async fn get_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionDetailResponse>, Response> {
    let session = state
        .session_store
        .get_session(session_id)
        .await
        .map_err(storage_error)?;

    let run_status = state.run_manager.state(session_id).await;

    Ok(Json(SessionDetailResponse {
        session: SessionDto {
            id: session.id,
            title: session.title,
            cwd: session.cwd,
            created_at: session.created_at,
            updated_at: session.updated_at,
            run_status: map_run_status(run_status),
        },
        config: runtime_config(&state),
    }))
}

async fn update_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(body): Json<UpdateSessionBody>,
) -> Result<Json<SessionDetailResponse>, Response> {
    let mut request = StorageUpdateSessionRequest::new();
    if let Some(title) = body.title {
        request = request.with_title(title);
    }
    if let Some(visibility) = body.visibility {
        request = request.with_visibility(visibility);
    }

    let session = state
        .session_store
        .update_session(session_id, &request)
        .await
        .map_err(storage_error)?;

    let run_status = state.run_manager.state(session_id).await;

    Ok(Json(SessionDetailResponse {
        session: SessionDto {
            id: session.id,
            title: session.title,
            cwd: session.cwd,
            created_at: session.created_at,
            updated_at: session.updated_at,
            run_status: map_run_status(run_status),
        },
        config: runtime_config(&state),
    }))
}

async fn delete_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<StatusCode, Response> {
    if let Some(run_id) = state.run_manager.active_run_id(session_id).await {
        let _ = state.run_manager.cancel_run(session_id, run_id).await;
    }

    state
        .session_store
        .delete_session(session_id)
        .await
        .map_err(storage_error)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn sessions_message_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<SessionMessageRequest>,
) -> Result<Response, Response> {
    let _ = state
        .session_store
        .get_session(session_id)
        .await
        .map_err(storage_error)?;

    if let Some(error_response) = validate_session_message_request(&request) {
        return Err(error_response);
    }

    let idempotency = prepare_idempotency(
        &state,
        &headers,
        "POST",
        format!("/v1/sessions/{session_id}/messages"),
        serde_json::to_value(&request).unwrap_or_else(|_| json!({})),
    )
    .await?;

    if let Some(replayed) = idempotency_replay_response(idempotency.lookup_result.clone()) {
        return Ok(replayed);
    }

    let active_run_id = state.run_manager.active_run_id(session_id).await;

    match request.r#type {
        SessionMessageType::Message => {
            if let (Some(requested_run_id), Some(active_run_id)) = (request.run_id, active_run_id)
                && requested_run_id != active_run_id
            {
                return Err(api_error(
                    StatusCode::CONFLICT,
                    "run_mismatch",
                    "Provided run_id does not match active run",
                ));
            }

            let _ = state.refresh_mcp_tools().await;

            let requested_or_persisted_model = match request.model.as_deref() {
                Some(model) => Some(model.to_string()),
                None => load_persisted_model_for_session(&state, session_id).await,
            };

            let model = state
                .resolve_model(requested_or_persisted_model.as_deref())
                .ok_or_else(|| {
                    api_error(StatusCode::BAD_REQUEST, "invalid_model", "Unknown model")
                })?;

            let state_for_spawn = state.clone();
            let message_for_spawn = request.message;
            let sandbox_config = if request.sandbox.unwrap_or(false) {
                state.sandbox_config.clone()
            } else {
                None
            };

            let run_id = state
                .run_manager
                .start_run(session_id, move |allocated_run_id| {
                    let state = state_for_spawn.clone();
                    let message = message_for_spawn.clone();
                    let model = model.clone();
                    let sandbox_config = sandbox_config.clone();
                    async move {
                        spawn_session_actor(
                            state,
                            session_id,
                            allocated_run_id,
                            model,
                            message,
                            sandbox_config,
                        )
                    }
                })
                .await
                .map_err(run_manager_error)?;

            let payload = SessionMessageResponse { run_id };
            save_idempotency_response(&state, idempotency.request, StatusCode::OK, &payload).await;
            Ok((StatusCode::OK, Json(payload)).into_response())
        }
        SessionMessageType::Steering | SessionMessageType::FollowUp => {
            let Some(run_id) = request.run_id else {
                return Err(api_error(
                    StatusCode::CONFLICT,
                    "run_mismatch",
                    "run_id is required for steering/follow_up",
                ));
            };

            let text = extract_message_text(&request.message);

            let command = match request.r#type {
                SessionMessageType::Steering => AgentCommand::Steering(text),
                SessionMessageType::FollowUp => AgentCommand::FollowUp(text),
                SessionMessageType::Message => AgentCommand::FollowUp(String::new()),
            };

            state
                .run_manager
                .send_command(session_id, run_id, command)
                .await
                .map_err(run_manager_error)?;

            let payload = SessionMessageResponse { run_id };
            save_idempotency_response(&state, idempotency.request, StatusCode::OK, &payload).await;
            Ok((StatusCode::OK, Json(payload)).into_response())
        }
    }
}

async fn get_session_messages_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Query(query): Query<ListMessagesParams>,
) -> Result<Json<SessionMessagesResponse>, Response> {
    let all_messages = match state.checkpoint_store.load_latest(session_id).await {
        Ok(Some(envelope)) => envelope.messages,
        Ok(None) => {
            let checkpoint = state
                .session_store
                .get_active_checkpoint(session_id)
                .await
                .map_err(storage_error)?;
            message_bridge::chat_to_stakai(checkpoint.state.messages)
        }
        Err(error) => {
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "checkpoint_read_failed",
                &format!("Failed to read checkpoint envelope: {error}"),
            ));
        }
    };

    let total = all_messages.len();

    let offset = query.offset.unwrap_or(0).min(total);
    let limit = query.limit.unwrap_or(100);
    let end = offset.saturating_add(limit).min(total);

    let messages = all_messages[offset..end].to_vec();

    Ok(Json(SessionMessagesResponse { messages, total }))
}

async fn session_events_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let last_event_id = headers
        .get("Last-Event-ID")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let subscription = state.events.subscribe(session_id, last_event_id).await;
    let replay = subscription.replay;
    let gap_detected = subscription.gap_detected;
    let mut live = subscription.live;

    let stream = stream! {
        if let Some(gap) = gap_detected {
            let data = serde_json::to_string(&gap)
                .unwrap_or_else(|_| "{\"error\":\"serialization_failed\"}".to_string());
            yield Ok::<Event, Infallible>(Event::default().event("gap_detected").data(data));
        }

        for envelope in replay {
            yield Ok::<Event, Infallible>(envelope_to_sse_event(envelope));
        }

        loop {
            match live.recv().await {
                Ok(envelope) => yield Ok::<Event, Infallible>(envelope_to_sse_event(envelope)),
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    };

    Ok(Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response())
}

async fn pending_tools_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<PendingToolsResponse>, Response> {
    let pending = state.pending_tools(session_id).await;

    Ok(Json(PendingToolsResponse {
        run_id: pending.as_ref().map(|value| value.run_id),
        tool_calls: pending.map(|value| value.tool_calls).unwrap_or_default(),
    }))
}

async fn tool_decision_handler(
    State(state): State<AppState>,
    Path((session_id, tool_call_id)): Path<(Uuid, String)>,
    headers: HeaderMap,
    Json(request): Json<ToolDecisionRequest>,
) -> Result<Response, Response> {
    let idempotency = prepare_idempotency(
        &state,
        &headers,
        "POST",
        format!("/v1/sessions/{session_id}/tools/{tool_call_id}/decision"),
        serde_json::to_value(&request).unwrap_or_else(|_| json!({})),
    )
    .await?;

    if let Some(replayed) = idempotency_replay_response(idempotency.lookup_result.clone()) {
        return Ok(replayed);
    }

    state
        .run_manager
        .send_command(
            session_id,
            request.run_id,
            AgentCommand::ResolveTool {
                tool_call_id,
                decision: map_decision(request.decision)
                    .map_err(DecisionMappingError::into_response)?,
            },
        )
        .await
        .map_err(run_manager_error)?;

    let payload = ToolDecisionResponse {
        accepted: true,
        run_id: request.run_id,
    };
    save_idempotency_response(&state, idempotency.request, StatusCode::OK, &payload).await;

    Ok((StatusCode::OK, Json(payload)).into_response())
}

async fn tool_decisions_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<ToolDecisionsRequest>,
) -> Result<Response, Response> {
    let idempotency = prepare_idempotency(
        &state,
        &headers,
        "POST",
        format!("/v1/sessions/{session_id}/tools/decisions"),
        serde_json::to_value(&request).unwrap_or_else(|_| json!({})),
    )
    .await?;

    if let Some(replayed) = idempotency_replay_response(idempotency.lookup_result.clone()) {
        return Ok(replayed);
    }

    let mut decisions = HashMap::new();
    for (tool_call_id, decision) in request.decisions {
        decisions.insert(
            tool_call_id,
            map_decision(decision).map_err(DecisionMappingError::into_response)?,
        );
    }

    state
        .run_manager
        .send_command(
            session_id,
            request.run_id,
            AgentCommand::ResolveTools { decisions },
        )
        .await
        .map_err(run_manager_error)?;

    let payload = ToolDecisionResponse {
        accepted: true,
        run_id: request.run_id,
    };
    save_idempotency_response(&state, idempotency.request, StatusCode::OK, &payload).await;

    Ok((StatusCode::OK, Json(payload)).into_response())
}

async fn cancel_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<CancelRequest>,
) -> Result<Response, Response> {
    let idempotency = prepare_idempotency(
        &state,
        &headers,
        "POST",
        format!("/v1/sessions/{session_id}/cancel"),
        serde_json::to_value(&request).unwrap_or_else(|_| json!({})),
    )
    .await?;

    if let Some(replayed) = idempotency_replay_response(idempotency.lookup_result.clone()) {
        return Ok(replayed);
    }

    state
        .run_manager
        .cancel_run(session_id, request.run_id)
        .await
        .map_err(run_manager_error)?;

    let payload = CancelResponse {
        cancelled: true,
        run_id: request.run_id,
    };
    save_idempotency_response(&state, idempotency.request, StatusCode::OK, &payload).await;

    Ok((StatusCode::OK, Json(payload)).into_response())
}

async fn model_switch_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<ModelSwitchRequest>,
) -> Result<Response, Response> {
    let idempotency = prepare_idempotency(
        &state,
        &headers,
        "POST",
        format!("/v1/sessions/{session_id}/model"),
        serde_json::to_value(&request).unwrap_or_else(|_| json!({})),
    )
    .await?;

    if let Some(replayed) = idempotency_replay_response(idempotency.lookup_result.clone()) {
        return Ok(replayed);
    }

    let model = state
        .resolve_model(Some(&request.model))
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "invalid_model", "Unknown model"))?;

    state
        .run_manager
        .send_command(session_id, request.run_id, AgentCommand::SwitchModel(model))
        .await
        .map_err(run_manager_error)?;

    let payload = ModelSwitchResponse {
        accepted: true,
        run_id: request.run_id,
        model: request.model,
    };
    save_idempotency_response(&state, idempotency.request, StatusCode::OK, &payload).await;

    Ok((StatusCode::OK, Json(payload)).into_response())
}

async fn models_handler(State(state): State<AppState>) -> Json<ModelsResponse> {
    Json(ModelsResponse {
        models: state.models.as_ref().clone(),
    })
}

async fn config_handler(State(state): State<AppState>) -> Json<ConfigResponse> {
    Json(runtime_config(&state))
}

async fn load_persisted_model_for_session(state: &AppState, session_id: Uuid) -> Option<String> {
    match state.checkpoint_store.load_latest(session_id).await {
        Ok(Some(envelope)) => envelope
            .metadata
            .get(ACTIVE_MODEL_METADATA_KEY)
            .and_then(serde_json::Value::as_str)
            .map(std::string::ToString::to_string),
        Ok(None) | Err(_) => None,
    }
}

fn runtime_config(state: &AppState) -> ConfigResponse {
    ConfigResponse {
        default_model: state
            .default_model
            .as_ref()
            .map(|model| format!("{}/{}", model.provider, model.id)),
        auto_approve_mode: match &state.tool_approval_policy {
            stakpak_agent_core::ToolApprovalPolicy::None => AutoApproveMode::None,
            stakpak_agent_core::ToolApprovalPolicy::All => AutoApproveMode::All,
            stakpak_agent_core::ToolApprovalPolicy::Custom { .. } => AutoApproveMode::Custom,
        },
    }
}

fn parse_status_param(status: &str) -> Option<SessionStatus> {
    match status.to_ascii_uppercase().as_str() {
        "ACTIVE" => Some(SessionStatus::Active),
        "DELETED" => Some(SessionStatus::Deleted),
        _ => None,
    }
}

fn map_run_status(state: SessionRuntimeState) -> RunStatusDto {
    match state {
        SessionRuntimeState::Idle => RunStatusDto {
            state: RunState::Idle,
            run_id: None,
        },
        SessionRuntimeState::Starting { run_id } => RunStatusDto {
            state: RunState::Starting,
            run_id: Some(run_id),
        },
        SessionRuntimeState::Running { run_id, .. } => RunStatusDto {
            state: RunState::Running,
            run_id: Some(run_id),
        },
        SessionRuntimeState::Failed { .. } => RunStatusDto {
            state: RunState::Failed,
            run_id: None,
        },
    }
}

fn storage_error(error: stakpak_api::StorageError) -> Response {
    match error {
        stakpak_api::StorageError::NotFound(message) => {
            api_error(StatusCode::NOT_FOUND, "not_found", &message)
        }
        stakpak_api::StorageError::InvalidRequest(message) => {
            api_error(StatusCode::BAD_REQUEST, "invalid_request", &message)
        }
        stakpak_api::StorageError::Unauthorized(message) => {
            api_error(StatusCode::UNAUTHORIZED, "unauthorized", &message)
        }
        stakpak_api::StorageError::RateLimited(message) => {
            api_error(StatusCode::TOO_MANY_REQUESTS, "rate_limited", &message)
        }
        stakpak_api::StorageError::Connection(message) => {
            api_error(StatusCode::BAD_GATEWAY, "connection_error", &message)
        }
        stakpak_api::StorageError::Internal(message) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &message,
        ),
    }
}

fn run_manager_error(error: crate::error::SessionManagerError) -> Response {
    match error {
        crate::error::SessionManagerError::SessionAlreadyRunning
        | crate::error::SessionManagerError::SessionStarting => api_error(
            StatusCode::CONFLICT,
            "session_already_running",
            "Session already has an active run",
        ),
        crate::error::SessionManagerError::SessionNotRunning
        | crate::error::SessionManagerError::CommandChannelClosed => api_error(
            StatusCode::CONFLICT,
            "session_not_running",
            "Session has no active run",
        ),
        crate::error::SessionManagerError::RunMismatch { .. } => api_error(
            StatusCode::CONFLICT,
            "run_mismatch",
            "Provided run_id does not match active run",
        ),
        crate::error::SessionManagerError::ActorStartupFailed(message) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "actor_startup_failed",
            &message,
        ),
    }
}

fn api_error(status: StatusCode, code: &str, message: &str) -> Response {
    let body = ApiErrorBody {
        error: message.to_string(),
        code: code.to_string(),
        request_id: format!("req_{}", Uuid::new_v4().simple()),
    };

    (status, Json(body)).into_response()
}

fn validate_session_message_request(request: &SessionMessageRequest) -> Option<Response> {
    if request.message.role != stakai::Role::User {
        return Some(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_message_role",
            "message.role must be 'user' for this endpoint",
        ));
    }

    None
}

fn extract_message_text(message: &stakai::Message) -> String {
    if let Some(text) = message.text() {
        return text;
    }

    match &message.content {
        stakai::MessageContent::Text(text) => text.clone(),
        stakai::MessageContent::Parts(parts) => parts
            .iter()
            .filter_map(|part| match part {
                stakai::ContentPart::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[derive(Debug, Clone, Copy)]
enum DecisionMappingError {
    MissingCustomResultContent,
}

impl DecisionMappingError {
    fn into_response(self) -> Response {
        match self {
            DecisionMappingError::MissingCustomResultContent => api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "custom_result requires content",
            ),
        }
    }
}

fn map_decision(input: DecisionInput) -> Result<ToolDecision, DecisionMappingError> {
    match input.action {
        DecisionAction::Accept => Ok(ToolDecision::Accept),
        DecisionAction::Reject => Ok(ToolDecision::Reject),
        DecisionAction::CustomResult => {
            let Some(content) = input.content else {
                return Err(DecisionMappingError::MissingCustomResultContent);
            };
            Ok(ToolDecision::CustomResult { content })
        }
    }
}

fn event_name(event: &AgentEvent) -> &'static str {
    match event {
        AgentEvent::RunStarted { .. } => "run_started",
        AgentEvent::TurnStarted { .. } => "turn_started",
        AgentEvent::TurnCompleted { .. } => "turn_completed",
        AgentEvent::RunCompleted { .. } => "run_completed",
        AgentEvent::RunError { .. } => "run_error",
        AgentEvent::TextDelta { .. } => "text_delta",
        AgentEvent::ThinkingDelta { .. } => "thinking_delta",
        AgentEvent::TextComplete { .. } => "text_complete",
        AgentEvent::ToolCallsProposed { .. } => "tool_calls_proposed",
        AgentEvent::WaitingForToolApproval { .. } => "waiting_for_tool_approval",
        AgentEvent::ToolExecutionStarted { .. } => "tool_execution_started",
        AgentEvent::ToolExecutionProgress { .. } => "tool_execution_progress",
        AgentEvent::ToolExecutionCompleted { .. } => "tool_execution_completed",
        AgentEvent::ToolRejected { .. } => "tool_rejected",
        AgentEvent::RetryAttempt { .. } => "retry_attempt",
        AgentEvent::CompactionStarted { .. } => "compaction_started",
        AgentEvent::CompactionCompleted { .. } => "compaction_completed",
        AgentEvent::UsageReport { .. } => "usage_report",
    }
}

fn envelope_to_sse_event(envelope: crate::event_log::EventEnvelope) -> Event {
    let event = event_name(&envelope.event);
    let id = envelope.id.to_string();
    let data = serde_json::to_string(&envelope)
        .unwrap_or_else(|_| "{\"error\":\"serialization_failed\"}".to_string());

    Event::default().id(id).event(event).data(data)
}

#[derive(Clone)]
struct IdempotencyCheck {
    request: Option<IdempotencyRequest>,
    lookup_result: Option<LookupResult>,
}

async fn prepare_idempotency(
    state: &AppState,
    headers: &HeaderMap,
    method: &str,
    path: String,
    body: serde_json::Value,
) -> Result<IdempotencyCheck, Response> {
    let key = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    let Some(key) = key else {
        return Ok(IdempotencyCheck {
            request: None,
            lookup_result: None,
        });
    };

    let request = IdempotencyRequest::new(method, path, key, body);
    let lookup = state.idempotency.lookup(&request).await;

    if matches!(lookup, LookupResult::Conflict) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "idempotency_key_reused",
            "Idempotency key was reused with different payload",
        ));
    }

    Ok(IdempotencyCheck {
        request: Some(request),
        lookup_result: Some(lookup),
    })
}

fn idempotency_replay_response(lookup: Option<LookupResult>) -> Option<Response> {
    let LookupResult::Replay(stored) = lookup? else {
        return None;
    };

    let status = match StatusCode::from_u16(stored.status_code) {
        Ok(status) => status,
        Err(_) => StatusCode::OK,
    };

    Some((status, Json(stored.body)).into_response())
}

async fn save_idempotency_response<T: Serialize>(
    state: &AppState,
    request: Option<IdempotencyRequest>,
    status: StatusCode,
    payload: &T,
) {
    let Some(request) = request else {
        return;
    };

    if let Ok(body) = serde_json::to_value(payload) {
        state
            .idempotency
            .save(&request, StoredResponse::new(status.as_u16(), body))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{
            Request,
            header::{AUTHORIZATION, CONTENT_TYPE},
        },
    };
    use http_body_util::BodyExt as _;
    use stakpak_agent_core::{ToolApprovalAction, ToolApprovalPolicy};
    use stakpak_api::SessionStorage;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn test_state_with_event_capacity(capacity: usize) -> Result<AppState, String> {
        let storage_backend = stakpak_api::LocalStorage::new(":memory:")
            .await
            .map_err(|error| error.to_string())?;
        let storage: Arc<dyn SessionStorage> = Arc::new(storage_backend);

        let inference = Arc::new(stakai::Inference::new());
        let events = Arc::new(crate::EventLog::new(capacity));
        let idempotency = Arc::new(crate::IdempotencyStore::new(Duration::from_secs(3600)));
        let model = stakai::Model::custom("test-model", "openai");

        let checkpoint_root = std::env::temp_dir().join(format!(
            "stakpak-server-routes-checkpoints-{}",
            Uuid::new_v4()
        ));

        Ok(AppState::new(
            storage,
            events,
            idempotency,
            inference,
            vec![model.clone()],
            Some(model),
            ToolApprovalPolicy::Custom {
                rules: HashMap::from([("stakpak__view".to_string(), ToolApprovalAction::Approve)]),
                default: ToolApprovalAction::Ask,
            },
        )
        .with_checkpoint_store(Arc::new(crate::CheckpointStore::new(checkpoint_root))))
    }

    async fn test_state() -> Result<AppState, String> {
        test_state_with_event_capacity(256).await
    }

    async fn next_sse_chunk(body: &mut Body) -> Option<String> {
        let next = match tokio::time::timeout(Duration::from_millis(750), body.frame()).await {
            Ok(next) => next,
            Err(_) => return None,
        };

        let frame = match next {
            Some(Ok(frame)) => frame,
            Some(Err(_)) | None => return None,
        };

        let data = match frame.into_data() {
            Ok(data) => data,
            Err(_) => return None,
        };

        Some(String::from_utf8_lossy(&data).to_string())
    }

    #[tokio::test]
    async fn openapi_endpoint_is_generated() {
        let app = match test_state().await {
            Ok(state) => router(state, AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let request = match Request::builder()
            .method("GET")
            .uri("/v1/openapi.json")
            .body(Body::empty())
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::OK);

        let body = match to_bytes(response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read body: {error}"),
        };

        let openapi: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(error) => panic!("invalid openapi json: {error}"),
        };

        assert_eq!(openapi.get("openapi"), Some(&json!("3.1.0")));
        assert!(
            openapi
                .get("paths")
                .and_then(|value| value.get("/v1/sessions/{id}/messages"))
                .is_some(),
            "expected generated paths to include /v1/sessions/{{id}}/messages"
        );

        assert!(
            openapi
                .get("components")
                .and_then(|value| value.get("securitySchemes"))
                .and_then(|value| value.get("bearer_auth"))
                .is_some(),
            "expected generated components.securitySchemes.bearer_auth"
        );

        let message_content_schema = openapi
            .get("components")
            .and_then(|value| value.get("schemas"))
            .and_then(|value| value.get("MessageContentDoc"));

        assert!(
            message_content_schema
                .and_then(|value| value.get("oneOf").or_else(|| value.get("anyOf")))
                .is_some(),
            "expected MessageContentDoc to model text-or-parts variants"
        );
    }

    #[tokio::test]
    async fn health_endpoint_is_public() {
        let app = match test_state().await {
            Ok(state) => router(state, AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let request = match Request::builder().uri("/v1/health").body(Body::empty()) {
            Ok(request) => request,
            Err(error) => panic!("failed to build request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn events_endpoint_replays_from_last_event_id() {
        let state = match test_state().await {
            Ok(state) => state,
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let app = router(state.clone(), AuthConfig::token("secret"));

        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        state
            .events
            .publish(session_id, Some(run_id), AgentEvent::RunStarted { run_id })
            .await;
        state
            .events
            .publish(
                session_id,
                Some(run_id),
                AgentEvent::TurnStarted { run_id, turn: 1 },
            )
            .await;
        state
            .events
            .publish(
                session_id,
                Some(run_id),
                AgentEvent::TextDelta {
                    run_id,
                    delta: "hello".to_string(),
                },
            )
            .await;

        let request = match Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{session_id}/events"))
            .header(AUTHORIZATION, "Bearer secret")
            .header("Last-Event-ID", "1")
            .body(Body::empty())
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build events request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("events request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::OK);

        let mut body = response.into_body();

        let first = match next_sse_chunk(&mut body).await {
            Some(chunk) => chunk,
            None => panic!("expected first replay SSE chunk"),
        };

        let second = match next_sse_chunk(&mut body).await {
            Some(chunk) => chunk,
            None => panic!("expected second replay SSE chunk"),
        };

        let replay = format!("{first}{second}");

        assert!(
            replay.contains("id:2") || replay.contains("id: 2"),
            "expected replay to include event id 2, got: {replay}"
        );
        assert!(
            replay.contains("id:3") || replay.contains("id: 3"),
            "expected replay to include event id 3, got: {replay}"
        );
        assert!(
            replay.contains("event:turn_started") || replay.contains("event: turn_started"),
            "expected replay to include turn_started event"
        );
        assert!(
            replay.contains("event:text_delta") || replay.contains("event: text_delta"),
            "expected replay to include text_delta event"
        );
    }

    #[tokio::test]
    async fn events_endpoint_emits_gap_detected_control_event() {
        let state = match test_state_with_event_capacity(2).await {
            Ok(state) => state,
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let app = router(state.clone(), AuthConfig::token("secret"));

        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        for turn in 1..=4 {
            state
                .events
                .publish(
                    session_id,
                    Some(run_id),
                    AgentEvent::TurnStarted { run_id, turn },
                )
                .await;
        }

        let request = match Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{session_id}/events"))
            .header(AUTHORIZATION, "Bearer secret")
            .header("Last-Event-ID", "1")
            .body(Body::empty())
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build events request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("events request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::OK);

        let mut body = response.into_body();
        let first_chunk = match next_sse_chunk(&mut body).await {
            Some(chunk) => chunk,
            None => panic!("expected gap_detected SSE chunk"),
        };

        assert!(
            first_chunk.contains("event:gap_detected")
                || first_chunk.contains("event: gap_detected"),
            "expected gap_detected control event, got: {first_chunk}"
        );
        assert!(
            first_chunk.contains("\"requested_after_id\":1"),
            "expected requested_after_id in gap payload"
        );
        assert!(
            first_chunk.contains("\"resume_hint\":\"refresh_snapshot_then_resume\""),
            "expected resume hint in gap payload"
        );
    }

    #[tokio::test]
    async fn sessions_messages_accepts_stakai_message_input() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let create_payload = json!({"title":"test"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create response body: {error}"),
        };

        let created_session: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create response json: {error}"),
        };

        let session_id = match created_session.get("id").and_then(|value| value.as_str()) {
            Some(session_id) => session_id,
            None => panic!("create response missing session id"),
        };

        let payload = json!({
            "message": {
                "role": "user",
                "content": "hello from stakai"
            },
            "model": "openai/test-model"
        });

        let request = match Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session_id}/messages"))
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::OK);

        let body = match to_bytes(response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read response body: {error}"),
        };

        let parsed: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(parsed) => parsed,
            Err(error) => panic!("response should be valid json: {error}"),
        };

        assert!(parsed.get("run_id").is_some());
    }

    #[tokio::test]
    async fn sessions_messages_rejects_non_user_role_input() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let create_payload = json!({"title":"test-invalid-role"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create response body: {error}"),
        };

        let created_session: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create response json: {error}"),
        };

        let session_id = match created_session.get("id").and_then(|value| value.as_str()) {
            Some(session_id) => session_id,
            None => panic!("create response missing session id"),
        };

        let payload = json!({
            "message": {
                "role": "assistant",
                "content": "this should be rejected"
            },
            "type": "message"
        });

        let request = match Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session_id}/messages"))
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = match to_bytes(response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read response body: {error}"),
        };

        let parsed: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(parsed) => parsed,
            Err(error) => panic!("response should be valid json: {error}"),
        };

        assert_eq!(parsed.get("code"), Some(&json!("invalid_message_role")));
    }

    #[tokio::test]
    async fn get_messages_returns_stakai_parts_from_checkpoint_envelope() {
        let state = match test_state().await {
            Ok(state) => state,
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let app = router(state.clone(), AuthConfig::token("secret"));

        let create_payload = json!({"title":"checkpoint-message-test"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create body: {error}"),
        };

        let created_session: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create response json: {error}"),
        };

        let session_id = match created_session.get("id").and_then(|value| value.as_str()) {
            Some(session_id) => session_id,
            None => panic!("create response missing session id"),
        };

        let session_uuid = match Uuid::parse_str(session_id) {
            Ok(uuid) => uuid,
            Err(error) => panic!("invalid session id: {error}"),
        };

        let envelope = stakpak_agent_core::CheckpointEnvelopeV1::new(
            Some(Uuid::new_v4()),
            vec![
                stakai::Message::new(stakai::Role::User, "hello"),
                stakai::Message::new(
                    stakai::Role::Assistant,
                    vec![
                        stakai::ContentPart::text("calling tool"),
                        stakai::ContentPart::tool_call(
                            "tc_1",
                            "stakpak__view",
                            json!({"path":"README.md"}),
                        ),
                    ],
                ),
            ],
            json!({"source": "test"}),
        );

        let save = state
            .checkpoint_store
            .save_latest(session_uuid, &envelope)
            .await;
        assert!(save.is_ok(), "checkpoint save should succeed");

        let request = match Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{session_id}/messages"))
            .header(AUTHORIZATION, "Bearer secret")
            .body(Body::empty())
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build messages request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("messages request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::OK);

        let body = match to_bytes(response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read messages body: {error}"),
        };

        let parsed: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(error) => panic!("invalid messages json: {error}"),
        };

        assert_eq!(parsed.get("total"), Some(&json!(2)));

        let messages = match parsed.get("messages").and_then(|value| value.as_array()) {
            Some(messages) => messages,
            None => panic!("messages payload should be an array"),
        };

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].get("role"), Some(&json!("assistant")));
        assert_eq!(
            messages[1]
                .get("content")
                .and_then(|value| value.get(1))
                .and_then(|value| value.get("type")),
            Some(&json!("tool_call"))
        );
        assert_eq!(
            messages[1]
                .get("content")
                .and_then(|value| value.get(1))
                .and_then(|value| value.get("name")),
            Some(&json!("stakpak__view"))
        );
    }

    #[tokio::test]
    async fn update_session_accepts_visibility() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let create_payload = json!({"title":"visibility-test"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create body: {error}"),
        };

        let create_json: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create json: {error}"),
        };

        let session_id = match create_json.get("id").and_then(|value| value.as_str()) {
            Some(value) => value,
            None => panic!("missing session id"),
        };

        let update_payload = json!({"visibility":"PUBLIC"});
        let update_request = match Request::builder()
            .method("PATCH")
            .uri(format!("/v1/sessions/{session_id}"))
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(update_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build update request: {error}"),
        };

        let update_response = match app.oneshot(update_request).await {
            Ok(response) => response,
            Err(error) => panic!("update request should succeed: {error}"),
        };

        assert_eq!(update_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_session_includes_runtime_config() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let create_payload = json!({"title":"runtime-config-test"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create response body: {error}"),
        };

        let created_session: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create response json: {error}"),
        };

        let session_id = match created_session.get("id").and_then(|value| value.as_str()) {
            Some(session_id) => session_id,
            None => panic!("create response missing session id"),
        };

        let get_request = match Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{session_id}"))
            .header(AUTHORIZATION, "Bearer secret")
            .body(Body::empty())
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build get request: {error}"),
        };

        let get_response = match app.oneshot(get_request).await {
            Ok(response) => response,
            Err(error) => panic!("get request should succeed: {error}"),
        };

        assert_eq!(get_response.status(), StatusCode::OK);

        let body = match to_bytes(get_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read get response body: {error}"),
        };

        let parsed: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(error) => panic!("invalid get response json: {error}"),
        };

        assert_eq!(
            parsed
                .get("config")
                .and_then(|value| value.get("default_model")),
            Some(&json!("openai/test-model"))
        );
        assert_eq!(
            parsed
                .get("config")
                .and_then(|value| value.get("auto_approve_mode")),
            Some(&json!("custom"))
        );
    }

    #[tokio::test]
    async fn delete_session_returns_no_content() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let create_payload = json!({"title":"to-delete"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create response body: {error}"),
        };

        let created_session: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create response json: {error}"),
        };

        let session_id = match created_session.get("id").and_then(|value| value.as_str()) {
            Some(session_id) => session_id,
            None => panic!("create response missing session id"),
        };

        let delete_request = match Request::builder()
            .method("DELETE")
            .uri(format!("/v1/sessions/{session_id}"))
            .header(AUTHORIZATION, "Bearer secret")
            .body(Body::empty())
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build delete request: {error}"),
        };

        let delete_response = match app.oneshot(delete_request).await {
            Ok(response) => response,
            Err(error) => panic!("delete request should succeed: {error}"),
        };

        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn create_session_is_idempotent_for_same_key_and_payload() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let payload = json!({"title":"idem"}).to_string();

        let request_a = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", "idem-key")
            .body(Body::from(payload.clone()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build first request: {error}"),
        };

        let response_a = match app.clone().oneshot(request_a).await {
            Ok(response) => response,
            Err(error) => panic!("first request should succeed: {error}"),
        };
        assert_eq!(response_a.status(), StatusCode::CREATED);

        let body_a = match to_bytes(response_a.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read first body: {error}"),
        };

        let request_b = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", "idem-key")
            .body(Body::from(payload))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build second request: {error}"),
        };

        let response_b = match app.oneshot(request_b).await {
            Ok(response) => response,
            Err(error) => panic!("second request should succeed: {error}"),
        };
        assert_eq!(response_b.status(), StatusCode::CREATED);

        let body_b = match to_bytes(response_b.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read second body: {error}"),
        };

        let json_a: serde_json::Value = match serde_json::from_slice(&body_a) {
            Ok(value) => value,
            Err(error) => panic!("first body should be valid json: {error}"),
        };
        let json_b: serde_json::Value = match serde_json::from_slice(&body_b) {
            Ok(value) => value,
            Err(error) => panic!("second body should be valid json: {error}"),
        };

        assert_eq!(json_a, json_b);
    }

    #[tokio::test]
    async fn cancel_rejects_run_mismatch() {
        let state = match test_state().await {
            Ok(state) => state,
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let app = router(state.clone(), AuthConfig::token("secret"));

        let create_payload = json!({"title":"run-mismatch"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create body: {error}"),
        };

        let session_value: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create json: {error}"),
        };

        let session_id_str = match session_value.get("id").and_then(|value| value.as_str()) {
            Some(value) => value,
            None => panic!("missing session id"),
        };

        let session_id = match Uuid::parse_str(session_id_str) {
            Ok(id) => id,
            Err(error) => panic!("invalid session id: {error}"),
        };

        let active_run_id = match state
            .run_manager
            .start_run(session_id, |_run_id| async move {
                let (command_tx, _command_rx) = tokio::sync::mpsc::channel(8);
                Ok(crate::SessionHandle::new(
                    command_tx,
                    tokio_util::sync::CancellationToken::new(),
                ))
            })
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("failed to start run: {error}"),
        };

        let mismatched_run_id = Uuid::new_v4();
        let cancel_payload = json!({"run_id": mismatched_run_id});
        let cancel_request = match Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session_id}/cancel"))
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(cancel_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build cancel request: {error}"),
        };

        let cancel_response = match app.oneshot(cancel_request).await {
            Ok(response) => response,
            Err(error) => panic!("cancel request should succeed: {error}"),
        };

        assert_eq!(cancel_response.status(), StatusCode::CONFLICT);

        let cancel_body = match to_bytes(cancel_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read cancel body: {error}"),
        };
        let cancel_json: serde_json::Value = match serde_json::from_slice(&cancel_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid cancel json: {error}"),
        };

        assert_eq!(cancel_json.get("code"), Some(&json!("run_mismatch")));

        let _ = state
            .run_manager
            .cancel_run(session_id, active_run_id)
            .await;
        let _ = state
            .run_manager
            .mark_run_finished(session_id, active_run_id, Ok(()))
            .await;
    }

    #[tokio::test]
    async fn create_session_rejects_reused_idempotency_key_with_different_payload() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let first_payload = json!({"title":"idem-a"}).to_string();
        let second_payload = json!({"title":"idem-b"}).to_string();

        let first_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", "idem-key-conflict")
            .body(Body::from(first_payload))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build first request: {error}"),
        };

        let first_response = match app.clone().oneshot(first_request).await {
            Ok(response) => response,
            Err(error) => panic!("first request should succeed: {error}"),
        };
        assert_eq!(first_response.status(), StatusCode::CREATED);

        let second_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", "idem-key-conflict")
            .body(Body::from(second_payload))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build second request: {error}"),
        };

        let second_response = match app.oneshot(second_request).await {
            Ok(response) => response,
            Err(error) => panic!("second request should succeed: {error}"),
        };
        assert_eq!(second_response.status(), StatusCode::CONFLICT);

        let second_body = match to_bytes(second_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read second body: {error}"),
        };

        let second_json: serde_json::Value = match serde_json::from_slice(&second_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid second json: {error}"),
        };

        assert_eq!(
            second_json.get("code"),
            Some(&json!("idempotency_key_reused"))
        );
    }

    #[tokio::test]
    async fn steering_requires_active_run() {
        let app = match test_state().await {
            Ok(state) => router(state.clone(), AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let create_payload = json!({"title":"steering"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create body: {error}"),
        };

        let create_json: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create json: {error}"),
        };

        let session_id = match create_json.get("id").and_then(|value| value.as_str()) {
            Some(value) => value,
            None => panic!("missing session id"),
        };

        let steering_payload = json!({
            "message": {
                "role": "user",
                "content": "steer"
            },
            "type": "steering",
            "run_id": Uuid::new_v4()
        });

        let steering_request = match Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session_id}/messages"))
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(steering_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build steering request: {error}"),
        };

        let steering_response = match app.oneshot(steering_request).await {
            Ok(response) => response,
            Err(error) => panic!("steering request should succeed: {error}"),
        };

        assert_eq!(steering_response.status(), StatusCode::CONFLICT);

        let steering_body = match to_bytes(steering_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read steering body: {error}"),
        };

        let steering_json: serde_json::Value = match serde_json::from_slice(&steering_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid steering json: {error}"),
        };

        assert_eq!(
            steering_json.get("code"),
            Some(&json!("session_not_running"))
        );
    }

    #[tokio::test]
    async fn message_rejects_mismatched_run_id_when_active_run_exists() {
        let state = match test_state().await {
            Ok(state) => state,
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let app = router(state.clone(), AuthConfig::token("secret"));

        let create_payload = json!({"title":"mismatch"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create body: {error}"),
        };

        let create_json: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create json: {error}"),
        };

        let session_id = match create_json.get("id").and_then(|value| value.as_str()) {
            Some(value) => value,
            None => panic!("missing session id"),
        };

        let session_uuid = match Uuid::parse_str(session_id) {
            Ok(value) => value,
            Err(error) => panic!("invalid session id: {error}"),
        };

        let active_run_id = match state
            .run_manager
            .start_run(session_uuid, |_run_id| async move {
                let (command_tx, _command_rx) = tokio::sync::mpsc::channel(8);
                Ok(crate::SessionHandle::new(
                    command_tx,
                    tokio_util::sync::CancellationToken::new(),
                ))
            })
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("failed to start run: {error}"),
        };

        let mismatch_payload = json!({
            "message": {
                "role": "user",
                "content": "hello"
            },
            "type": "message",
            "run_id": Uuid::new_v4()
        });

        let mismatch_request = match Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session_id}/messages"))
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(mismatch_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build mismatch request: {error}"),
        };

        let mismatch_response = match app.oneshot(mismatch_request).await {
            Ok(response) => response,
            Err(error) => panic!("mismatch request should succeed: {error}"),
        };

        assert_eq!(mismatch_response.status(), StatusCode::CONFLICT);

        let mismatch_body = match to_bytes(mismatch_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read mismatch body: {error}"),
        };

        let mismatch_json: serde_json::Value = match serde_json::from_slice(&mismatch_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid mismatch json: {error}"),
        };

        assert_eq!(mismatch_json.get("code"), Some(&json!("run_mismatch")));

        let _ = state
            .run_manager
            .cancel_run(session_uuid, active_run_id)
            .await;
        let _ = state
            .run_manager
            .mark_run_finished(session_uuid, active_run_id, Ok(()))
            .await;
    }

    #[tokio::test]
    async fn pending_tools_endpoint_returns_pending_calls() {
        let state = match test_state().await {
            Ok(state) => state,
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let app = router(state.clone(), AuthConfig::token("secret"));

        let create_payload = json!({"title":"pending"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create body: {error}"),
        };

        let create_json: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create json: {error}"),
        };

        let session_id = match create_json.get("id").and_then(|value| value.as_str()) {
            Some(value) => value,
            None => panic!("missing session id"),
        };

        let session_uuid = match Uuid::parse_str(session_id) {
            Ok(value) => value,
            Err(error) => panic!("invalid session id: {error}"),
        };

        let run_id = Uuid::new_v4();
        state
            .set_pending_tools(
                session_uuid,
                run_id,
                vec![stakpak_agent_core::ProposedToolCall {
                    id: "tc_1".to_string(),
                    name: "stakpak__view".to_string(),
                    arguments: json!({"path":"README.md"}),
                    metadata: None,
                }],
            )
            .await;

        let pending_request = match Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{session_id}/tools/pending"))
            .header(AUTHORIZATION, "Bearer secret")
            .body(Body::empty())
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build pending request: {error}"),
        };

        let pending_response = match app.oneshot(pending_request).await {
            Ok(response) => response,
            Err(error) => panic!("pending request should succeed: {error}"),
        };

        assert_eq!(pending_response.status(), StatusCode::OK);

        let pending_body = match to_bytes(pending_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read pending body: {error}"),
        };

        let pending_json: serde_json::Value = match serde_json::from_slice(&pending_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid pending json: {error}"),
        };

        assert_eq!(pending_json.get("run_id"), Some(&json!(run_id)));
        assert_eq!(
            pending_json
                .get("tool_calls")
                .and_then(|value| value.as_array())
                .map(|array| array.len()),
            Some(1)
        );
    }

    #[tokio::test]
    async fn model_switch_rejects_run_mismatch() {
        let state = match test_state().await {
            Ok(state) => state,
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let app = router(state.clone(), AuthConfig::token("secret"));

        let create_payload = json!({"title":"model-switch"});
        let create_request = match Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(create_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build create request: {error}"),
        };

        let create_response = match app.clone().oneshot(create_request).await {
            Ok(response) => response,
            Err(error) => panic!("create request should succeed: {error}"),
        };

        let create_body = match to_bytes(create_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read create body: {error}"),
        };

        let create_json: serde_json::Value = match serde_json::from_slice(&create_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid create json: {error}"),
        };

        let session_id = match create_json.get("id").and_then(|value| value.as_str()) {
            Some(value) => value,
            None => panic!("missing session id"),
        };

        let session_uuid = match Uuid::parse_str(session_id) {
            Ok(value) => value,
            Err(error) => panic!("invalid session id: {error}"),
        };

        let active_run_id = match state
            .run_manager
            .start_run(session_uuid, |_run_id| async move {
                let (command_tx, _command_rx) = tokio::sync::mpsc::channel(8);
                Ok(crate::SessionHandle::new(
                    command_tx,
                    tokio_util::sync::CancellationToken::new(),
                ))
            })
            .await
        {
            Ok(run_id) => run_id,
            Err(error) => panic!("failed to start run: {error}"),
        };

        let switch_payload = json!({
            "run_id": Uuid::new_v4(),
            "model": "openai/test-model"
        });

        let switch_request = match Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session_id}/model"))
            .header(AUTHORIZATION, "Bearer secret")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(switch_payload.to_string()))
        {
            Ok(request) => request,
            Err(error) => panic!("failed to build switch request: {error}"),
        };

        let switch_response = match app.oneshot(switch_request).await {
            Ok(response) => response,
            Err(error) => panic!("switch request should succeed: {error}"),
        };

        assert_eq!(switch_response.status(), StatusCode::CONFLICT);

        let switch_body = match to_bytes(switch_response.into_body(), 1024 * 1024).await {
            Ok(body) => body,
            Err(error) => panic!("failed to read switch body: {error}"),
        };

        let switch_json: serde_json::Value = match serde_json::from_slice(&switch_body) {
            Ok(value) => value,
            Err(error) => panic!("invalid switch json: {error}"),
        };

        assert_eq!(switch_json.get("code"), Some(&json!("run_mismatch")));

        let _ = state
            .run_manager
            .cancel_run(session_uuid, active_run_id)
            .await;
        let _ = state
            .run_manager
            .mark_run_finished(session_uuid, active_run_id, Ok(()))
            .await;
    }

    #[tokio::test]
    async fn protected_endpoint_rejects_missing_token() {
        let app = match test_state().await {
            Ok(state) => router(state, AuthConfig::token("secret")),
            Err(error) => panic!("failed to create app state: {error}"),
        };

        let request = match Request::builder().uri("/v1/sessions").body(Body::empty()) {
            Ok(request) => request,
            Err(error) => panic!("failed to build request: {error}"),
        };

        let response = match app.oneshot(request).await {
            Ok(response) => response,
            Err(error) => panic!("request should succeed: {error}"),
        };

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
