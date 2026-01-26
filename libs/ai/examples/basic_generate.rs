//! Basic generation example

use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client (reads OPENAI_API_KEY from environment)
    let client = Inference::new();

    // Build a request
    let mut request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![
            Message::new(Role::System, "You are a helpful assistant"),
            Message::new(Role::User, "What is Rust programming language?"),
        ],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(100);

    // Generate response
    println!("Generating response...");
    let response = client.generate(&request).await?;

    // Print results
    println!("\nResponse: {}", response.text());
    println!("\nTokens used:");
    println!("  Prompt: {}", response.usage.prompt_tokens);
    println!("  Completion: {}", response.usage.completion_tokens);
    println!("  Total: {}", response.usage.total_tokens);
    println!("\nFinish reason: {:?}", response.finish_reason);

    Ok(())
}
