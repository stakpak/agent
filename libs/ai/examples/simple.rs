//! Simplest possible example

use stakai::{GenerateRequest, Inference, Message, Model, Role};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Inference::new();

    let request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message::new(Role::User, "What is 2+2?")],
    );

    let response = client.generate(&request).await?;

    println!("{}", response.text());

    Ok(())
}
