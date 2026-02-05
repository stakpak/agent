//! Test example for InferenceConfig

use stakai::{GenerateRequest, Inference, InferenceConfig, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test 1: Create client with explicit config
    println!("=== Test 1: Explicit Config ===");
    let client = Inference::with_config(
        InferenceConfig::new()
            .openai(
                "test-key-openai",
                Some("https://api.openai.com/v1".to_string()),
            )
            .anthropic("test-key-anthropic", None)
            .gemini("test-key-gemini", None)
            .temperature(0.7)
            .max_tokens(100),
    )?;

    println!("✓ Client created with explicit config");
    println!(
        "  Registered providers: {:?}",
        client.registry().list_providers()
    );

    // Test 2: Create client with default config (from env vars)
    println!("\n=== Test 2: Default Config (from env) ===");
    let client_default = Inference::new();
    println!("✓ Client created with default config");
    println!(
        "  Registered providers: {:?}",
        client_default.registry().list_providers()
    );

    // Test 3: Builder pattern with InferenceConfig
    println!("\n=== Test 3: Builder Pattern ===");
    let client_builder = Inference::builder()
        .with_inference_config(InferenceConfig::new().openai("builder-key", None))
        .with_temperature(0.5)
        .build()?;

    println!("✓ Client created with builder pattern");
    println!(
        "  Registered providers: {:?}",
        client_builder.registry().list_providers()
    );

    // Test 4: Try to make a request (will fail without real API key, but tests the API)
    println!("\n=== Test 4: Request API Test ===");
    let request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message::new(Role::User, "Hello!")],
    );

    println!("✓ Request created successfully");
    println!("  Model: {}", request.model.id);
    println!("  Messages: {}", request.messages.len());

    println!("\n=== All Tests Passed ===");
    Ok(())
}
