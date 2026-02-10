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
            metadata: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: CheckpointState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.messages.len(), 2);
        assert!(deserialized.metadata.is_none());
    }

    #[test]
    fn test_checkpoint_state_deserialize_empty_json() {
        let state: CheckpointState = serde_json::from_str("{}").unwrap();
        assert!(state.messages.is_empty());
        assert!(state.metadata.is_none());
    }

    #[test]
    fn test_checkpoint_state_with_metadata_roundtrip() {
        let state = CheckpointState {
            messages: vec![user_msg("hello")],
            metadata: Some(serde_json::json!({"trimmed_up_to_index": 5})),
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: CheckpointState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.messages.len(), 1);
        assert!(deserialized.metadata.is_some());
        assert_eq!(
            deserialized.metadata.unwrap()["trimmed_up_to_index"],
            serde_json::json!(5)
        );
    }

    #[test]
    fn test_checkpoint_state_old_format_without_metadata_deserializes() {
        // Simulate old checkpoint format without metadata field
        let old_json = r#"{"messages":[{"role":"user","content":"test"}]}"#;
        let state: CheckpointState = serde_json::from_str(old_json).unwrap();
        assert_eq!(state.messages.len(), 1);
        assert!(
            state.metadata.is_none(),
            "Old checkpoints should have None metadata"
        );
    }

    #[test]
    fn test_create_checkpoint_request_with_metadata() {
        let request = CreateCheckpointRequest::new(vec![user_msg("hello")])
            .with_metadata(serde_json::json!({"trimmed_up_to_index": 10}));

        assert_eq!(request.state.messages.len(), 1);
        assert!(request.state.metadata.is_some());
        assert_eq!(
            request.state.metadata.unwrap()["trimmed_up_to_index"],
            serde_json::json!(10)
        );
    }

    #[test]
    fn test_create_checkpoint_request_without_metadata() {
        let request = CreateCheckpointRequest::new(vec![user_msg("hello")]);

        assert_eq!(request.state.messages.len(), 1);
        assert!(request.state.metadata.is_none());
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

    // =========================================================================
    // Async mode: trimming threshold & metadata persistence integration tests
    // =========================================================================

    /// Helper: build a large user message to inflate token count
    fn large_user_msg(turn: usize) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String(format!(
                "Turn {}: {}",
                turn,
                "x".repeat(200)
            ))),
            ..Default::default()
        }
    }

    /// Helper: build a large assistant message to inflate token count
    fn large_assistant_msg(turn: usize) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String(format!(
                "Response {}: {}",
                turn,
                "y".repeat(200)
            ))),
            ..Default::default()
        }
    }

    /// Simulates the async mode flow: trimming should NOT trigger when
    /// estimated tokens are under the context_budget_threshold.
    #[tokio::test]
    async fn test_async_no_trimming_under_threshold() {
        use crate::local::context_managers::task_board_context_manager::{
            TaskBoardContextManager, TaskBoardContextManagerOptions,
        };

        let storage = create_test_storage().await;

        // Create a session with a small conversation (2 turns)
        let initial_msgs = vec![user_msg("hello"), assistant_msg("hi there")];
        let session = storage
            .create_session(&session_request("Async Trim Test", initial_msgs))
            .await
            .unwrap();

        // Simulate a few more turns (still small)
        let messages = vec![
            user_msg("hello"),
            assistant_msg("hi there"),
            user_msg("how are you?"),
            assistant_msg("I'm doing well, thanks!"),
        ];

        // Save checkpoint with these messages (no metadata yet)
        let cp = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(messages.clone()).with_parent(session.checkpoint.id),
            )
            .await
            .unwrap();

        // Simulate async mode resume: load checkpoint
        let loaded = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();
        assert_eq!(loaded.id, cp.id);
        assert!(
            loaded.state.metadata.is_none(),
            "Fresh checkpoint should have no metadata"
        );

        // Run context manager with a LARGE context window (no trimming expected)
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 50,
            context_budget_threshold: 0.8,
        });

        let (result, metadata) =
            cm.reduce_context_with_budget(messages.clone(), 200_000, loaded.state.metadata, None);

        // No trimming should have occurred
        assert!(
            metadata.is_none(),
            "Should not produce metadata when under threshold"
        );
        // All content should be preserved
        for msg in &result {
            match &msg.content {
                stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                    assert_ne!(s, "[trimmed]", "No messages should be trimmed");
                }
                _ => {}
            }
        }

        // Save checkpoint without metadata (simulating async mode saving)
        let cp2 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(messages).with_parent(cp.id),
            )
            .await
            .unwrap();

        // Verify: checkpoint has no metadata
        let loaded2 = storage.get_checkpoint(cp2.id).await.unwrap();
        assert!(
            loaded2.state.metadata.is_none(),
            "Checkpoint should have no metadata when trimming didn't trigger"
        );
    }

    /// Simulates the async mode flow: trimming SHOULD trigger when
    /// estimated tokens exceed the context_budget_threshold.
    #[tokio::test]
    async fn test_async_trimming_triggers_at_threshold() {
        use crate::local::context_managers::task_board_context_manager::{
            TaskBoardContextManager, TaskBoardContextManagerOptions,
        };

        let storage = create_test_storage().await;

        // Build a large conversation (10 turns of large messages)
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(large_user_msg(i));
            messages.push(large_assistant_msg(i));
        }

        // Create session with initial messages
        let session = storage
            .create_session(&session_request(
                "Async Trim Trigger",
                vec![messages[0].clone()],
            ))
            .await
            .unwrap();

        // Save checkpoint with all messages
        let cp = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(messages.clone()).with_parent(session.checkpoint.id),
            )
            .await
            .unwrap();

        // Simulate async mode resume
        let loaded = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();
        assert_eq!(loaded.id, cp.id);

        // Run context manager with a SMALL context window (trimming expected).
        // Budget is the hard constraint — keep_last_n is best-effort and will
        // be overridden when the last N messages themselves exceed the budget.
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 4,
            context_budget_threshold: 0.8,
        });

        let (result, metadata) =
            cm.reduce_context_with_budget(messages.clone(), 200, loaded.state.metadata, None);

        // Trimming should have occurred
        assert!(
            metadata.is_some(),
            "Should produce metadata when trimming triggers"
        );
        let meta = metadata.as_ref().unwrap();
        let trimmed_idx = meta["trimmed_up_to_message_index"].as_u64().unwrap() as usize;
        assert!(trimmed_idx > 0, "Should have trimmed some messages");

        // Verify early assistant messages are trimmed (user messages preserved)
        // result[0] is user (NOT trimmed), result[1] is assistant (trimmed)
        match &result[0].content {
            stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                assert_ne!(s, "[trimmed]", "First user message should NOT be trimmed");
            }
            _ => panic!("Expected string content"),
        }
        match &result[1].content {
            stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                assert_eq!(s, "[trimmed]", "First assistant message should be trimmed");
            }
            _ => panic!("Expected string content"),
        }

        // Budget is the hard constraint: the trimmer trims all assistant/tool
        // messages it can. With a 200-token window, user messages alone (~700
        // tokens) exceed the threshold, so the trimmer can't get fully under
        // budget — but it MUST trim every assistant/tool message it can reach.
        // Verify all assistant messages before the trim boundary are trimmed.
        for (i, msg) in result.iter().enumerate() {
            if i < trimmed_idx && msg.role == "assistant" {
                match &msg.content {
                    stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                        assert_eq!(
                            s, "[trimmed]",
                            "Assistant at {} before trim boundary should be trimmed",
                            i
                        );
                    }
                    _ => {}
                }
            }
        }

        // User messages are never trimmed regardless of budget pressure
        for msg in &result {
            if msg.role == "user" {
                match &msg.content {
                    stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                        assert_ne!(s, "[trimmed]", "User messages should never be trimmed");
                    }
                    _ => {}
                }
            }
        }

        // Save checkpoint WITH metadata (this is what async mode should do)
        let cp2 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(messages)
                    .with_parent(cp.id)
                    .with_metadata(meta.clone()),
            )
            .await
            .unwrap();

        // Verify: checkpoint has metadata persisted
        let loaded2 = storage.get_checkpoint(cp2.id).await.unwrap();
        assert!(
            loaded2.state.metadata.is_some(),
            "Checkpoint should have metadata"
        );
        assert_eq!(
            loaded2.state.metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
                .as_u64()
                .unwrap() as usize,
            trimmed_idx,
            "Persisted trimmed_up_to_message_index should match"
        );
    }

    /// Full async session resume integration test:
    /// 1. Create session, build up conversation until trimming triggers
    /// 2. Save checkpoint with trimming metadata
    /// 3. Resume session (load checkpoint by session_id)
    /// 4. Add more messages, run trimming again with loaded metadata
    /// 5. Verify trimming state is continuous (index advances, prefix stays trimmed)
    #[tokio::test]
    async fn test_async_trimming_state_persists_across_session_resume() {
        use crate::local::context_managers::task_board_context_manager::{
            TaskBoardContextManager, TaskBoardContextManagerOptions,
        };

        let storage = create_test_storage().await;
        let context_window = 200u64; // Small window to force trimming

        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 4, // Keep last 4 messages untrimmed
            context_budget_threshold: 0.8,
        });

        // === Phase 1: Initial session with 8 turns ===
        let mut messages = Vec::new();
        for i in 0..8 {
            messages.push(large_user_msg(i));
            messages.push(large_assistant_msg(i));
        }

        let session = storage
            .create_session(&session_request("Resume Test", vec![messages[0].clone()]))
            .await
            .unwrap();

        // Run trimming (simulating first chat_completion call)
        let (result1, metadata1) =
            cm.reduce_context_with_budget(messages.clone(), context_window, None, None);

        assert!(metadata1.is_some(), "Phase 1: trimming should trigger");
        let trimmed_idx_1 = metadata1.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(trimmed_idx_1 > 0, "Phase 1: should have trimmed messages");

        // Verify early non-user messages are trimmed, user messages preserved.
        // Budget is the hard constraint — keep_last_n is best-effort, so the
        // last 4 messages may also be trimmed if budget demands it.
        for (i, msg) in result1.iter().enumerate() {
            let is_trimmed = match &msg.content {
                stakpak_shared::models::llm::LLMMessageContent::String(s) => s == "[trimmed]",
                stakpak_shared::models::llm::LLMMessageContent::List(parts) => {
                    parts.iter().all(|p| match p {
                        stakpak_shared::models::llm::LLMMessageTypedContent::Text { text } => {
                            text == "[trimmed]"
                        }
                        stakpak_shared::models::llm::LLMMessageTypedContent::ToolResult {
                            content,
                            ..
                        } => content == "[trimmed]",
                        _ => true,
                    })
                }
            };
            if i < trimmed_idx_1 && msg.role != "user" {
                assert!(
                    is_trimmed,
                    "Phase 1: non-user message {} should be trimmed",
                    i
                );
            }
            if msg.role == "user" {
                assert!(
                    !is_trimmed,
                    "Phase 1: user message {} should NOT be trimmed",
                    i
                );
            }
        }
        // Budget hard constraint: trimmer trims all assistant/tool messages it
        // can. With a 200-token window, user messages alone exceed the threshold,
        // so the trimmer can't get fully under budget — but all trimmable
        // messages before the boundary must be trimmed.
        for (i, msg) in result1.iter().enumerate() {
            if i < trimmed_idx_1 && msg.role == "assistant" {
                let is_trimmed = match &msg.content {
                    stakpak_shared::models::llm::LLMMessageContent::String(s) => s == "[trimmed]",
                    _ => true,
                };
                assert!(
                    is_trimmed,
                    "Phase 1: assistant at {} before boundary should be trimmed",
                    i
                );
            }
        }

        // Save checkpoint with metadata (simulating async mode save_checkpoint)
        let cp1 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(messages.clone())
                    .with_parent(session.checkpoint.id)
                    .with_metadata(metadata1.as_ref().unwrap().clone()),
            )
            .await
            .unwrap();

        // === Phase 2: Resume session (simulate `stakpak -s <session_id>`) ===
        let resumed_checkpoint = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();

        assert_eq!(
            resumed_checkpoint.id, cp1.id,
            "Should resume from latest checkpoint"
        );
        assert!(
            resumed_checkpoint.state.metadata.is_some(),
            "Resumed checkpoint should have metadata"
        );
        let resumed_metadata = resumed_checkpoint.state.metadata.clone();
        let resumed_trimmed_idx = resumed_metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert_eq!(
            resumed_trimmed_idx, trimmed_idx_1,
            "Resumed metadata should match saved metadata"
        );

        // Load messages from checkpoint (this is what async mode does)
        let mut resumed_messages = resumed_checkpoint.state.messages;

        // Add 4 more turns (simulating continued conversation after resume)
        for i in 8..12 {
            resumed_messages.push(large_user_msg(i));
            resumed_messages.push(large_assistant_msg(i));
        }

        // Run trimming again with loaded metadata
        let (result2, metadata2) = cm.reduce_context_with_budget(
            resumed_messages.clone(),
            context_window,
            resumed_metadata,
            None,
        );

        assert!(metadata2.is_some(), "Phase 2: trimming should trigger");
        let trimmed_idx_2 = metadata2.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;

        // Trimmed index should advance (more messages to trim)
        assert!(
            trimmed_idx_2 >= trimmed_idx_1,
            "Trimmed index should not decrease: {} < {}",
            trimmed_idx_2,
            trimmed_idx_1
        );

        // Verify: budget overrides keep_last_n — all trimmable messages before
        // the boundary are trimmed, user messages are always preserved.
        for (i, msg) in result2.iter().enumerate() {
            if msg.role == "user" {
                match &msg.content {
                    stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                        assert_ne!(
                            s, "[trimmed]",
                            "Phase 2: user message {} should NOT be trimmed",
                            i
                        );
                    }
                    _ => {}
                }
            } else if i < trimmed_idx_2 {
                match &msg.content {
                    stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                        assert_eq!(
                            s, "[trimmed]",
                            "Phase 2: non-user message {} before boundary should be trimmed",
                            i
                        );
                    }
                    _ => {}
                }
            }
        }

        // Save second checkpoint with updated metadata
        let cp2 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(resumed_messages)
                    .with_parent(cp1.id)
                    .with_metadata(metadata2.as_ref().unwrap().clone()),
            )
            .await
            .unwrap();

        // === Phase 3: Resume again and verify metadata chain ===
        let final_checkpoint = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();

        assert_eq!(final_checkpoint.id, cp2.id);
        assert!(final_checkpoint.state.metadata.is_some());
        let final_trimmed_idx =
            final_checkpoint.state.metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
                .as_u64()
                .unwrap() as usize;
        assert_eq!(
            final_trimmed_idx, trimmed_idx_2,
            "Final checkpoint metadata should match phase 2 metadata"
        );
    }

    /// Verify that when resuming a session with NO prior trimming metadata,
    /// trimming still works correctly (backward compatibility).
    #[tokio::test]
    async fn test_async_resume_without_metadata_backward_compat() {
        use crate::local::context_managers::task_board_context_manager::{
            TaskBoardContextManager, TaskBoardContextManagerOptions,
        };

        let storage = create_test_storage().await;

        // Create session with large conversation but NO metadata (old checkpoint format)
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(large_user_msg(i));
            messages.push(large_assistant_msg(i));
        }

        let session = storage
            .create_session(&session_request(
                "Backward Compat",
                vec![messages[0].clone()],
            ))
            .await
            .unwrap();

        // Save checkpoint WITHOUT metadata (simulating old checkpoint format)
        let _cp = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(messages.clone()).with_parent(session.checkpoint.id),
            )
            .await
            .unwrap();

        // Resume: metadata should be None
        let loaded = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();
        assert!(
            loaded.state.metadata.is_none(),
            "Old checkpoint has no metadata"
        );

        // Run trimming with None metadata and small window
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 4, // Keep last 4 messages untrimmed
            context_budget_threshold: 0.8,
        });

        let (result, metadata) = cm.reduce_context_with_budget(messages, 200, None, None);

        // Should still trigger trimming correctly
        assert!(
            metadata.is_some(),
            "Trimming should work without prior metadata"
        );
        let trimmed_idx = metadata.as_ref().unwrap()["trimmed_up_to_message_index"]
            .as_u64()
            .unwrap() as usize;
        assert!(trimmed_idx > 0, "Should have trimmed messages");

        // Verify trimming is correct — user messages preserved, assistant messages trimmed
        match &result[0].content {
            stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                assert_ne!(s, "[trimmed]", "First user message should NOT be trimmed");
            }
            _ => panic!("Expected string content"),
        }
        match &result[1].content {
            stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                assert_eq!(s, "[trimmed]", "First assistant message should be trimmed");
            }
            _ => panic!("Expected string content"),
        }
    }

    /// Verify that metadata is correctly persisted and loaded through multiple
    /// checkpoint saves within the same session (simulating multi-turn async mode).
    #[tokio::test]
    async fn test_async_metadata_persists_through_checkpoint_chain() {
        let storage = create_test_storage().await;

        let session = storage
            .create_session(&session_request(
                "Checkpoint Chain",
                vec![user_msg("start")],
            ))
            .await
            .unwrap();

        // Checkpoint 1: no metadata
        let cp1 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![user_msg("start"), assistant_msg("ok")])
                    .with_parent(session.checkpoint.id),
            )
            .await
            .unwrap();

        let loaded1 = storage.get_checkpoint(cp1.id).await.unwrap();
        assert!(loaded1.state.metadata.is_none());

        // Checkpoint 2: with trimming metadata
        let meta2 = serde_json::json!({"trimmed_up_to_message_index": 5});
        let cp2 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![
                    user_msg("start"),
                    assistant_msg("ok"),
                    user_msg("more"),
                    assistant_msg("sure"),
                ])
                .with_parent(cp1.id)
                .with_metadata(meta2.clone()),
            )
            .await
            .unwrap();

        let loaded2 = storage.get_checkpoint(cp2.id).await.unwrap();
        assert!(loaded2.state.metadata.is_some());
        assert_eq!(
            loaded2.state.metadata.as_ref().unwrap()["trimmed_up_to_message_index"],
            serde_json::json!(5)
        );

        // Checkpoint 3: updated trimming metadata (index advances)
        let meta3 = serde_json::json!({"trimmed_up_to_message_index": 8});
        let cp3 = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(vec![
                    user_msg("start"),
                    assistant_msg("ok"),
                    user_msg("more"),
                    assistant_msg("sure"),
                    user_msg("even more"),
                    assistant_msg("got it"),
                ])
                .with_parent(cp2.id)
                .with_metadata(meta3.clone()),
            )
            .await
            .unwrap();

        // Active checkpoint should be the latest with correct metadata
        let active = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();
        assert_eq!(active.id, cp3.id);
        assert!(active.state.metadata.is_some());
        assert_eq!(
            active.state.metadata.as_ref().unwrap()["trimmed_up_to_message_index"],
            serde_json::json!(8),
            "Active checkpoint should have the latest trimming metadata"
        );

        // Verify each checkpoint in the chain has its own metadata
        let all_checkpoints = storage
            .list_checkpoints(
                session.session_id,
                &ListCheckpointsQuery::new().with_state(),
            )
            .await
            .unwrap();

        // initial + 3 = 4 checkpoints
        assert_eq!(all_checkpoints.checkpoints.len(), 4);
    }

    /// Verify that trimming does NOT trigger when prev_trimmed_up_to == 0
    /// and tokens are under threshold, even with an empty metadata object.
    #[tokio::test]
    async fn test_async_no_false_positive_trimming_with_empty_metadata() {
        use crate::local::context_managers::task_board_context_manager::{
            TaskBoardContextManager, TaskBoardContextManagerOptions,
        };

        let storage = create_test_storage().await;

        // Small conversation
        let messages = vec![
            user_msg("hello"),
            assistant_msg("hi"),
            user_msg("bye"),
            assistant_msg("goodbye"),
        ];

        let session = storage
            .create_session(&session_request(
                "No False Positive",
                vec![messages[0].clone()],
            ))
            .await
            .unwrap();

        // Save checkpoint with empty metadata object (not None, but {})
        let cp = storage
            .create_checkpoint(
                session.session_id,
                &CreateCheckpointRequest::new(messages.clone())
                    .with_parent(session.checkpoint.id)
                    .with_metadata(serde_json::json!({})),
            )
            .await
            .unwrap();

        // Resume
        let loaded = storage
            .get_active_checkpoint(session.session_id)
            .await
            .unwrap();
        assert_eq!(loaded.id, cp.id);
        assert!(loaded.state.metadata.is_some());

        // Run with large context window — should NOT trim
        let cm = TaskBoardContextManager::new(TaskBoardContextManagerOptions {
            keep_last_n_assistant_messages: 50,
            context_budget_threshold: 0.8,
        });

        let (result, _metadata) =
            cm.reduce_context_with_budget(messages, 200_000, loaded.state.metadata, None);

        // Should return the empty metadata as-is (no trimming triggered)
        // The function returns metadata unchanged when under threshold and prev_trimmed_up_to == 0
        for msg in &result {
            match &msg.content {
                stakpak_shared::models::llm::LLMMessageContent::String(s) => {
                    assert_ne!(s, "[trimmed]", "No messages should be trimmed");
                }
                _ => {}
            }
        }
    }

    // =========================================================================
    // Migration tests
    // =========================================================================

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
