//! Example: Streaming with Gemini

use futures::StreamExt;
use stakai::{GenerateRequest, Inference, Message, Model, Role, StreamEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Requires GEMINI_API_KEY environment variable

    let client = Inference::new();

    let mut request = GenerateRequest::new(
        Model::custom("gemini-1.5-flash", "google"),
        vec![Message::new(
            Role::User,
            "Tell me an interesting fact about space exploration.",
        )],
    );
    request.options.temperature = Some(0.8);
    request.options.max_tokens = Some(300);

    println!("🤖 Streaming with Gemini...\n");

    let mut stream = client.stream(&request).await?;

    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::TextDelta { delta, .. } => {
                print!("{}", delta);
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            StreamEvent::Finish { usage, reason } => {
                println!("\n\n✓ Done!");
                println!("Usage: {:?}", usage);
                println!("Finish reason: {:?}", reason);
            }
            StreamEvent::Error { message } => {
                eprintln!("\n✗ Error: {}", message);
            }
            _ => {}
        }
    }

    Ok(())
}
