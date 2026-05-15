//! MiniMax integration tests
//!
//! Run with: cargo test --test lib -- --ignored minimax

use futures::StreamExt;
use stakai::{GenerateRequest, Inference, Message, Model, Role, StreamEvent};

#[tokio::test]
#[ignore] // Requires MINIMAX_API_KEY
async fn test_minimax_generate() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("MiniMax-M2.7", "minimax"),
        vec![Message {
            role: Role::User,
            content: "Say 'Hello, World!' and nothing else".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.5);
    request.options.max_tokens = Some(500);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    assert!(!response.text().is_empty());
    assert!(response.usage.total_tokens > 0);
}

#[tokio::test]
#[ignore] // Requires MINIMAX_API_KEY
async fn test_minimax_streaming() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("MiniMax-M2.7", "minimax"),
        vec![Message {
            role: Role::User,
            content: "Count from 1 to 3".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.5);
    request.options.max_tokens = Some(200);

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

    assert!(finished, "Stream should finish");
    assert!(!text.is_empty(), "Should receive text content");
}

#[tokio::test]
#[ignore] // Requires MINIMAX_API_KEY
async fn test_minimax_with_system_message() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("MiniMax-M2.7", "minimax"),
        vec![
            Message {
                role: Role::System,
                content: "You are a helpful assistant that always responds in exactly one word."
                    .into(),
                name: None,
                provider_options: None,
            },
            Message {
                role: Role::User,
                content: "What color is the sky?".into(),
                name: None,
                provider_options: None,
            },
        ],
    );
    request.options.temperature = Some(0.1);
    request.options.max_tokens = Some(50);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();
    assert!(!response.text().is_empty());
}
