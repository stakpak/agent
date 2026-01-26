//! Example: Using custom headers with providers

use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Inference::new();

    // Create request with custom headers using GenerateOptions
    let mut request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message::new(Role::User, "Hello!")],
    );
    request.options.max_tokens = Some(100);
    request.options = request
        .options
        .add_header("X-Custom-Header", "my-value")
        .add_header("X-Request-ID", "12345");

    println!("🤖 Sending request with custom headers...\n");

    // Works with any provider
    let response = client.generate(&request).await?;

    println!("Response: {}\n", response.text());

    Ok(())
}
