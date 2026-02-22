use std::{collections::HashMap, pin::Pin};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stakpak_agent_core::ProposedToolCall;
pub use stakpak_shared::models::context::{
    CallerContextInput, MAX_CALLER_CONTEXT_CONTENT_CHARS, MAX_CALLER_CONTEXT_ITEMS,
    MAX_CALLER_CONTEXT_NAME_CHARS, validate_caller_context,
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StakpakClient {
    base_url: String,
    auth_token: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub id: Uuid,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionResponse>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionDetailResponse {
    pub session: SessionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub run_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMessagesResponse {
    pub messages: Vec<stakai::Message>,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Message,
    Steering,
    FollowUp,
}

#[derive(Debug, Clone)]
pub struct SendMessageOptions {
    pub model: Option<String>,
    pub message_type: MessageType,
    pub run_id: Option<Uuid>,
    pub sandbox: Option<bool>,
    pub context: Vec<CallerContextInput>,
}

impl Default for SendMessageOptions {
    fn default() -> Self {
        Self {
            model: None,
            message_type: MessageType::Message,
            run_id: None,
            sandbox: None,
            context: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingToolsResponse {
    pub run_id: Option<Uuid>,
    pub tool_calls: Vec<ProposedToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDecisionAction {
    Accept,
    Reject,
    CustomResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDecisionInput {
    pub action: ToolDecisionAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

pub struct EventStream {
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buffer: String,
}

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub id: Option<String>,
    pub event_id_u64: Option<u64>,
    pub event_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCompletedPayload {
    #[serde(default)]
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub total_turns: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunErrorPayload {
    #[serde(default)]
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallsProposedPayload {
    #[serde(default)]
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub tool_calls: Vec<ProposedToolCall>,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("server returned {status}: {body}")]
    ApiError {
        status: u16,
        code: String,
        body: String,
    },

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("session already running")]
    Conflict,

    #[error("sse parse error: {0}")]
    SseParse(String),

    #[error("connection failed: {0}")]
    Connection(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

impl StakpakClient {
    pub fn new(base_url: String, auth_token: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_token,
            http: reqwest::Client::new(),
        }
    }

    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        let mut last_error: Option<ClientError> = None;
        for _ in 0..3 {
            match self
                .request_json::<HealthResponse>(reqwest::Method::GET, "/v1/health", None)
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| ClientError::Connection("health check failed".to_string())))
    }

    pub async fn create_session(&self, title: &str) -> Result<CreateSessionResponse, ClientError> {
        let payload = serde_json::json!({ "title": title });
        self.request_json(reqwest::Method::POST, "/v1/sessions", Some(payload))
            .await
    }

    pub async fn get_session(&self, session_id: &str) -> Result<SessionResponse, ClientError> {
        let response: SessionDetailResponse = self
            .request_json(
                reqwest::Method::GET,
                &format!("/v1/sessions/{session_id}"),
                None,
            )
            .await?;

        Ok(response.session)
    }

    pub async fn list_sessions(&self) -> Result<ListSessionsResponse, ClientError> {
        self.request_json(reqwest::Method::GET, "/v1/sessions", None)
            .await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<(), ClientError> {
        let response = self
            .request(
                reqwest::Method::DELETE,
                &format!("/v1/sessions/{session_id}"),
                None,
            )
            .await?;
        map_error_status(response.status(), response.text().await.unwrap_or_default())?;
        Ok(())
    }

    pub async fn send_messages(
        &self,
        session_id: &str,
        messages: Vec<stakai::Message>,
        opts: SendMessageOptions,
    ) -> Result<SendMessageResponse, ClientError> {
        let message = if let Some(message) = messages.into_iter().last() {
            message
        } else {
            return Err(ClientError::InvalidRequest(
                "send_messages requires at least one message".to_string(),
            ));
        };

        let SendMessageOptions {
            model,
            message_type,
            run_id,
            sandbox,
            context,
        } = opts;

        validate_context_inputs(&context)?;

        let payload = serde_json::json!({
            "message": message,
            "type": message_type,
            "run_id": run_id,
            "model": model,
            "sandbox": sandbox,
            "context": if context.is_empty() { None::<Vec<CallerContextInput>> } else { Some(context) },
        });

        self.request_json(
            reqwest::Method::POST,
            &format!("/v1/sessions/{session_id}/messages"),
            Some(payload),
        )
        .await
    }

    pub async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<GetMessagesResponse, ClientError> {
        self.request_json(
            reqwest::Method::GET,
            &format!("/v1/sessions/{session_id}/messages?limit={limit}&offset={offset}"),
            None,
        )
        .await
    }

    pub async fn subscribe_events(
        &self,
        session_id: &str,
        last_event_id: Option<u64>,
    ) -> Result<EventStream, ClientError> {
        let mut request = self
            .http
            .get(format!("{}/v1/sessions/{session_id}/events", self.base_url));

        if !self.auth_token.is_empty() {
            request = request.bearer_auth(&self.auth_token);
        }

        if let Some(last_event_id) = last_event_id {
            request = request.header("Last-Event-ID", last_event_id.to_string());
        }

        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return match map_error_status(status, body) {
                Ok(()) => Err(ClientError::ApiError {
                    status: status.as_u16(),
                    code: "api_error".to_string(),
                    body: "unexpected non-success status".to_string(),
                }),
                Err(error) => Err(error),
            };
        }

        Ok(EventStream {
            stream: Box::pin(response.bytes_stream()),
            buffer: String::new(),
        })
    }

    pub async fn pending_tools(
        &self,
        session_id: &str,
    ) -> Result<PendingToolsResponse, ClientError> {
        self.request_json(
            reqwest::Method::GET,
            &format!("/v1/sessions/{session_id}/tools/pending"),
            None,
        )
        .await
    }

    pub async fn resolve_tools(
        &self,
        session_id: &str,
        run_id: &str,
        decisions: HashMap<String, ToolDecisionInput>,
    ) -> Result<(), ClientError> {
        let payload = serde_json::json!({
            "run_id": run_id,
            "decisions": decisions,
        });

        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/v1/sessions/{session_id}/tools/decisions"),
                Some(payload),
            )
            .await?;

        map_error_status(response.status(), response.text().await.unwrap_or_default())?;
        Ok(())
    }

    pub async fn cancel_run(&self, session_id: &str, run_id: &str) -> Result<(), ClientError> {
        let payload = serde_json::json!({ "run_id": run_id });
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/v1/sessions/{session_id}/cancel"),
                Some(payload),
            )
            .await?;

        map_error_status(response.status(), response.text().await.unwrap_or_default())?;
        Ok(())
    }

    async fn request_json<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, ClientError> {
        let response = self.request(method, path, body).await?;
        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return match map_error_status(status, body_text) {
                Ok(()) => Err(ClientError::ApiError {
                    status: status.as_u16(),
                    code: "api_error".to_string(),
                    body: "unexpected non-success status".to_string(),
                }),
                Err(error) => Err(error),
            };
        }

        Ok(response.json().await?)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response, ClientError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path));

        if !self.auth_token.is_empty() {
            request = request.bearer_auth(&self.auth_token);
        }

        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request.send().await.map_err(|error| {
            if error.is_connect() {
                ClientError::Connection(error.to_string())
            } else {
                ClientError::Http(error)
            }
        })?;

        Ok(response)
    }
}

impl EventStream {
    pub async fn next_event(&mut self) -> Result<Option<SseEvent>, ClientError> {
        loop {
            if let Some(event) = try_take_event(&mut self.buffer)? {
                return Ok(Some(event));
            }

            let next = self.stream.next().await;
            let Some(chunk) = next else {
                if self.buffer.trim().is_empty() {
                    return Ok(None);
                }

                let event = parse_sse_block(&self.buffer)?;
                self.buffer.clear();
                if event.data.is_empty() && event.event_type == "message" {
                    return Ok(None);
                }
                return Ok(Some(event));
            };

            let bytes = chunk.map_err(ClientError::Http)?;
            let text = String::from_utf8(bytes.to_vec()).map_err(|error| {
                ClientError::SseParse(format!("invalid utf-8 in stream: {error}"))
            })?;
            self.buffer.push_str(&text);
        }
    }
}

impl SseEvent {
    pub fn run_id(&self) -> Option<String> {
        let envelope = parse_event_envelope(&self.data).ok()?;

        if let Some(run_id) = envelope.run_id {
            return Some(run_id);
        }

        match envelope.event {
            Some(Value::Object(map)) => {
                let (_variant, payload) = map.into_iter().next()?;
                payload.get("run_id")?.as_str().map(ToOwned::to_owned)
            }
            _ => None,
        }
    }

    pub fn as_text_delta(&self) -> Option<String> {
        let envelope = parse_event_envelope(&self.data).ok()?;
        extract_variant_payload(&envelope.event, "TextDelta")?
            .get("delta")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
    }

    pub fn as_run_completed(&self) -> Option<RunCompletedPayload> {
        let envelope = parse_event_envelope(&self.data).ok()?;
        let payload = extract_variant_payload(&envelope.event, "RunCompleted")?;
        serde_json::from_value(payload.clone()).ok()
    }

    pub fn as_run_error(&self) -> Option<RunErrorPayload> {
        let envelope = parse_event_envelope(&self.data).ok()?;
        let payload = extract_variant_payload(&envelope.event, "RunError")?;
        serde_json::from_value(payload.clone()).ok()
    }

    pub fn as_tool_calls_proposed(&self) -> Option<ToolCallsProposedPayload> {
        let envelope = parse_event_envelope(&self.data).ok()?;
        let payload = extract_variant_payload(&envelope.event, "ToolCallsProposed")?;
        serde_json::from_value(payload.clone()).ok()
    }

    pub fn is_run_terminal(&self) -> bool {
        self.event_type == "run_completed" || self.event_type == "run_error"
    }
}

#[derive(Debug, Deserialize)]
struct EnvelopeData {
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    event: Option<Value>,
}

fn parse_event_envelope(data: &str) -> Result<EnvelopeData, ClientError> {
    serde_json::from_str(data).map_err(|error| ClientError::SseParse(error.to_string()))
}

fn extract_variant_payload<'a>(event: &'a Option<Value>, variant: &str) -> Option<&'a Value> {
    let event = event.as_ref()?;
    let map = event.as_object()?;
    map.get(variant)
}

/// index from find() of ASCII separators ("\n\n", "\r\n\r\n") on same string
#[allow(clippy::string_slice)]
fn try_take_event(buffer: &mut String) -> Result<Option<SseEvent>, ClientError> {
    let separator = if let Some(index) = buffer.find("\n\n") {
        Some((index, 2_usize))
    } else {
        buffer.find("\r\n\r\n").map(|index| (index, 4_usize))
    };

    let Some((index, separator_len)) = separator else {
        return Ok(None);
    };

    let block = buffer[..index].to_string();
    let remaining = buffer[index + separator_len..].to_string();
    *buffer = remaining;

    if block.trim().is_empty() {
        return Ok(None);
    }

    let event = parse_sse_block(&block)?;
    Ok(Some(event))
}

fn parse_sse_block(block: &str) -> Result<SseEvent, ClientError> {
    let mut id: Option<String> = None;
    let mut event_type = String::from("message");
    let mut data_lines = Vec::new();

    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("id:") {
            id = Some(rest.trim().to_string());
            continue;
        }

        if let Some(rest) = line.strip_prefix("event:") {
            event_type = rest.trim().to_string();
            continue;
        }

        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }

    let event_id_u64 = id.as_ref().and_then(|value| value.parse::<u64>().ok());

    Ok(SseEvent {
        id,
        event_id_u64,
        event_type,
        data: data_lines.join("\n"),
    })
}

fn map_error_status(status: StatusCode, body: String) -> Result<(), ClientError> {
    if status.is_success() {
        return Ok(());
    }

    match status {
        StatusCode::NOT_FOUND => Err(ClientError::NotFound(body)),
        StatusCode::CONFLICT => Err(ClientError::Conflict),
        _ => Err(ClientError::ApiError {
            status: status.as_u16(),
            code: "api_error".to_string(),
            body,
        }),
    }
}

fn validate_context_inputs(inputs: &[CallerContextInput]) -> Result<(), ClientError> {
    validate_caller_context(inputs).map_err(ClientError::InvalidRequest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_event_block() {
        let block = "id:1\nevent:text_delta\ndata:{\"id\":1}\n";
        let event = match parse_sse_block(block) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse block: {error}"),
        };

        assert_eq!(event.id.as_deref(), Some("1"));
        assert_eq!(event.event_type, "text_delta");
        assert_eq!(event.data, "{\"id\":1}");
        assert_eq!(event.event_id_u64, Some(1));
    }

    #[test]
    fn validate_context_inputs_accepts_exact_limits() {
        let input = CallerContextInput {
            name: "n".repeat(MAX_CALLER_CONTEXT_NAME_CHARS),
            content: "x".repeat(MAX_CALLER_CONTEXT_CONTENT_CHARS),
            priority: Some("high".to_string()),
        };

        assert!(validate_context_inputs(&[input]).is_ok());
    }

    #[test]
    fn validate_context_inputs_rejects_oversized_content() {
        let input = CallerContextInput {
            name: "valid".to_string(),
            content: "x".repeat(MAX_CALLER_CONTEXT_CONTENT_CHARS + 1),
            priority: None,
        };

        assert!(validate_context_inputs(&[input]).is_err());
    }

    #[test]
    fn validate_context_inputs_rejects_oversized_whitespace_only_name() {
        let input = CallerContextInput {
            name: " ".repeat(MAX_CALLER_CONTEXT_NAME_CHARS + 1),
            content: "ok".to_string(),
            priority: None,
        };

        assert!(
            validate_context_inputs(&[input]).is_err(),
            "raw name length must be enforced even when trimmed name is empty"
        );
    }

    #[test]
    fn validate_context_inputs_rejects_oversized_trimmed_name() {
        let input = CallerContextInput {
            name: "n".repeat(MAX_CALLER_CONTEXT_NAME_CHARS + 1),
            content: "ok".to_string(),
            priority: None,
        };

        assert!(validate_context_inputs(&[input]).is_err());
    }

    #[test]
    fn validate_context_inputs_rejects_oversized_whitespace_only_content() {
        let input = CallerContextInput {
            name: "ctx".to_string(),
            content: " ".repeat(MAX_CALLER_CONTEXT_CONTENT_CHARS + 1),
            priority: None,
        };

        assert!(
            validate_context_inputs(&[input]).is_err(),
            "raw content length must be enforced even when trimmed content is empty"
        );
    }
}
