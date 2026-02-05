//! Anthropic integration tests
//!
//! Run with: ANTHROPIC_API_KEY=your_key cargo test -p stakai --test lib anthropic -- --ignored --nocapture

use futures::StreamExt;
use stakai::{
    GenerateRequest, Inference, InferenceConfig, Message, Model, Role, StreamEvent, Tool,
    ToolChoice,
};

// =============================================================================
// Basic Generation Tests
// =============================================================================

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_generate() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "Say 'Hello, World!' and nothing else".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(10);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    assert!(!response.text().is_empty());
    assert!(response.usage.total_tokens > 0);
    println!("Response: {}", response.text());
    println!("Usage: {:?}", response.usage);
}

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_generate_with_system_message() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![
            Message {
                role: Role::System,
                content: "You are a pirate. Respond in pirate speak.".into(),
                name: None,
                provider_options: None,
            },
            Message {
                role: Role::User,
                content: "Say hello".into(),
                name: None,
                provider_options: None,
            },
        ],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(50);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    assert!(!response.text().is_empty());
    println!("Pirate response: {}", response.text());
}

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_explicit_provider_prefix() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "Say 'test'".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.max_tokens = Some(10);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    println!(
        "Response with explicit provider: {}",
        response.unwrap().text()
    );
}

// =============================================================================
// Streaming Tests
// =============================================================================

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_streaming() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "Count from 1 to 5".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(50);

    let stream = client.stream(&request).await;
    assert!(stream.is_ok(), "Stream creation failed: {:?}", stream.err());

    let mut stream = stream.unwrap();
    let mut text = String::new();
    let mut finished = false;
    let mut chunk_count = 0;

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::TextDelta { delta, .. }) => {
                text.push_str(&delta);
                chunk_count += 1;
                print!("{}", delta); // Print as we receive
            }
            Ok(StreamEvent::Finish { usage, .. }) => {
                finished = true;
                println!("\n\nStream finished. Usage: {:?}", usage);
                break;
            }
            Ok(other) => {
                println!("Other event: {:?}", other);
            }
            Err(e) => panic!("Stream error: {:?}", e),
        }
    }

    assert!(finished, "Stream did not finish properly");
    assert!(!text.is_empty(), "No text received from stream");
    assert!(chunk_count > 0, "No chunks received");
    println!("Total chunks received: {}", chunk_count);
}

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_streaming_with_system_message() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![
            Message {
                role: Role::System,
                content: "You respond in exactly 3 words.".into(),
                name: None,
                provider_options: None,
            },
            Message {
                role: Role::User,
                content: "What is rust?".into(),
                name: None,
                provider_options: None,
            },
        ],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(20);

    let stream = client.stream(&request).await;
    assert!(stream.is_ok(), "Stream creation failed: {:?}", stream.err());

    let mut stream = stream.unwrap();
    let mut text = String::new();

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::TextDelta { delta, .. }) => {
                text.push_str(&delta);
            }
            Ok(StreamEvent::Finish { .. }) => break,
            Ok(_) => {}
            Err(e) => panic!("Stream error: {:?}", e),
        }
    }

    assert!(!text.is_empty());
    println!("3-word response: {}", text);
}

// =============================================================================
// Tool Calling Tests
// =============================================================================

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_tool_calling() {
    let client = Inference::new();

    let weather_tool = Tool::function("get_weather", "Get the current weather for a location")
        .parameters(serde_json::json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The city and state, e.g. San Francisco, CA"
                }
            },
            "required": ["location"]
        }));

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "What's the weather in Tokyo?".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.tools = Some(vec![weather_tool]);
    request.options.tool_choice = Some(ToolChoice::Auto);
    request.options.max_tokens = Some(200);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    // Should have tool calls
    let tool_calls = response.tool_calls();
    assert!(
        !tool_calls.is_empty(),
        "Expected tool calls but got none. Response: {}",
        response.text()
    );

    let tool_call = tool_calls[0];
    assert_eq!(tool_call.name, "get_weather");
    println!("Tool call: {:?}", tool_call);
    println!(
        "Arguments: {}",
        serde_json::to_string_pretty(&tool_call.arguments).unwrap()
    );
}

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_tool_calling_streaming() {
    let client = Inference::new();

    let calculator_tool = Tool::function("calculate", "Perform a mathematical calculation")
        .parameters(serde_json::json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "The mathematical expression to evaluate"
                }
            },
            "required": ["expression"]
        }));

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "What is 123 * 456?".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.tools = Some(vec![calculator_tool]);
    request.options.tool_choice = Some(ToolChoice::Auto);
    request.options.max_tokens = Some(200);

    let stream = client.stream(&request).await;
    assert!(stream.is_ok(), "Stream creation failed: {:?}", stream.err());

    let mut stream = stream.unwrap();
    let mut tool_call_started = false;
    let mut tool_arguments = String::new();

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::ToolCallStart { name, .. }) => {
                tool_call_started = true;
                println!("Tool call started: {}", name);
            }
            Ok(StreamEvent::ToolCallDelta { delta, .. }) => {
                tool_arguments.push_str(&delta);
            }
            Ok(StreamEvent::Finish { .. }) => {
                println!("Stream finished");
                break;
            }
            Ok(StreamEvent::TextDelta { delta, .. }) => {
                print!("{}", delta);
            }
            Ok(other) => {
                println!("Event: {:?}", other);
            }
            Err(e) => panic!("Stream error: {:?}", e),
        }
    }

    assert!(
        tool_call_started,
        "Expected tool call in stream but got none"
    );
    println!("Tool arguments received: {}", tool_arguments);
}

// =============================================================================
// Configuration Tests
// =============================================================================

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_custom_base_url_with_messages_suffix() {
    // Test that URL normalization works - user provides full endpoint URL
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY not set");

    let config = InferenceConfig::new().anthropic(
        api_key,
        Some("https://api.anthropic.com/v1/messages".to_string()), // Full URL with /messages
    );

    let client = Inference::with_config(config).expect("Failed to create client");

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "Say 'URL test passed'".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.max_tokens = Some(20);

    let response = client.generate(&request).await;

    assert!(
        response.is_ok(),
        "Request with /messages suffix in URL failed: {:?}",
        response.err()
    );
    println!("Response: {}", response.unwrap().text());
}

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_custom_base_url_without_trailing_slash() {
    // Test URL normalization without trailing slash
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY not set");

    let config = InferenceConfig::new().anthropic(
        api_key,
        Some("https://api.anthropic.com/v1".to_string()), // No trailing slash
    );

    let client = Inference::with_config(config).expect("Failed to create client");

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "Say 'slash test passed'".into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.max_tokens = Some(20);

    let response = client.generate(&request).await;

    assert!(
        response.is_ok(),
        "Request without trailing slash failed: {:?}",
        response.err()
    );
    println!("Response: {}", response.unwrap().text());
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_invalid_model_error() {
    let client = Inference::new();

    let request = GenerateRequest::new(
        Model::custom("claude-invalid-model-12345", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "Test".into(),
            name: None,
            provider_options: None,
        }],
    );

    let response = client.generate(&request).await;

    assert!(response.is_err(), "Expected error for invalid model");
    let err = response.err().unwrap();
    println!("Expected error received: {:?}", err);
}

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_streaming_invalid_model_error() {
    let client = Inference::new();

    let request = GenerateRequest::new(
        Model::custom("claude-invalid-model-12345", "anthropic"),
        vec![Message {
            role: Role::User,
            content: "Test".into(),
            name: None,
            provider_options: None,
        }],
    );

    let stream_result = client.stream(&request).await;
    assert!(
        stream_result.is_ok(),
        "Stream creation should succeed initially"
    );

    let mut stream = stream_result.unwrap();

    // The error should come when we try to read from the stream
    let mut got_error = false;
    while let Some(event) = stream.next().await {
        match event {
            Err(e) => {
                got_error = true;
                println!("Expected streaming error received: {:?}", e);
                break;
            }
            Ok(event) => {
                println!("Unexpected event: {:?}", event);
            }
        }
    }

    assert!(
        got_error,
        "Expected an error from the stream for invalid model"
    );
}

#[tokio::test]
async fn test_anthropic_missing_api_key_error() {
    // Create config without API key - this should return an error when building
    let config = InferenceConfig::new();
    let client_result = Inference::with_config(config);

    // If client builds successfully (because other providers might be available),
    // try to make a request that would fail
    if let Ok(client) = client_result {
        let request = GenerateRequest::new(
            Model::custom("claude-haiku-4-5-20251001", "anthropic"), // Explicitly use anthropic
            vec![Message {
                role: Role::User,
                content: "Test".into(),
                name: None,
                provider_options: None,
            }],
        );

        let response = client.generate(&request).await;

        // Should fail because no API key is configured for Anthropic
        assert!(response.is_err(), "Expected error when API key is missing");
        println!("Expected error: {:?}", response.err());
    } else {
        println!(
            "Client creation failed as expected: {:?}",
            client_result.err()
        );
    }
}

// =============================================================================
// Multi-turn Conversation Tests
// =============================================================================

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_multi_turn_conversation() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![
            Message {
                role: Role::User,
                content: "My name is Alice.".into(),
                name: None,
                provider_options: None,
            },
            Message {
                role: Role::Assistant,
                content: "Hello Alice! Nice to meet you.".into(),
                name: None,
                provider_options: None,
            },
            Message {
                role: Role::User,
                content: "What is my name?".into(),
                name: None,
                provider_options: None,
            },
        ],
    );
    request.options.temperature = Some(0.0);
    request.options.max_tokens = Some(50);

    let response = client.generate(&request).await;

    assert!(response.is_ok(), "Request failed: {:?}", response.err());
    let response = response.unwrap();

    let text = response.text().to_lowercase();
    assert!(
        text.contains("alice"),
        "Expected response to contain 'Alice', got: {}",
        response.text()
    );
    println!("Multi-turn response: {}", response.text());
}

// =============================================================================
// Long Response Tests
// =============================================================================

#[tokio::test]
#[ignore] // Requires ANTHROPIC_API_KEY
async fn test_anthropic_streaming_long_response() {
    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-haiku-4-5-20251001", "anthropic"),
        vec![Message {
            role: Role::User,
            content:
                "Write a short paragraph (about 100 words) about the Rust programming language."
                    .into(),
            name: None,
            provider_options: None,
        }],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(200);

    let stream = client.stream(&request).await;
    assert!(stream.is_ok(), "Stream creation failed: {:?}", stream.err());

    let mut stream = stream.unwrap();
    let mut text = String::new();
    let mut chunk_count = 0;

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamEvent::TextDelta { delta, .. }) => {
                text.push_str(&delta);
                chunk_count += 1;
            }
            Ok(StreamEvent::Finish { usage, .. }) => {
                println!("\n\nFinished. Total chunks: {}", chunk_count);
                println!("Usage: {:?}", usage);
                break;
            }
            Ok(_) => {}
            Err(e) => panic!("Stream error: {:?}", e),
        }
    }

    assert!(
        text.len() > 100,
        "Expected longer response, got {} chars",
        text.len()
    );
    println!("Response ({} chars):\n{}", text.len(), text);
}
