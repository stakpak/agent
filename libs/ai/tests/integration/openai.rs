//! OpenAI integration tests
//!
//! Run with: cargo test --test integration -- --ignored

use futures::StreamExt;
use stakai::{GenerateRequest, Inference, Message, Model, Role, StreamEvent};

#[tokio::test]
#[ignore] // Requires OPENAI_API_KEY
async fn test_openai_generate() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("gpt-5-mini-2025-08-07", "openai"),
        vec![Message {
            role: Role::User,
            content: "Say 'Hello, World!' and nothing else".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(500);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    assert!(!response.text().is_empty());
    assert!(response.usage.total_tokens > 0);
}

#[tokio::test]
#[ignore] // Requires OPENAI_API_KEY
async fn test_openai_streaming() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("gpt-5-mini-2025-08-07", "openai"),
        vec![Message {
            role: Role::User,
            content: "Count from 1 to 3".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.0);
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

    assert!(finished, "Stream did not finish properly");
    assert!(!text.is_empty(), "No text received from stream");
}

#[tokio::test]
#[ignore] // Requires OPENAI_API_KEY
async fn test_openai_with_system_message() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("gpt-5-mini-2025-08-07", "openai"),
        vec![
            Message {
                role: Role::System,
                content: "You are a helpful assistant that responds in one word".into(),
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
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(200);

    let response = client.generate(&request).await;

    assert!(response.is_ok());
    let response = response.unwrap();

    assert!(!response.text().is_empty());
}

#[tokio::test]
#[ignore] // Requires OPENAI_API_KEY
async fn test_openai_explicit_provider() {
    let client = Inference::new();

    let request = GenerateRequest::new(
        Model::custom("gpt-3.5-turbo", "openai"),
        vec![Message {
            role: Role::User,
            content: "Say hello".into(),
            name: None,
            provider_options: None,
        }],
    );

    // Test with explicit provider/model format
    let response = client.generate(&request).await;

    assert!(response.is_ok());
}

#[tokio::test]
#[ignore] // Requires OPENAI_API_KEY
async fn test_openai_temperature_variation() {
    let client = Inference::new();

    // Test with temperature 0 (deterministic)
    let mut request = GenerateRequest::new(
        Model::custom("gpt-3.5-turbo", "openai"),
        vec![Message {
            role: Role::User,
            content: "Say exactly: 'Test'".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(200);

    let response = client.generate(&request).await;
    assert!(response.is_ok());
}
