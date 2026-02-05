//! Test Anthropic provider

use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This example requires ANTHROPIC_API_KEY environment variable

    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-3-5-sonnet-20241022", "anthropic"),
        vec![Message::new(
            Role::User,
            "What is the capital of France? Answer in one word.",
        )],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(100);

    println!("Testing Anthropic provider...");

    match client.generate(&request).await {
        Ok(response) => {
            println!("✓ Success!");
            println!("Response: {}", response.text());
            println!("Usage: {:?}", response.usage);
        }
        Err(e) => {
            println!("✗ Error: {}", e);
        }
    }

    Ok(())
}
