//! End-to-end tests for `stakpak sessions` driven against an in-memory
//! `LocalStorage`, so we exercise real storage + filtering + rendering without
//! spinning up the CLI binary or an HTTP server.

use std::sync::Arc;

use stakpak_api::{
    BackendInfo, Checkpoint, CheckpointState, ListSessionsQuery, LocalStorage, Session,
    SessionStatus, SessionStorage, SessionVisibility,
    StorageCreateSessionRequest as CreateSessionRequest, StorageError,
};
use stakpak_shared::models::integrations::openai::{ChatMessage, MessageContent, Role};
use uuid::Uuid;

use super::classify_storage_error;
use super::messages::{RoleFilter, filter_messages};
use super::output::{self, OutputMode, render_error};

async fn in_memory_storage() -> LocalStorage {
    LocalStorage::new(":memory:")
        .await
        .expect("in-memory storage")
}

fn msg(role: Role, text: &str) -> ChatMessage {
    ChatMessage {
        role,
        content: Some(MessageContent::String(text.to_string())),
        ..Default::default()
    }
}

fn assistant_tool_call_msg(name: &str, tool_call_id: &str) -> ChatMessage {
    use stakpak_shared::models::integrations::openai::{FunctionCall, ToolCall};
    ChatMessage {
        role: Role::Assistant,
        content: Some(MessageContent::String("calling a tool".to_string())),
        tool_calls: Some(vec![ToolCall {
            id: tool_call_id.to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: "{}".to_string(),
            },
            metadata: None,
        }]),
        ..Default::default()
    }
}

fn local_backend() -> BackendInfo {
    BackendInfo::local("/tmp/.stakpak/data/local.db")
}

fn remote_backend() -> BackendInfo {
    BackendInfo::stakpak_api(Some("dev".to_string()), "https://api.stakpak.test")
}

fn render_list(sessions: &[stakpak_api::SessionSummary], mode: OutputMode) -> String {
    output::render_list(sessions, &local_backend(), mode)
}

fn render_show(
    session: &Session,
    messages: &[ChatMessage],
    profile: Option<&str>,
    mode: OutputMode,
) -> String {
    output::render_show(session, messages, &local_backend(), profile, mode)
}

// =============================================================================
// 5.2 — `stakpak sessions list --json` end-to-end
// =============================================================================

#[tokio::test]
async fn sessions_list_json_emits_array_of_session_summaries() {
    let storage = in_memory_storage().await;

    let _ = storage
        .create_session(&CreateSessionRequest::new(
            "terraform refactor",
            vec![msg(Role::User, "rewrite the module")],
        ))
        .await
        .unwrap();
    let _ = storage
        .create_session(&CreateSessionRequest::new(
            "k8s upgrade",
            vec![msg(Role::User, "bump versions")],
        ))
        .await
        .unwrap();

    let result = storage
        .list_sessions(&ListSessionsQuery::new().with_limit(20))
        .await
        .unwrap();
    assert_eq!(result.sessions.len(), 2);

    let rendered = render_list(&result.sessions, OutputMode::Json);
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
    let obj = value.as_object().expect("object at top level");
    let backend = obj
        .get("backend")
        .and_then(serde_json::Value::as_object)
        .expect("backend object");
    assert_eq!(
        backend.get("kind").and_then(serde_json::Value::as_str),
        Some("local")
    );
    assert_eq!(
        backend
            .get("store_path")
            .and_then(serde_json::Value::as_str),
        Some("/tmp/.stakpak/data/local.db")
    );

    let arr = obj
        .get("sessions")
        .and_then(serde_json::Value::as_array)
        .expect("sessions array at top level");
    assert_eq!(arr.len(), 2);

    for s in arr {
        let obj = s.as_object().expect("object");
        for field in &[
            "id",
            "title",
            "status",
            "visibility",
            "message_count",
            "created_at",
            "updated_at",
        ] {
            assert!(
                obj.contains_key(*field),
                "expected field `{}` in session summary JSON",
                field
            );
        }
    }
}

#[tokio::test]
async fn sessions_list_json_empty_storage_is_empty_array() {
    let storage = in_memory_storage().await;
    let result = storage
        .list_sessions(&ListSessionsQuery::new())
        .await
        .unwrap();

    let rendered = render_list(&result.sessions, OutputMode::Json);
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
    let obj = value.as_object().expect("object at top level");
    assert!(
        obj.get("sessions")
            .and_then(serde_json::Value::as_array)
            .expect("sessions array")
            .is_empty()
    );
    assert_eq!(obj["backend"]["kind"].as_str(), Some("local"));
}

#[tokio::test]
async fn sessions_list_json_respects_search_filter() {
    let storage = in_memory_storage().await;
    let _ = storage
        .create_session(&CreateSessionRequest::new(
            "terraform refactor",
            vec![msg(Role::User, "a")],
        ))
        .await
        .unwrap();
    let _ = storage
        .create_session(&CreateSessionRequest::new(
            "docker images",
            vec![msg(Role::User, "b")],
        ))
        .await
        .unwrap();

    let result = storage
        .list_sessions(&ListSessionsQuery::new().with_search("terraform"))
        .await
        .unwrap();

    let rendered = render_list(&result.sessions, OutputMode::Json);
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    let arr = value["sessions"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(value["backend"]["kind"].as_str(), Some("local"));
    assert!(
        arr[0]["title"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("terraform")
    );
}

#[tokio::test]
async fn sessions_list_human_shows_header_when_non_empty() {
    let storage = in_memory_storage().await;
    let _ = storage
        .create_session(&CreateSessionRequest::new(
            "demo",
            vec![msg(Role::User, "hi")],
        ))
        .await
        .unwrap();

    let result = storage
        .list_sessions(&ListSessionsQuery::new())
        .await
        .unwrap();
    let rendered = render_list(&result.sessions, OutputMode::Human);
    assert!(rendered.starts_with("Backend: local (/tmp/.stakpak/data/local.db)\n"));
    assert!(rendered.contains("ID"));
    assert!(rendered.contains("TITLE"));
    assert!(rendered.contains("MSGS"));
    assert!(rendered.contains("demo"));
}

#[tokio::test]
async fn sessions_list_human_empty_storage_is_friendly_message() {
    let rendered = render_list(&[], OutputMode::Human);
    assert!(rendered.starts_with("Backend: local (/tmp/.stakpak/data/local.db)\n"));
    assert!(rendered.contains("No sessions found"));
}

#[test]
fn render_list_human_includes_local_backend_header() {
    let rendered = output::render_list(&[], &local_backend(), OutputMode::Human);
    assert!(rendered.starts_with("Backend: local (/tmp/.stakpak/data/local.db)\n"));
}

#[test]
fn render_show_human_includes_remote_backend_header() {
    let session = fake_session(Uuid::new_v4(), true);
    let rendered = output::render_show(
        &session,
        &[],
        &remote_backend(),
        Some("dev"),
        OutputMode::Human,
    );
    assert!(
        rendered.starts_with(
            "Backend: stakpak-api (profile: dev, endpoint: https://api.stakpak.test)\n"
        ),
        "unexpected header: {rendered}"
    );
}

// =============================================================================
// 5.3 — `stakpak sessions show <id> --role assistant --last --json` end-to-end
// =============================================================================

#[tokio::test]
async fn sessions_show_role_assistant_last_json_returns_one_assistant_message() {
    let storage = in_memory_storage().await;

    let result = storage
        .create_session(&CreateSessionRequest::new(
            "chat",
            vec![
                msg(Role::System, "you are helpful"),
                msg(Role::User, "hello"),
                msg(Role::Assistant, "hi there"),
                msg(Role::User, "what time is it?"),
                msg(Role::Assistant, "I don't have a clock"),
                msg(Role::User, "bye"),
            ],
        ))
        .await
        .unwrap();

    let session_id: Uuid = result.session_id;
    let session = storage.get_session(session_id).await.unwrap();
    let checkpoint = storage.get_active_checkpoint(session_id).await.unwrap();

    let filtered = filter_messages(
        checkpoint.state.messages,
        Some(RoleFilter::Assistant),
        true,
        None,
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].role, Role::Assistant);

    let rendered = render_show(&session, &filtered, None, OutputMode::Json);
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

    // Object-shaped (not an array).
    let obj = value.as_object().expect("object at top level");
    for field in &[
        "id",
        "title",
        "status",
        "visibility",
        "created_at",
        "updated_at",
        "messages",
        "backend",
    ] {
        assert!(
            obj.contains_key(*field),
            "missing field `{}` in show JSON",
            field
        );
    }
    assert_eq!(obj["backend"]["kind"].as_str(), Some("local"));

    let messages = obj.get("messages").unwrap().as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"].as_str().unwrap(), "assistant");
    assert_eq!(
        messages[0]["content"].as_str().unwrap(),
        "I don't have a clock"
    );
    assert_eq!(
        obj.get("id").unwrap().as_str().unwrap(),
        session_id.to_string()
    );
}

#[tokio::test]
async fn sessions_show_human_includes_resume_hint_and_metadata() {
    let storage = in_memory_storage().await;
    let result = storage
        .create_session(
            &CreateSessionRequest::new("demo", vec![msg(Role::User, "hi")])
                .with_visibility(SessionVisibility::Private)
                .with_cwd("/tmp/proj"),
        )
        .await
        .unwrap();

    let session_id = result.session_id;
    let session = storage.get_session(session_id).await.unwrap();
    let checkpoint = storage.get_active_checkpoint(session_id).await.unwrap();

    let rendered = render_show(
        &session,
        &checkpoint.state.messages,
        None,
        OutputMode::Human,
    );
    assert!(rendered.starts_with("Backend: local (/tmp/.stakpak/data/local.db)\n"));
    assert!(rendered.contains(&format!("Resume: stakpak --session {}", session_id)));
    assert!(rendered.contains("demo"));
    assert!(rendered.contains("/tmp/proj"));
    assert!(rendered.contains("user"));
    assert!(rendered.contains("hi"));
}

#[tokio::test]
async fn sessions_show_json_roundtrip_preserves_tool_calls() {
    let storage = in_memory_storage().await;
    let result = storage
        .create_session(&CreateSessionRequest::new(
            "tools",
            vec![
                msg(Role::User, "do the thing"),
                assistant_tool_call_msg("run_command", "tc_1"),
            ],
        ))
        .await
        .unwrap();

    let session_id = result.session_id;
    let session = storage.get_session(session_id).await.unwrap();
    let checkpoint = storage.get_active_checkpoint(session_id).await.unwrap();

    let rendered = render_show(&session, &checkpoint.state.messages, None, OutputMode::Json);
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    let messages = value["messages"].as_array().unwrap();
    let assistant = messages.iter().find(|m| m["role"] == "assistant").unwrap();
    let tool_calls = assistant["tool_calls"].as_array().unwrap();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["id"].as_str().unwrap(), "tc_1");
    assert_eq!(
        tool_calls[0]["function"]["name"].as_str().unwrap(),
        "run_command"
    );
}

#[tokio::test]
async fn sessions_show_json_is_object_not_array() {
    let storage = in_memory_storage().await;
    let result = storage
        .create_session(&CreateSessionRequest::new("x", vec![msg(Role::User, "hi")]))
        .await
        .unwrap();
    let session = storage.get_session(result.session_id).await.unwrap();
    let cp = storage
        .get_active_checkpoint(result.session_id)
        .await
        .unwrap();

    let rendered = render_show(&session, &cp.state.messages, None, OutputMode::Json);
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert!(
        value.is_object(),
        "show JSON must be an object, not an array"
    );
    assert_eq!(value["backend"]["kind"].as_str(), Some("local"));
}

#[tokio::test]
async fn sessions_cli_json_roundtrip_list_then_show_does_not_surface_not_found() {
    let storage: Arc<dyn SessionStorage> = Arc::new(in_memory_storage().await);
    let created = storage
        .create_session(&CreateSessionRequest::new(
            "round trip",
            vec![msg(Role::User, "hello")],
        ))
        .await
        .unwrap();

    let list_json = super::list_sessions_output(storage.clone(), None, 20, 0, OutputMode::Json)
        .await
        .expect("list output should succeed");
    let list_value: serde_json::Value = serde_json::from_str(&list_json).expect("valid list JSON");
    let session_id = list_value["sessions"][0]["id"]
        .as_str()
        .expect("session id in list output");

    let show_json = super::show_session_output(
        storage,
        created.session_id,
        None,
        false,
        None,
        Some("default"),
        OutputMode::Json,
    )
    .await
    .expect("show output should succeed");
    let show_value: serde_json::Value = serde_json::from_str(&show_json).expect("valid show JSON");

    assert_eq!(session_id, created.session_id.to_string());
    assert_eq!(show_value["id"].as_str(), Some(session_id));
    assert_eq!(show_value["title"].as_str(), Some("round trip"));
}

// =============================================================================
// Branch coverage for `output.rs`
// =============================================================================

fn fake_session(id: Uuid, with_checkpoint: bool) -> Session {
    let now = chrono::Utc::now();
    let checkpoint = if with_checkpoint {
        Some(Checkpoint {
            id: Uuid::new_v4(),
            session_id: id,
            parent_id: None,
            state: CheckpointState::default(),
            created_at: now,
            updated_at: now,
        })
    } else {
        None
    };
    Session {
        id,
        title: "t".to_string(),
        visibility: SessionVisibility::Private,
        status: SessionStatus::Active,
        cwd: None,
        created_at: now,
        updated_at: now,
        active_checkpoint: checkpoint,
    }
}

#[test]
fn output_mode_from_flag_maps_bool_to_variant() {
    assert_eq!(OutputMode::from_flag(true), OutputMode::Json);
    assert_eq!(OutputMode::from_flag(false), OutputMode::Human);
}

#[test]
fn render_error_json_contains_error_and_code_fields() {
    let rendered = render_error("boom", "not_found", OutputMode::Json);
    let v: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
    assert_eq!(v["error"].as_str().unwrap(), "boom");
    assert_eq!(v["code"].as_str().unwrap(), "not_found");
}

#[test]
fn render_error_human_is_plain_prefixed_message() {
    let rendered = render_error("boom", "not_found", OutputMode::Human);
    assert_eq!(rendered, "Error: boom");
}

#[tokio::test]
async fn render_list_human_truncates_long_titles_with_ellipsis() {
    let storage = in_memory_storage().await;
    let long = "x".repeat(120);
    let _ = storage
        .create_session(&CreateSessionRequest::new(
            long,
            vec![msg(Role::User, "hi")],
        ))
        .await
        .unwrap();

    let result = storage
        .list_sessions(&ListSessionsQuery::new())
        .await
        .unwrap();
    let rendered = render_list(&result.sessions, OutputMode::Human);
    assert!(
        rendered.contains('…'),
        "expected ellipsis in truncated title, got: {}",
        rendered
    );
    assert!(
        !rendered.contains(&"x".repeat(120)),
        "full long title should not appear untruncated"
    );
}

#[tokio::test]
async fn render_list_human_collapses_multiline_titles_to_single_row() {
    let storage = in_memory_storage().await;
    let _ = storage
        .create_session(&CreateSessionRequest::new(
            "deploy\n\tprod\r\ncluster",
            vec![msg(Role::User, "hi")],
        ))
        .await
        .unwrap();

    let result = storage
        .list_sessions(&ListSessionsQuery::new())
        .await
        .unwrap();
    let rendered = render_list(&result.sessions, OutputMode::Human);
    let lines: Vec<&str> = rendered.lines().collect();

    assert_eq!(
        lines.len(),
        4,
        "expected backend header, table header, separator, and one row"
    );
    assert!(
        rendered.contains("deploy prod cluster"),
        "expected collapsed title in one cell, got: {rendered}"
    );
    assert!(
        !rendered.contains("deploy\n") && !rendered.contains("\n\tprod"),
        "title cell must not contain stray line breaks: {rendered}"
    );
}

#[test]
fn render_show_human_with_no_active_checkpoint_notes_none() {
    let session = fake_session(Uuid::new_v4(), false);
    let rendered = render_show(&session, &[], None, OutputMode::Human);
    assert!(rendered.contains("Checkpoint:  (none)"));
    assert!(rendered.contains("(no messages)"));
}

#[test]
fn render_show_human_collapses_multiline_title_without_shifting_metadata() {
    let mut session = fake_session(Uuid::new_v4(), true);
    session.title = "deploy\n\tprod\r\ncluster".to_string();

    let rendered = render_show(&session, &[], None, OutputMode::Human);
    let lines: Vec<&str> = rendered.lines().collect();
    let title_line = lines
        .iter()
        .find(|line| line.starts_with("Title:"))
        .copied()
        .expect("title line");
    let status_index = lines
        .iter()
        .position(|line| line.starts_with("Status:"))
        .expect("status line");

    assert_eq!(title_line, "Title:       deploy prod cluster");
    assert_eq!(lines[status_index - 1], title_line);
    assert_eq!(lines[status_index], "Status:      ACTIVE");
    assert!(rendered.contains("Checkpoint:"));
}

#[test]
fn render_show_human_with_empty_messages_and_checkpoint_shows_no_messages_marker() {
    let session = fake_session(Uuid::new_v4(), true);
    let rendered = render_show(&session, &[], None, OutputMode::Human);
    assert!(rendered.contains("Messages (0):"));
    assert!(rendered.contains("(no messages)"));
}

#[test]
fn render_show_human_renders_tool_call_id_line_for_tool_messages() {
    let session = fake_session(Uuid::new_v4(), true);
    let tool_msg = ChatMessage {
        role: Role::Tool,
        content: Some(MessageContent::String("tool output".to_string())),
        tool_call_id: Some("tc_42".to_string()),
        ..Default::default()
    };
    let rendered = render_show(&session, &[tool_msg], None, OutputMode::Human);
    assert!(rendered.contains("[tool_call_id] tc_42"));
    assert!(rendered.contains("tool output"));
}

#[test]
fn render_show_human_renders_tool_call_line_for_assistant_messages() {
    let session = fake_session(Uuid::new_v4(), true);
    let asst = assistant_tool_call_msg("run_cmd", "tc_7");
    let rendered = render_show(&session, &[asst], None, OutputMode::Human);
    assert!(rendered.contains("[tool_call] run_cmd (tc_7)"));
}

// =============================================================================
// Terminal-escape sanitization (security)
// =============================================================================

#[test]
fn render_show_human_strips_terminal_escape_sequences() {
    use stakpak_shared::models::integrations::openai::{FunctionCall, ToolCall};

    let mut session = fake_session(Uuid::new_v4(), true);
    session.title = "evil\x1b[2Jtitle\x1b]52;c;YmFkYm95\x07".to_string();
    session.cwd = Some("/tmp/\x1b[31mred\x1b[0m".to_string());

    let tool_msg = ChatMessage {
        role: Role::Assistant,
        content: Some(MessageContent::String(
            "hi\x1b[2Jhidden\x1b]8;;https://evil.example/\x07click\x1b]8;;\x07".to_string(),
        )),
        tool_calls: Some(vec![ToolCall {
            id: "tc_\x1b[Kx".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "run_\x1b[31mcmd".to_string(),
                arguments: "{}".to_string(),
            },
            metadata: None,
        }]),
        tool_call_id: None,
        ..Default::default()
    };

    let rendered = render_show(&session, &[tool_msg], None, OutputMode::Human);
    assert!(
        !rendered.contains('\x1b'),
        "human output must not contain ESC (0x1B); got: {:?}",
        rendered
    );
    assert!(
        !rendered.contains('\x07'),
        "human output must not contain BEL (0x07); got: {:?}",
        rendered
    );
}

#[tokio::test]
async fn render_list_human_strips_terminal_escape_sequences_from_title() {
    let storage = in_memory_storage().await;
    let _ = storage
        .create_session(&CreateSessionRequest::new(
            "normal\x1b[2Jpart",
            vec![msg(Role::User, "hi")],
        ))
        .await
        .unwrap();

    let result = storage
        .list_sessions(&ListSessionsQuery::new())
        .await
        .unwrap();
    let rendered = render_list(&result.sessions, OutputMode::Human);
    assert!(
        !rendered.contains('\x1b'),
        "list human output must not contain ESC; got: {:?}",
        rendered
    );
}

#[test]
fn render_show_json_preserves_raw_content_including_escapes() {
    let session = fake_session(Uuid::new_v4(), true);
    let m = ChatMessage {
        role: Role::User,
        content: Some(MessageContent::String("hi\x1b[2Jhidden".to_string())),
        ..Default::default()
    };
    let rendered = render_show(&session, &[m], None, OutputMode::Json);
    let v: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(
        v["messages"][0]["content"].as_str().unwrap(),
        "hi\x1b[2Jhidden"
    );
}

// =============================================================================
// Profile-aware resume hint
// =============================================================================

#[test]
fn render_show_human_resume_hint_omits_default_profile() {
    let session = fake_session(Uuid::new_v4(), true);
    let rendered = render_show(&session, &[], Some("default"), OutputMode::Human);
    assert!(rendered.contains(&format!("Resume: stakpak --session {}", session.id)));
    assert!(!rendered.contains("--profile"));
}

#[test]
fn render_show_human_resume_hint_includes_non_default_profile() {
    let session = fake_session(Uuid::new_v4(), true);
    let rendered = render_show(&session, &[], Some("local"), OutputMode::Human);
    assert!(rendered.contains(&format!(
        "Resume: stakpak --profile local --session {}",
        session.id
    )));
}

#[test]
fn render_show_human_resume_hint_with_no_profile_omits_flag() {
    let session = fake_session(Uuid::new_v4(), true);
    let rendered = render_show(&session, &[], None, OutputMode::Human);
    assert!(rendered.contains(&format!("Resume: stakpak --session {}", session.id)));
    assert!(!rendered.contains("--profile"));
}

// =============================================================================
// CLI error-path argument parsing
// =============================================================================

#[test]
fn invalid_uuid_string_fails_to_parse() {
    assert!(Uuid::parse_str("not-a-uuid").is_err());
    assert!(Uuid::parse_str("").is_err());
    assert!(Uuid::parse_str("12345").is_err());
}

#[test]
fn invalid_role_string_is_rejected_by_role_filter() {
    assert!("robot".parse::<RoleFilter>().is_err());
    assert!("".parse::<RoleFilter>().is_err());
    let err = "robot".parse::<RoleFilter>().unwrap_err();
    assert!(err.contains("invalid role"));
    assert!(err.contains("user"));
}

// =============================================================================
// Branch coverage for `classify_storage_error`
// =============================================================================

#[test]
fn classify_storage_error_maps_every_variant() {
    assert_eq!(
        classify_storage_error(&StorageError::NotFound("x".into())),
        ("not_found", 1)
    );
    assert_eq!(
        classify_storage_error(&StorageError::InvalidRequest("x".into())),
        ("invalid_request", 2)
    );
    assert_eq!(
        classify_storage_error(&StorageError::Unauthorized("x".into())),
        ("unauthorized", 1)
    );
    assert_eq!(
        classify_storage_error(&StorageError::RateLimited("x".into())),
        ("rate_limited", 1)
    );
    assert_eq!(
        classify_storage_error(&StorageError::Connection("x".into())),
        ("connection_error", 1)
    );
    assert_eq!(
        classify_storage_error(&StorageError::Internal("x".into())),
        ("internal_error", 1)
    );
}
