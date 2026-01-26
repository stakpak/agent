//! Example: Comparing responses from multiple providers

use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Inference::new();

    let question = "What is the meaning of life?";

    let mut request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message::new(Role::User, question)],
    );
    request.options.temperature = Some(0.7);
    request.options.max_tokens = Some(200);

    println!("Question: {}\n", question);
    println!("{}", "=".repeat(80));

    // Try OpenAI
    request.model = Model::custom("gpt-4", "openai");
    if let Ok(response) = client.generate(&request).await {
        println!("\n🤖 OpenAI GPT-4:");
        println!("{}", response.text());
        println!("Tokens: {}", response.usage.total_tokens);
    }

    // Try Anthropic
    request.model = Model::custom("claude-3-5-sonnet-20241022", "anthropic");
    if let Ok(response) = client.generate(&request).await {
        println!("\n🤖 Anthropic Claude:");
        println!("{}", response.text());
        println!("Tokens: {}", response.usage.total_tokens);
    }

    // Try Gemini
    request.model = Model::custom("gemini-2.0-flash-exp", "google");
    if let Ok(response) = client.generate(&request).await {
        println!("\n🤖 Google Gemini:");
        println!("{}", response.text());
        println!("Tokens: {}", response.usage.total_tokens);
    }

    println!("\n{}", "=".repeat(80));

    Ok(())
}
