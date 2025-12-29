//! Anthropic integration tests
//!
//! Run with: cargo test --test integration -- --ignored

use futures::StreamExt;
use stakai::{GenerateRequest, Inference, Message, Role, StreamEvent};

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_generate() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        "claude-haiku-4-5",
        vec![Message {
            role: Role::User,
            content: "Say 'Hello, World!' and nothing else".into(),
            name: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(10);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    assert!(!response.text().is_empty());
    assert!(response.usage.total_tokens > 0);
}

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_streaming() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        "claude-haiku-4-5",
        vec![Message {
            role: Role::User,
            content: "Count from 1 to 3".into(),
            name: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(20);

    let stream = client.stream(&request).await;
    assert!(stream.is_ok(), "Stream creation failed: {:?}", stream.err());

    let mut stream = stream.unwrap();
    let mut text = String::new();
    let mut finished = false;

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::TextDelta { delta, .. }) => {
                text.push_str(&delta);
            }
            Ok(StreamEvent::Finish { .. }) => {
                finished = true;
                break;
            }
            Ok(_) => {}
            Err(e) => panic!("Stream error: {:?}", e),
        }
    }

    assert!(finished, "Stream did not finish properly");
    assert!(!text.is_empty(), "No text received from stream");
}
