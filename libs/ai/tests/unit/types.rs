//! Unit tests for core types

use stakai::Model;
use stakai::types::*;

#[test]
fn test_message_creation() {
    let msg = Message {
        role: Role::User,
        content: "Hello".into(),
        name: None,
        provider_options: None,
    };
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.text(), Some("Hello".to_string()));
}

#[test]
fn test_message_system() {
    let msg = Message {
        role: Role::System,
        content: "You are helpful".into(),
        name: None,
        provider_options: None,
    };
    assert_eq!(msg.role, Role::System);
    assert_eq!(msg.text(), Some("You are helpful".to_string()));
}

#[test]
fn test_message_assistant() {
    let msg = Message {
        role: Role::Assistant,
        content: "I can help".into(),
        name: None,
        provider_options: None,
    };
    assert_eq!(msg.role, Role::Assistant);
    assert_eq!(msg.text(), Some("I can help".to_string()));
}

#[test]
fn test_message_with_name() {
    let msg = Message {
        role: Role::User,
        content: "Hello".into(),
        name: Some("Alice".to_string()),
        provider_options: None,
    };
    assert_eq!(msg.name, Some("Alice".to_string()));
}

#[test]
fn test_content_part_text() {
    let part = ContentPart::text("Hello");
    match part {
        ContentPart::Text { text, .. } => assert_eq!(text, "Hello"),
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_content_part_image() {
    let part = ContentPart::image("https://example.com/image.jpg");
    match part {
        ContentPart::Image { url, detail, .. } => {
            assert_eq!(url, "https://example.com/image.jpg");
            assert_eq!(detail, None);
        }
        _ => panic!("Expected image content"),
    }
}

#[test]
fn test_content_part_image_with_detail() {
    let part = ContentPart::image_with_detail("https://example.com/image.jpg", ImageDetail::High);
    match part {
        ContentPart::Image { url, detail, .. } => {
            assert_eq!(url, "https://example.com/image.jpg");
            assert_eq!(detail, Some(ImageDetail::High));
        }
        _ => panic!("Expected image content"),
    }
}

#[test]
fn test_generate_request_creation() {
    let mut request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message {
            role: Role::User,
            content: "Hello".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(100);

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.options.temperature, Some(0.7));
    assert_eq!(request.options.max_tokens, Some(100));
}

#[test]
fn test_generate_request_simple() {
    let request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message {
            role: Role::User,
            content: "Hello".into(),
            name: None,
            provider_options: None,
        }],
    );
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, Role::User);
}

#[test]
fn test_generate_options() {
    let options = GenerateOptions::new()
        .temperature(0.8)
        .max_tokens(200)
        .top_p(0.9)
        .add_stop_sequence("STOP");

    assert_eq!(options.temperature, Some(0.8));
    assert_eq!(options.max_tokens, Some(200));
    assert_eq!(options.top_p, Some(0.9));
    assert_eq!(options.stop_sequences, Some(vec!["STOP".to_string()]));
}

#[test]
fn test_tool_creation() {
    let tool = Tool::function("get_weather", "Get the current weather");
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "get_weather");
    assert_eq!(tool.function.description, "Get the current weather");
}

#[test]
fn test_usage_default() {
    let usage = Usage::default();
    assert_eq!(usage.prompt_tokens, 0);
    assert_eq!(usage.completion_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn test_finish_reason_serialization() {
    use serde_json;

    let reason = FinishReason::stop();
    let json = serde_json::to_string(&reason).unwrap();
    assert!(json.contains("\"unified\":\"stop\""));

    let reason = FinishReason::length();
    let json = serde_json::to_string(&reason).unwrap();
    assert!(json.contains("\"unified\":\"length\""));

    // Test with raw value
    let reason = FinishReason::with_raw(FinishReasonKind::Stop, "end_turn");
    let json = serde_json::to_string(&reason).unwrap();
    assert!(json.contains("\"unified\":\"stop\""));
    assert!(json.contains("\"raw\":\"end_turn\""));
}

#[test]
fn test_response_text_extraction() {
    let response = GenerateResponse {
        content: vec![
            ResponseContent::Text {
                text: "Hello ".to_string(),
            },
            ResponseContent::Text {
                text: "World".to_string(),
            },
        ],
        usage: Usage::default(),
        finish_reason: FinishReason::stop(),
        metadata: None,
        warnings: None,
    };

    assert_eq!(response.text(), "Hello World");
}

#[test]
fn test_stream_event_creation() {
    let event = StreamEvent::start("test-id");
    match event {
        StreamEvent::Start { id } => assert_eq!(id, "test-id"),
        _ => panic!("Expected Start event"),
    }

    let event = StreamEvent::text_delta("test-id", "Hello");
    match event {
        StreamEvent::TextDelta { id, delta } => {
            assert_eq!(id, "test-id");
            assert_eq!(delta, "Hello");
        }
        _ => panic!("Expected TextDelta event"),
    }
}
