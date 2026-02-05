//! Gemini integration tests
//!
//! Run with: cargo test --test integration -- --ignored

use futures::StreamExt;
use stakai::{GenerateRequest, Inference, Message, Model, Role, StreamEvent};

#[tokio::test]
#[ignore] // Requires GEMINI_API_KEY
async fn test_gemini_generate() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("gemini-2.5-flash-lite-preview-09-2025", "google"),
        vec![Message {
            role: Role::User,
            content: "Say 'Hello, World!' and nothing else".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(1000);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    assert!(!response.text().is_empty());
}

#[tokio::test]
#[ignore] // Requires GEMINI_API_KEY
async fn test_gemini_streaming() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("gemini-2.5-flash-lite-preview-09-2025", "google"),
        vec![Message {
            role: Role::User,
            content: "Count from 1 to 3".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(2000);

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
