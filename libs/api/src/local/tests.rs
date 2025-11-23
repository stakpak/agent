use super::*;

use uuid::Uuid;

#[tokio::test]
async fn test_local_db_operations() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir
        .path()
        .join("stakpak-test.db")
        .to_string_lossy()
        .to_string();

    let config = LocalClientConfig {
        store_path: Some(db_path.clone()),
    };

    let client = LocalClient::new(config)
        .await
        .expect("Failed to create local client");

    // Test Session CRUD
    let session_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let session = AgentSession {
        id: session_id,
        title: "Test Session".to_string(),
        agent_id: AgentID::PabloV1,
        visibility: AgentSessionVisibility::Private,
        checkpoints: vec![],
        created_at: now,
        updated_at: now,
    };

    db::create_session(&client.db, &session)
        .await
        .expect("Failed to create session");

    let sessions = db::list_sessions(&client.db)
        .await
        .expect("Failed to list sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session_id);

    let fetched_session = db::get_session(&client.db, session_id)
        .await
        .expect("Failed to get session");
    assert_eq!(fetched_session.id, session_id);
    assert_eq!(fetched_session.title, "Test Session");

    // Test Checkpoint CRUD
    let checkpoint_id = Uuid::new_v4();
    let checkpoint = AgentCheckpointListItem {
        id: checkpoint_id,
        status: AgentStatus::Running,
        execution_depth: 1,
        parent: None,
        created_at: now,
        updated_at: now,
    };

    use stakpak_shared::models::integrations::openai::{ChatMessage, MessageContent, Role};

    let output = AgentOutput::PabloV1 {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("Hello".to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        node_states: serde_json::Value::Null,
    };

    db::create_checkpoint(&client.db, session_id, &checkpoint, &output)
        .await
        .expect("Failed to create checkpoint");

    let fetched_checkpoint = db::get_checkpoint(&client.db, checkpoint_id)
        .await
        .expect("Failed to get checkpoint");
    assert_eq!(fetched_checkpoint.checkpoint.id, checkpoint_id);
    assert_eq!(fetched_checkpoint.session.id, session_id);

    let latest_checkpoint = db::get_latest_checkpoint(&client.db, session_id)
        .await
        .expect("Failed to get latest checkpoint");
    assert_eq!(latest_checkpoint.checkpoint.id, checkpoint_id);

    let AgentOutput::PabloV1 { messages, .. } = latest_checkpoint.output;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, Role::User);
    if let Some(MessageContent::String(content)) = &messages[0].content {
        assert_eq!(content, "Hello");
    } else {
        panic!("Unexpected message content");
    }

    drop(client);
    // temp_dir will be dropped here and clean up the directory
}
