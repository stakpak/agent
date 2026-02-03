#[cfg(test)]
mod tests {
    use crate::storage::*;
    use stakpak_shared::models::integrations::openai::{ChatMessage, MessageContent, Role};
    use uuid::Uuid;

    /// Helper: create an in-memory LocalStorage
    async fn create_test_storage() -> crate::local::storage::LocalStorage {
        crate::local::storage::LocalStorage::new(":memory:")
            .await
            .expect("Failed to create in-memory storage")
    }

    /// Helper: build a simple CreateSessionRequest
    fn session_request(title: &str, messages: Vec<ChatMessage>) -> CreateSessionRequest {
        CreateSessionRequest::new(title, messages)
    }

    /// Helper: build a user ChatMessage
    fn user_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String(text.to_string())),
            ..Default::default()
        }
    }

    /// Helper: build an assistant ChatMessage
    fn assistant_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String(text.to_string())),
            ..Default::default()
        }
    }

    // =========================================================================
    // Session CRUD
    // =========================================================================

    #[tokio::test]
    async fn test_create_session() {
        let storage = create_test_storage().await;
        let msgs = vec![user_msg("hello")];
        let result = storage
            .create_session(&session_request("My Session", msgs.clone()))
            .await
            .unwrap();

        assert!(!result.session_id.is_nil());
        assert!(!result.checkpoint.id.is_nil());
        assert_eq!(result.checkpoint.session_id, result.session_id);
        assert!(result.checkpoint.parent_id.is_none());
        assert_eq!(result.checkpoint.state.messages.len(), 1);
        assert_eq!(
            result.checkpoint.state.messages[0]
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "hello"
        );
    }

    #[tokio::test]
    async fn test_create_session_with_cwd() {
        let storage = create_test_storage().await;
        let req = CreateSessionRequest::new("cwd test", vec![user_msg("hi")]).with_cwd("/tmp/test");
        let result = storage.create_session(&req).await.unwrap();

        let session = storage.get_session(result.session_id).await.unwrap();
        assert_eq!(session.cwd, Some("/tmp/test".to_string()));
    }

    #[tokio::test]
    async fn test_create_session_with_visibility() {
        let storage = create_test_storage().await;
        let req = CreateSessionRequest::new("pub test", vec![user_msg("hi")])
            .with_visibility(SessionVisibility::Public);
        let result = storage.create_session(&req).await.unwrap();

        let session = storage.get_session(result.session_id).await.unwrap();
        assert_eq!(session.visibility, SessionVisibility::Public);
    }

    #[tokio::test]
    async fn test_get_session() {
        let storage = create_test_storage().await;
        let result = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        let session = storage.get_session(result.session_id).await.unwrap();
        assert_eq!(session.id, result.session_id);
        assert_eq!(session.title, "Test");
        assert_eq!(session.visibility, SessionVisibility::Private);
        assert_eq!(session.status, SessionStatus::Active);
        assert!(session.active_checkpoint.is_some());
        assert_eq!(session.active_checkpoint.unwrap().id, result.checkpoint.id);
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let storage = create_test_storage().await;
        let err = storage.get_session(Uuid::new_v4()).await;
        assert!(err.is_err());
        assert!(matches!(err.unwrap_err(), StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_update_session_title() {
        let storage = create_test_storage().await;
        let result = storage
            .create_session(&session_request("Old Title", vec![user_msg("hi")]))
            .await
            .unwrap();

        let updated = storage
            .update_session(
                result.session_id,
                &UpdateSessionRequest::new().with_title("New Title"),
            )
            .await
            .unwrap();

        assert_eq!(updated.title, "New Title");
    }

    #[tokio::test]
    async fn test_update_session_visibility() {
        let storage = create_test_storage().await;
        let result = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        let updated = storage
            .update_session(
                result.session_id,
                &UpdateSessionRequest::new().with_visibility(SessionVisibility::Public),
            )
            .await
            .unwrap();

        assert_eq!(updated.visibility, SessionVisibility::Public);
    }

    #[tokio::test]
    async fn test_delete_session() {
        let storage = create_test_storage().await;
        let result = storage
            .create_session(&session_request("To Delete", vec![user_msg("hi")]))
            .await
            .unwrap();

        storage.delete_session(result.session_id).await.unwrap();

        let session = storage.get_session(result.session_id).await.unwrap();
        assert_eq!(session.status, SessionStatus::Deleted);
    }

    // =========================================================================
    // List Sessions
    // =========================================================================

    #[tokio::test]
    async fn test_list_sessions_empty() {
        let storage = create_test_storage().await;
        let result = storage
            .list_sessions(&ListSessionsQuery::new())
            .await
            .unwrap();
        assert!(result.sessions.is_empty());
    }

    #[tokio::test]
    async fn test_list_sessions_returns_all() {
        let storage = create_test_storage().await;
        storage
            .create_session(&session_request("A", vec![user_msg("a")]))
            .await
            .unwrap();
        storage
            .create_session(&session_request("B", vec![user_msg("b")]))
            .await
            .unwrap();
        storage
            .create_session(&session_request("C", vec![user_msg("c")]))
            .await
            .unwrap();

        let result = storage
            .list_sessions(&ListSessionsQuery::new())
            .await
            .unwrap();
        assert_eq!(result.sessions.len(), 3);
    }

    #[tokio::test]
    async fn test_list_sessions_with_limit() {
        let storage = create_test_storage().await;
        for i in 0..5 {
            storage
                .create_session(&session_request(&format!("S{}", i), vec![user_msg("hi")]))
                .await
                .unwrap();
        }

        let result = storage
            .list_sessions(&ListSessionsQuery::new().with_limit(2))
            .await
            .unwrap();
        assert_eq!(result.sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_with_offset() {
        let storage = create_test_storage().await;
        for i in 0..5 {
            storage
                .create_session(&session_request(&format!("S{}", i), vec![user_msg("hi")]))
                .await
                .unwrap();
        }

        let result = storage
            .list_sessions(&ListSessionsQuery::new().with_offset(3))
            .await
            .unwrap();
        assert_eq!(result.sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_with_search() {
        let storage = create_test_storage().await;
        storage
            .create_session(&session_request("Rust project", vec![user_msg("hi")]))
            .await
            .unwrap();
        storage
            .create_session(&session_request("Python script", vec![user_msg("hi")]))
            .await
            .unwrap();
        storage
            .create_session(&session_request("Rust CLI", vec![user_msg("hi")]))
            .await
            .unwrap();

        let result = storage
            .list_sessions(&ListSessionsQuery::new().with_search("Rust"))
            .await
            .unwrap();
        assert_eq!(result.sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_summary_has_checkpoint_info() {
        let storage = create_test_storage().await;
        let created = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        let result = storage
            .list_sessions(&ListSessionsQuery::new())
            .await
            .unwrap();

        assert_eq!(result.sessions.len(), 1);
        let summary = &result.sessions[0];
        assert_eq!(summary.id, created.session_id);
        assert_eq!(summary.title, "Test");
        assert!(summary.active_checkpoint_id.is_some());
        assert_eq!(summary.active_checkpoint_id.unwrap(), created.checkpoint.id);
        assert!(summary.message_count > 0);
    }

    // =========================================================================
    // Checkpoint CRUD
    // =========================================================================

    #[tokio::test]
    async fn test_create_checkpoint() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        let msgs = vec![
            user_msg("hi"),
            assistant_msg("hello"),
            user_msg("how are you?"),
        ];
        let req = CreateCheckpointRequest::new(msgs.clone()).with_parent(session.checkpoint.id);

        let checkpoint = storage
            .create_checkpoint(session.session_id, &req)
            .await
            .unwrap();

        assert_eq!(checkpoint.session_id, session.session_id);
        assert_eq!(checkpoint.parent_id, Some(session.checkpoint.id));
        assert_eq!(checkpoint.state.messages.len(), 3);
    }

    #[tokio::test]
    async fn test_create_checkpoint_without_parent() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        let req = CreateCheckpointRequest::new(vec![user_msg("branch")]);
        let checkpoint = storage
            .create_checkpoint(session.session_id, &req)
            .await
            .unwrap();

        assert!(checkpoint.parent_id.is_none());
    }

    #[tokio::test]
    async fn test_get_checkpoint() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("original")]))
            .await
            .unwrap();

        let fetched = storage.get_checkpoint(session.checkpoint.id).await.unwrap();
        assert_eq!(fetched.id, session.checkpoint.id);
        assert_eq!(fetched.session_id, session.session_id);
        assert_eq!(fetched.state.messages.len(), 1);
    }

    #[tokio::test]
    async fn test_get_checkpoint_not_found() {
        let storage = create_test_storage().await;
        let err = storage.get_checkpoint(Uuid::new_v4()).await;
        assert!(err.is_err());
        assert!(matches!(err.unwrap_err(), StorageError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_list_checkpoints() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("first")]))
            .await
            .unwrap();

        // Create two more checkpoints
        storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![user_msg("first"), assistant_msg("second")])
                    .with_parent(session.checkpoint.id),
            )
            .await
            .unwrap();
        storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![
                    user_msg("first"),
                    assistant_msg("second"),
                    user_msg("third"),
                ]),
            )
            .await
            .unwrap();

        let result = storage
            .list_checkpoints(session.session_id, &ListCheckpointsQuery::new())
            .await
            .unwrap();

        // 1 initial + 2 created = 3
        assert_eq!(result.checkpoints.len(), 3);
        // Sorted by created_at ASC
        assert_eq!(result.checkpoints[0].id, session.checkpoint.id);
    }

    #[tokio::test]
    async fn test_list_checkpoints_with_limit() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("first")]))
            .await
            .unwrap();

        for _ in 0..4 {
            storage
                .create_checkpoint(
                    session.session_id,
                    &CreateCheckpointRequest::new(vec![user_msg("msg")]),
                )
                .await
                .unwrap();
        }

        let result = storage
            .list_checkpoints(
                session.session_id,
                &ListCheckpointsQuery::new().with_limit(2),
            )
            .await
            .unwrap();
        assert_eq!(result.checkpoints.len(), 2);
    }

    // =========================================================================
    // Active checkpoint / convenience methods
    // =========================================================================

    #[tokio::test]
    async fn test_get_active_checkpoint_returns_latest() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("first")]))
            .await
            .unwrap();

        let second = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![user_msg("first"), assistant_msg("second")])
                    .with_parent(session.checkpoint.id),
            )
            .await
            .unwrap();

        let third = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![
                    user_msg("first"),
                    assistant_msg("second"),
                    user_msg("third"),
                ])
                .with_parent(second.id),
            )
            .await
            .unwrap();

        let active = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();

        assert_eq!(active.id, third.id);
        assert_eq!(active.state.messages.len(), 3);
    }

    #[tokio::test]
    async fn test_get_active_checkpoint_on_new_session() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("hello")]))
            .await
            .unwrap();

        let active = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();

        assert_eq!(active.id, session.checkpoint.id);
    }

    #[tokio::test]
    async fn test_get_session_stats_returns_default() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        let stats = storage.get_session_stats(session.session_id).await.unwrap();

        // Local storage returns defaults
        assert_eq!(stats.total_sessions, 0);
        assert_eq!(stats.total_tool_calls, 0);
    }

    // =========================================================================
    // Checkpoint state with empty / null messages
    // =========================================================================

    #[tokio::test]
    async fn test_checkpoint_with_empty_messages() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![]))
            .await
            .unwrap();

        let fetched = storage.get_checkpoint(session.checkpoint.id).await.unwrap();
        assert!(fetched.state.messages.is_empty());
    }

    #[tokio::test]
    async fn test_checkpoint_preserves_message_roles() {
        let storage = create_test_storage().await;
        let msgs = vec![
            user_msg("question"),
            assistant_msg("answer"),
            ChatMessage {
                role: Role::Tool,
                content: Some(MessageContent::String("tool result".to_string())),
                tool_call_id: Some("tc_123".to_string()),
                ..Default::default()
            },
        ];
        let session = storage
            .create_session(&session_request("Test", msgs))
            .await
            .unwrap();

        let fetched = storage.get_checkpoint(session.checkpoint.id).await.unwrap();
        assert_eq!(fetched.state.messages.len(), 3);
        assert_eq!(fetched.state.messages[0].role, Role::User);
        assert_eq!(fetched.state.messages[1].role, Role::Assistant);
        assert_eq!(fetched.state.messages[2].role, Role::Tool);
        assert_eq!(
            fetched.state.messages[2].tool_call_id,
            Some("tc_123".to_string())
        );
    }

    // =========================================================================
    // Checkpoint chain (parent links)
    // =========================================================================

    #[tokio::test]
    async fn test_checkpoint_chain_parent_links() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("start")]))
            .await
            .unwrap();

        let cp1_id = session.checkpoint.id;

        let cp2 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![user_msg("start"), assistant_msg("reply")])
                    .with_parent(cp1_id),
            )
            .await
            .unwrap();

        let cp3 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![
                    user_msg("start"),
                    assistant_msg("reply"),
                    user_msg("followup"),
                ])
                .with_parent(cp2.id),
            )
            .await
            .unwrap();

        // Verify the chain
        let fetched1 = storage.get_checkpoint(cp1_id).await.unwrap();
        assert!(fetched1.parent_id.is_none());

        let fetched2 = storage.get_checkpoint(cp2.id).await.unwrap();
        assert_eq!(fetched2.parent_id, Some(cp1_id));

        let fetched3 = storage.get_checkpoint(cp3.id).await.unwrap();
        assert_eq!(fetched3.parent_id, Some(cp2.id));
    }

    // =========================================================================
    // Session updates bump updated_at on checkpoint creation
    // =========================================================================

    #[tokio::test]
    async fn test_create_checkpoint_updates_session_timestamp() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        let before = storage.get_session(session.session_id).await.unwrap();

        // Small delay to ensure different timestamp
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![user_msg("hi"), assistant_msg("there")]),
            )
            .await
            .unwrap();

        let after = storage.get_session(session.session_id).await.unwrap();
        assert!(after.updated_at >= before.updated_at);
    }

    // =========================================================================
    // Multiple sessions isolation
    // =========================================================================

    #[tokio::test]
    async fn test_sessions_are_isolated() {
        let storage = create_test_storage().await;

        let s1 = storage
            .create_session(&session_request("Session 1", vec![user_msg("s1")]))
            .await
            .unwrap();
        let s2 = storage
            .create_session(&session_request("Session 2", vec![user_msg("s2")]))
            .await
            .unwrap();

        // Add checkpoint to session 1 only
        storage
            .create_checkpoint(
                s1.session_id,
                &CreateCheckpointRequest::new(vec![user_msg("s1"), assistant_msg("s1 reply")]),
            )
            .await
            .unwrap();

        let s1_checkpoints = storage
            .list_checkpoints(s1.session_id, &ListCheckpointsQuery::new())
            .await
            .unwrap();
        let s2_checkpoints = storage
            .list_checkpoints(s2.session_id, &ListCheckpointsQuery::new())
            .await
            .unwrap();

        assert_eq!(s1_checkpoints.checkpoints.len(), 2); // initial + 1
        assert_eq!(s2_checkpoints.checkpoints.len(), 1); // initial only
    }

    // =========================================================================
    // Delete doesn't remove, only marks
    // =========================================================================

    #[tokio::test]
    async fn test_delete_session_still_accessible() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("To Delete", vec![user_msg("hi")]))
            .await
            .unwrap();

        storage.delete_session(session.session_id).await.unwrap();

        // Still accessible via get
        let fetched = storage.get_session(session.session_id).await.unwrap();
        assert_eq!(fetched.status, SessionStatus::Deleted);

        // Checkpoint still accessible
        let cp = storage.get_checkpoint(session.checkpoint.id).await.unwrap();
        assert_eq!(cp.session_id, session.session_id);
    }

    // =========================================================================
    // Old schema compatibility (null state in checkpoints)
    // =========================================================================

    #[tokio::test]
    async fn test_null_state_checkpoint_returns_empty_messages() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        // Manually insert a checkpoint with NULL state (simulating old schema data)
        let checkpoint_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        storage
            .connection()
            .lock()
            .await
            .execute(
                "INSERT INTO checkpoints (id, session_id, status, execution_depth, parent_id, state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    checkpoint_id.to_string(),
                    session.session_id.to_string(),
                    "COMPLETE",
                    0i64,
                    None::<String>,
                    None::<String>,  // NULL state
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ),
            )
            .await
            .unwrap();

        let fetched = storage.get_checkpoint(checkpoint_id).await.unwrap();
        assert!(fetched.state.messages.is_empty());
    }

    #[tokio::test]
    async fn test_malformed_state_json_returns_empty_messages() {
        let storage = create_test_storage().await;
        let session = storage
            .create_session(&session_request("Test", vec![user_msg("hi")]))
            .await
            .unwrap();

        // Insert checkpoint with malformed JSON state
        let checkpoint_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        storage
            .connection()
            .lock()
            .await
            .execute(
                "INSERT INTO checkpoints (id, session_id, state, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
                (
                    checkpoint_id.to_string(),
                    session.session_id.to_string(),
                    "not valid json",
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ),
            )
            .await
            .unwrap();

        let fetched = storage.get_checkpoint(checkpoint_id).await.unwrap();
        assert!(fetched.state.messages.is_empty());
    }

    // =========================================================================
    // StorageError variants
    // =========================================================================

    #[test]
    fn test_storage_error_display() {
        let err = StorageError::NotFound("missing".to_string());
        assert_eq!(format!("{}", err), "Not found: missing");

        let err = StorageError::Internal("broken".to_string());
        assert_eq!(format!("{}", err), "Internal error: broken");

        let err = StorageError::Connection("timeout".to_string());
        assert_eq!(format!("{}", err), "Connection error: timeout");
    }

    #[test]
    fn test_storage_error_from_string() {
        let err: StorageError = "something failed".into();
        assert!(matches!(err, StorageError::Internal(_)));
    }

    // =========================================================================
    // Request builder patterns
    // =========================================================================

    #[test]
    fn test_create_session_request_builder() {
        let req = CreateSessionRequest::new("title", vec![user_msg("hi")])
            .with_cwd("/home")
            .with_visibility(SessionVisibility::Public);

        assert_eq!(req.title, "title");
        assert_eq!(req.cwd, Some("/home".to_string()));
        assert_eq!(req.visibility, SessionVisibility::Public);
        assert_eq!(req.initial_state.messages.len(), 1);
    }

    #[test]
    fn test_update_session_request_builder() {
        let req = UpdateSessionRequest::new()
            .with_title("new title")
            .with_visibility(SessionVisibility::Public);

        assert_eq!(req.title, Some("new title".to_string()));
        assert_eq!(req.visibility, Some(SessionVisibility::Public));
    }

    #[test]
    fn test_create_checkpoint_request_builder() {
        let parent = Uuid::new_v4();
        let req = CreateCheckpointRequest::new(vec![user_msg("hi")]).with_parent(parent);

        assert_eq!(req.parent_id, Some(parent));
        assert_eq!(req.state.messages.len(), 1);
    }

    #[test]
    fn test_list_sessions_query_builder() {
        let q = ListSessionsQuery::new()
            .with_limit(10)
            .with_offset(5)
            .with_search("test");

        assert_eq!(q.limit, Some(10));
        assert_eq!(q.offset, Some(5));
        assert_eq!(q.search, Some("test".to_string()));
    }

    #[test]
    fn test_list_checkpoints_query_builder() {
        let q = ListCheckpointsQuery::new().with_limit(5).with_state();

        assert_eq!(q.limit, Some(5));
        assert_eq!(q.include_state, Some(true));
    }

    // =========================================================================
    // Visibility / Status display
    // =========================================================================

    #[test]
    fn test_visibility_display() {
        assert_eq!(SessionVisibility::Private.to_string(), "PRIVATE");
        assert_eq!(SessionVisibility::Public.to_string(), "PUBLIC");
    }

    #[test]
    fn test_status_display() {
        assert_eq!(SessionStatus::Active.to_string(), "ACTIVE");
        assert_eq!(SessionStatus::Deleted.to_string(), "DELETED");
    }

    #[test]
    fn test_visibility_default() {
        let v: SessionVisibility = Default::default();
        assert_eq!(v, SessionVisibility::Private);
    }

    #[test]
    fn test_status_default() {
        let s: SessionStatus = Default::default();
        assert_eq!(s, SessionStatus::Active);
    }

    // =========================================================================
    // Checkpoint state serialization round-trip
    // =========================================================================

    #[test]
    fn test_checkpoint_state_default_is_empty() {
        let state = CheckpointState::default();
        assert!(state.messages.is_empty());
    }

    #[test]
    fn test_checkpoint_state_serde_roundtrip() {
        let state = CheckpointState {
            messages: vec![user_msg("hello"), assistant_msg("world")],
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: CheckpointState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.messages.len(), 2);
    }

    #[test]
    fn test_checkpoint_state_deserialize_empty_json() {
        let state: CheckpointState = serde_json::from_str("{}").unwrap();
        assert!(state.messages.is_empty());
    }

    // =========================================================================
    // SessionStats defaults
    // =========================================================================

    #[test]
    fn test_session_stats_default() {
        let stats = SessionStats::default();
        assert_eq!(stats.total_sessions, 0);
        assert_eq!(stats.total_tool_calls, 0);
        assert_eq!(stats.successful_tool_calls, 0);
        assert_eq!(stats.failed_tool_calls, 0);
        assert_eq!(stats.aborted_tool_calls, 0);
        assert_eq!(stats.sessions_with_activity, 0);
        assert!(stats.total_time_saved_seconds.is_none());
        assert!(stats.tools_usage.is_empty());
    }

    // =========================================================================
    // Migration tests
    // =========================================================================

    #[tokio::test]
    async fn test_migrations_applied() {
        let storage = create_test_storage().await;
        let conn = storage.connection().lock().await;

        let version = crate::local::migrations::current_version(&conn)
            .await
            .unwrap();
        assert_eq!(version, 2, "All migrations should be applied");

        let status = crate::local::migrations::status(&conn).await.unwrap();
        assert_eq!(status.applied, vec![1, 2]);
        assert!(status.pending.is_empty());
    }

    #[tokio::test]
    async fn test_migration_rollback() {
        let storage = create_test_storage().await;
        let conn = storage.connection().lock().await;

        // Should be at version 2
        let version = crate::local::migrations::current_version(&conn)
            .await
            .unwrap();
        assert_eq!(version, 2);

        // Rollback to version 1
        let rolled_back = crate::local::migrations::rollback_last(&conn)
            .await
            .unwrap();
        assert_eq!(rolled_back, Some(2));

        let version = crate::local::migrations::current_version(&conn)
            .await
            .unwrap();
        assert_eq!(version, 1);

        // Rollback to version 0
        let rolled_back = crate::local::migrations::rollback_last(&conn)
            .await
            .unwrap();
        assert_eq!(rolled_back, Some(1));

        let version = crate::local::migrations::current_version(&conn)
            .await
            .unwrap();
        assert_eq!(version, 0);

        // Re-apply all
        let applied = crate::local::migrations::apply_all(&conn).await.unwrap();
        assert_eq!(applied, vec![1, 2]);
    }

    #[tokio::test]
    async fn test_migration_rollback_to_version() {
        let storage = create_test_storage().await;
        let conn = storage.connection().lock().await;

        // Rollback to version 1 (keeps 1, removes 2)
        let rolled_back = crate::local::migrations::rollback_to(&conn, 1)
            .await
            .unwrap();
        assert_eq!(rolled_back, vec![2]);

        let version = crate::local::migrations::current_version(&conn)
            .await
            .unwrap();
        assert_eq!(version, 1);
    }
}
