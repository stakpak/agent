//! Example: Basic Anthropic generation

use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Requires ANTHROPIC_API_KEY environment variable

    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("claude-3-5-sonnet-20241022", "anthropic"),
        vec![
            Message::new(Role::System, "You are a helpful AI assistant."),
            Message::new(Role::User, "Explain quantum computing in simple terms."),
        ],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(500);

    println!("🤖 Generating with Claude...\n");

    let response = client.generate(&request).await?;

    println!("Response:\n{}\n", response.text());
    println!("Usage: {:?}", response.usage);
    println!("Finish reason: {:?}", response.finish_reason);

    Ok(())
}
