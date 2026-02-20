//! Example: Basic Gemini generation

use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Requires GEMINI_API_KEY environment variable

    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("gemini-2.0-flash-exp", "google"),
        vec![
            Message::new(Role::System, "You are a knowledgeable science teacher."),
            Message::new(Role::User, "What causes the northern lights?"),
        ],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(400);

    println!("🤖 Generating with Gemini...\n");

    let response = client.generate(&request).await?;

    println!("Response:\n{}\n", response.text());
    println!("Usage: {:?}", response.usage);
    println!("Finish reason: {:?}", response.finish_reason);

    Ok(())
}
