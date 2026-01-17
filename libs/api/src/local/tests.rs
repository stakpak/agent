use super::*;
use crate::local::context_managers::{
    ContextManager, simple_context_manager::SimpleContextManager,
};
use crate::models::{
    AgentCheckpointListItem, AgentID, AgentOutput, AgentSession, AgentSessionVisibility,
    AgentStatus,
};
use stakpak_shared::models::integrations::openai::{
    ChatMessage, ContentPart, ImageUrl, MessageContent, Role,
};
use stakpak_shared::models::llm::{LLMMessage, LLMMessageContent, LLMMessageTypedContent};
use uuid::Uuid;

#[tokio::test]
async fn test_local_db_operations() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir
        .path()
        .join("stakpak-test.db")
        .to_string_lossy()
        .to_string();

    // Initialize database directly
    let db_builder = libsql::Builder::new_local(&db_path)
        .build()
        .await
        .expect("Failed to build database");
    let conn = db_builder.connect().expect("Failed to connect to database");
    db::init_schema(&conn)
        .await
        .expect("Failed to initialize schema");

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

    db::create_session(&conn, &session)
        .await
        .expect("Failed to create session");

    let sessions = db::list_sessions(&conn)
        .await
        .expect("Failed to list sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session_id);

    let fetched_session = db::get_session(&conn, session_id)
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

    let output = AgentOutput::PabloV1 {
        messages: vec![ChatMessage {
            role: Role::User,
            content: Some(MessageContent::String("Hello".to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            usage: None,
            ..Default::default()
        }],
        node_states: serde_json::Value::Null,
    };

    db::create_checkpoint(&conn, session_id, &checkpoint, &output)
        .await
        .expect("Failed to create checkpoint");

    let fetched_checkpoint = db::get_checkpoint(&conn, checkpoint_id)
        .await
        .expect("Failed to get checkpoint");
    assert_eq!(fetched_checkpoint.checkpoint.id, checkpoint_id);
    assert_eq!(fetched_checkpoint.session.id, session_id);

    let latest_checkpoint = db::get_latest_checkpoint(&conn, session_id)
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

    // temp_dir will be dropped here and clean up the directory
}

#[test]
fn test_context_manager_preserves_last_message_image() {
    let context_manager = SimpleContextManager;

    // Create a history message with an image (should be dropped)
    let history_msg = ChatMessage {
        role: Role::User,
        content: Some(MessageContent::Array(vec![
            ContentPart {
                r#type: "text".to_string(),
                text: Some("History text".to_string()),
                image_url: None,
            },
            ContentPart {
                r#type: "image_url".to_string(),
                text: None,
                image_url: Some(ImageUrl {
                    url: "data:image/jpeg;base64,history".to_string(),
                    detail: None,
                }),
            },
        ])),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
        ..Default::default()
    };

    // Create a last message with an image (should be preserved)
    let last_msg = ChatMessage {
        role: Role::User,
        content: Some(MessageContent::Array(vec![
            ContentPart {
                r#type: "text".to_string(),
                text: Some("Last message text".to_string()),
                image_url: None,
            },
            ContentPart {
                r#type: "image_url".to_string(),
                text: None,
                image_url: Some(ImageUrl {
                    url: "data:image/jpeg;base64,last".to_string(),
                    detail: None,
                }),
            },
        ])),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
        ..Default::default()
    };

    let messages = vec![history_msg, last_msg];
    let reduced = context_manager.reduce_context(messages);

    assert_eq!(reduced.len(), 2);

    // Check history (first message)
    match &reduced[0].content {
        LLMMessageContent::String(s) => {
            assert!(s.contains("History text"));
            // Image should be dropped from string representation
            assert!(!s.contains("data:image/jpeg;base64,history"));
        }
        _ => panic!("History should be flattened to string"),
    }

    // Check last message (second message)
    match &reduced[1].content {
        LLMMessageContent::List(parts) => {
            assert_eq!(parts.len(), 2);
            match &parts[0] {
                LLMMessageTypedContent::Text { text } => assert_eq!(text, "Last message text"),
                _ => panic!("First part should be text"),
            }
            match &parts[1] {
                LLMMessageTypedContent::Image { source } => {
                    assert_eq!(source.data, "last");
                }
                _ => panic!("Second part should be image"),
            }
        }
        _ => panic!("Last message should be preserved as list"),
    }
}

#[test]
fn test_openai_message_conversion() {
    // Test ChatMessage -> LLMMessage
    let chat_msg = ChatMessage {
        role: Role::User,
        content: Some(MessageContent::Array(vec![
            ContentPart {
                r#type: "text".to_string(),
                text: Some("Text part".to_string()),
                image_url: None,
            },
            ContentPart {
                r#type: "image_url".to_string(),
                text: None,
                image_url: Some(ImageUrl {
                    url: "data:image/png;base64,xyz".to_string(),
                    detail: None,
                }),
            },
        ])),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
        ..Default::default()
    };

    let llm_msg = LLMMessage::from(chat_msg.clone());

    match &llm_msg.content {
        LLMMessageContent::List(parts) => {
            assert_eq!(parts.len(), 2);
            match &parts[0] {
                LLMMessageTypedContent::Text { text } => assert_eq!(text, "Text part"),
                _ => panic!("Expected text part"),
            }
            match &parts[1] {
                LLMMessageTypedContent::Image { source } => {
                    assert_eq!(source.data, "xyz"); // Should be stripped of prefix
                    assert_eq!(source.media_type, "image/png"); // Should be parsed from prefix
                    assert_eq!(source.r#type, "base64");
                }
                _ => panic!("Expected image part"),
            }
        }
        _ => panic!("Expected list content"),
    }

    // Test LLMMessage -> ChatMessage
    // Note: The reconstruction back to ChatMessage currently just puts the raw data back into the URL.
    // It doesn't reconstruct the full data URL prefix if it was stripped.
    // This is acceptable for now as long as the outbound path (ChatMessage -> LLMMessage) is correct for the provider.
    let chat_msg_back = ChatMessage::from(llm_msg);
    match chat_msg_back.content {
        Some(MessageContent::Array(parts)) => {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0].r#type, "text");
            assert_eq!(parts[0].text.as_deref(), Some("Text part"));

            assert_eq!(parts[1].r#type, "image_url");
            assert!(parts[1].image_url.is_some());
            // The implementation of From<LLMMessage> for ChatMessage now reconstructs the data URL.
            assert_eq!(
                parts[1].image_url.as_ref().unwrap().url,
                "data:image/png;base64,xyz"
            );
        }
        _ => panic!("Expected array content"),
    }
}
