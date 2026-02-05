//! Streaming generation example

use futures::StreamExt;
use stakai::{GenerateRequest, Inference, Message, Model, Role, StreamEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client
    let client = Inference::new();

    // Build a request
    let mut request = GenerateRequest::new(
        Model::custom("gpt-4", "openai"),
        vec![Message::new(
            Role::User,
            "Write a haiku about Rust programming",
        )],
    );
    request.options.temperature = Some(0.8);

    // Start streaming
    println!("Streaming response:\n");
    let mut stream = client.stream(&request).await?;

    // Process stream events
    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::Start { id } => {
                println!("Stream started (id: {})\n", id);
            }
            StreamEvent::TextDelta { delta, .. } => {
                print!("{}", delta);
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            StreamEvent::Finish { usage, reason } => {
                println!("\n\nStream finished!");
                println!("Reason: {:?}", reason);
                println!("Tokens: {}", usage.total_tokens);
            }
            StreamEvent::Error { message } => {
                eprintln!("\nError: {}", message);
            }
            _ => {}
        }
    }

    Ok(())
}
