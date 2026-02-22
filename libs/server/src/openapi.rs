#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponseDoc {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponseDoc {
    pub error: String,
    pub code: String,
    pub request_id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStateDoc {
    Idle,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RunStatusDoc {
    pub state: RunStateDoc,
    pub run_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionDoc {
    pub id: Uuid,
    pub title: String,
    pub cwd: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub run_status: RunStatusDoc,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionsResponseDoc {
    pub sessions: Vec<SessionDoc>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionDetailResponseDoc {
    pub session: SessionDoc,
    pub config: ConfigResponseDoc,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateSessionBodyDoc {
    pub title: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum SessionVisibilityDoc {
    Private,
    Public,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UpdateSessionBodyDoc {
    pub title: Option<String>,
    pub visibility: Option<SessionVisibilityDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionMessageTypeDoc {
    Message,
    Steering,
    FollowUp,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MessageRoleDoc {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetailDoc {
    Low,
    High,
    Auto,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CacheControlDoc {
    Ephemeral {
        #[serde(skip_serializing_if = "Option::is_none")]
        ttl: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AnthropicMessageOptionsDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MessageProviderOptionsDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<AnthropicMessageOptionsDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AnthropicContentPartOptionsDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ContentPartProviderOptionsDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<AnthropicContentPartOptionsDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPartDoc {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptionsDoc>,
    },
    Image {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetailDoc>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptionsDoc>,
    },
    ToolCall {
        id: String,
        name: String,
        #[schema(value_type = Object)]
        arguments: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptionsDoc>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schema(value_type = Object)]
        metadata: Option<serde_json::Value>,
    },
    ToolResult {
        tool_call_id: String,
        #[schema(value_type = Object)]
        content: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ContentPartProviderOptionsDoc>,
    },
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum MessageContentDoc {
    Text(String),
    Parts(Vec<ContentPartDoc>),
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StakaiMessageDoc {
    pub role: MessageRoleDoc,
    pub content: MessageContentDoc,
    pub name: Option<String>,
    pub provider_options: Option<MessageProviderOptionsDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CallerContextInputDoc {
    pub name: String,
    pub content: String,
    pub priority: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionMessageRequestDoc {
    pub message: StakaiMessageDoc,
    #[serde(rename = "type")]
    pub message_type: Option<SessionMessageTypeDoc>,
    pub run_id: Option<Uuid>,
    pub model: Option<String>,
    pub sandbox: Option<bool>,
    pub context: Option<Vec<CallerContextInputDoc>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionMessageResponseDoc {
    pub run_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionMessagesResponseDoc {
    pub messages: Vec<StakaiMessageDoc>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProposedToolCallDoc {
    pub id: String,
    pub name: String,
    #[schema(value_type = Object)]
    pub arguments: serde_json::Value,
    #[schema(value_type = Object)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PendingToolsResponseDoc {
    pub run_id: Option<Uuid>,
    pub tool_calls: Vec<ProposedToolCallDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionActionDoc {
    Accept,
    Reject,
    CustomResult,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DecisionInputDoc {
    pub action: DecisionActionDoc,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ToolDecisionRequestDoc {
    pub run_id: Uuid,
    #[serde(flatten)]
    pub decision: DecisionInputDoc,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ToolDecisionsRequestDoc {
    pub run_id: Uuid,
    pub decisions: HashMap<String, DecisionInputDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ToolDecisionResponseDoc {
    pub accepted: bool,
    pub run_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CancelRequestDoc {
    pub run_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CancelResponseDoc {
    pub cancelled: bool,
    pub run_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ModelSwitchRequestDoc {
    pub run_id: Uuid,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ModelSwitchResponseDoc {
    pub accepted: bool,
    pub run_id: Uuid,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoApproveModeDoc {
    None,
    All,
    Custom,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ConfigResponseDoc {
    pub default_model: Option<String>,
    pub auto_approve_mode: AutoApproveModeDoc,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ModelCostDoc {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ModelLimitDoc {
    pub context: u64,
    pub output: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ModelDoc {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub reasoning: bool,
    pub cost: Option<ModelCostDoc>,
    pub limit: ModelLimitDoc,
    pub release_date: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ModelsResponseDoc {
    pub models: Vec<ModelDoc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventEnvelopeDoc {
    pub id: u64,
    pub session_id: Uuid,
    pub run_id: Option<Uuid>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[schema(value_type = Object)]
    pub event: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct GapDetectedDoc {
    pub requested_after_id: u64,
    pub oldest_available_id: u64,
    pub newest_available_id: u64,
    pub resume_hint: String,
}

#[utoipa::path(
    get,
    path = "/v1/health",
    responses((status = 200, body = HealthResponseDoc))
)]
fn health_doc() {}

#[utoipa::path(
    get,
    path = "/v1/openapi.json",
    responses((status = 200, description = "OpenAPI 3.1 specification document"))
)]
fn openapi_doc() {}

#[utoipa::path(
    get,
    path = "/v1/sessions",
    security(("bearer_auth" = [])),
    params(
        ("limit" = Option<u32>, Query, description = "page size"),
        ("offset" = Option<u32>, Query, description = "page offset"),
        ("search" = Option<String>, Query, description = "title query"),
        ("status" = Option<String>, Query, description = "ACTIVE or DELETED")
    ),
    responses(
        (status = 200, body = SessionsResponseDoc),
        (status = 401, body = ErrorResponseDoc)
    )
)]
fn list_sessions_doc() {}

#[utoipa::path(
    post,
    path = "/v1/sessions",
    security(("bearer_auth" = [])),
    request_body = CreateSessionBodyDoc,
    responses(
        (status = 201, body = SessionDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 409, body = ErrorResponseDoc)
    )
)]
fn create_session_doc() {}

#[utoipa::path(
    get,
    path = "/v1/sessions/{id}",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    responses(
        (status = 200, body = SessionDetailResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 404, body = ErrorResponseDoc)
    )
)]
fn get_session_doc() {}

#[utoipa::path(
    patch,
    path = "/v1/sessions/{id}",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    request_body = UpdateSessionBodyDoc,
    responses(
        (status = 200, body = SessionDetailResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 404, body = ErrorResponseDoc)
    )
)]
fn update_session_doc() {}

#[utoipa::path(
    delete,
    path = "/v1/sessions/{id}",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    responses(
        (status = 204, description = "Session deleted"),
        (status = 401, body = ErrorResponseDoc),
        (status = 404, body = ErrorResponseDoc)
    )
)]
fn delete_session_doc() {}

#[utoipa::path(
    post,
    path = "/v1/sessions/{id}/messages",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    request_body = SessionMessageRequestDoc,
    responses(
        (status = 200, body = SessionMessageResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 404, body = ErrorResponseDoc),
        (status = 409, body = ErrorResponseDoc)
    )
)]
fn post_messages_doc() {}

#[utoipa::path(
    get,
    path = "/v1/sessions/{id}/messages",
    security(("bearer_auth" = [])),
    params(
        ("id" = Uuid, Path, description = "session id"),
        ("limit" = Option<usize>, Query, description = "page size"),
        ("offset" = Option<usize>, Query, description = "page offset")
    ),
    responses(
        (status = 200, body = SessionMessagesResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 404, body = ErrorResponseDoc)
    )
)]
fn get_messages_doc() {}

#[utoipa::path(
    get,
    path = "/v1/sessions/{id}/events",
    security(("bearer_auth" = [])),
    params(
        ("id" = Uuid, Path, description = "session id"),
        ("Last-Event-ID" = Option<u64>, Header, description = "Replay cursor; stream events with id > Last-Event-ID")
    ),
    responses((
        status = 200,
        description = "text/event-stream of EventEnvelope frames and optional gap_detected control event",
        content_type = "text/event-stream"
    ))
)]
fn events_doc() {}

#[utoipa::path(
    get,
    path = "/v1/sessions/{id}/tools/pending",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    responses(
        (status = 200, body = PendingToolsResponseDoc),
        (status = 401, body = ErrorResponseDoc)
    )
)]
fn pending_tools_doc() {}

#[utoipa::path(
    post,
    path = "/v1/sessions/{id}/tools/{tool_call_id}/decision",
    security(("bearer_auth" = [])),
    params(
        ("id" = Uuid, Path, description = "session id"),
        ("tool_call_id" = String, Path, description = "tool call id")
    ),
    request_body = ToolDecisionRequestDoc,
    responses(
        (status = 200, body = ToolDecisionResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 409, body = ErrorResponseDoc)
    )
)]
fn tool_decision_doc() {}

#[utoipa::path(
    post,
    path = "/v1/sessions/{id}/tools/decisions",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    request_body = ToolDecisionsRequestDoc,
    responses(
        (status = 200, body = ToolDecisionResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 409, body = ErrorResponseDoc)
    )
)]
fn tool_decisions_doc() {}

#[utoipa::path(
    post,
    path = "/v1/sessions/{id}/tools/resolve",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    request_body = ToolDecisionsRequestDoc,
    responses(
        (status = 200, body = ToolDecisionResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 409, body = ErrorResponseDoc)
    )
)]
fn tool_resolve_doc() {}

#[utoipa::path(
    post,
    path = "/v1/sessions/{id}/cancel",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    request_body = CancelRequestDoc,
    responses(
        (status = 200, body = CancelResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 409, body = ErrorResponseDoc)
    )
)]
fn cancel_doc() {}

#[utoipa::path(
    post,
    path = "/v1/sessions/{id}/model",
    security(("bearer_auth" = [])),
    params(("id" = Uuid, Path, description = "session id")),
    request_body = ModelSwitchRequestDoc,
    responses(
        (status = 200, body = ModelSwitchResponseDoc),
        (status = 401, body = ErrorResponseDoc),
        (status = 409, body = ErrorResponseDoc)
    )
)]
fn switch_model_doc() {}

#[utoipa::path(
    get,
    path = "/v1/models",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = ModelsResponseDoc),
        (status = 401, body = ErrorResponseDoc)
    )
)]
fn models_doc() {}

#[utoipa::path(
    get,
    path = "/v1/config",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, body = ConfigResponseDoc),
        (status = 401, body = ErrorResponseDoc)
    )
)]
fn config_doc() {}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("Bearer")
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health_doc,
        openapi_doc,
        list_sessions_doc,
        create_session_doc,
        get_session_doc,
        update_session_doc,
        delete_session_doc,
        post_messages_doc,
        get_messages_doc,
        events_doc,
        pending_tools_doc,
        tool_decision_doc,
        tool_decisions_doc,
        tool_resolve_doc,
        cancel_doc,
        switch_model_doc,
        models_doc,
        config_doc,
    ),
    components(
        schemas(
            HealthResponseDoc,
            ErrorResponseDoc,
            RunStateDoc,
            RunStatusDoc,
            SessionDoc,
            SessionsResponseDoc,
            SessionDetailResponseDoc,
            CreateSessionBodyDoc,
            SessionVisibilityDoc,
            UpdateSessionBodyDoc,
            SessionMessageTypeDoc,
            MessageRoleDoc,
            ImageDetailDoc,
            CacheControlDoc,
            AnthropicMessageOptionsDoc,
            MessageProviderOptionsDoc,
            AnthropicContentPartOptionsDoc,
            ContentPartProviderOptionsDoc,
            ContentPartDoc,
            MessageContentDoc,
            StakaiMessageDoc,
            SessionMessageRequestDoc,
            SessionMessageResponseDoc,
            SessionMessagesResponseDoc,
            ProposedToolCallDoc,
            PendingToolsResponseDoc,
            DecisionActionDoc,
            DecisionInputDoc,
            ToolDecisionRequestDoc,
            ToolDecisionsRequestDoc,
            ToolDecisionResponseDoc,
            CancelRequestDoc,
            CancelResponseDoc,
            ModelSwitchRequestDoc,
            ModelSwitchResponseDoc,
            AutoApproveModeDoc,
            ConfigResponseDoc,
            ModelCostDoc,
            ModelLimitDoc,
            ModelDoc,
            ModelsResponseDoc,
            EventEnvelopeDoc,
            GapDetectedDoc,
        )
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "server", description = "Stakpak server runtime APIs")
    )
)]
pub struct ApiDoc;

pub fn generate_openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}
