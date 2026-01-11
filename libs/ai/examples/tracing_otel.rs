//! Example: OpenTelemetry tracing with StakAI
//!
//! This example shows how to set up OpenTelemetry tracing with StakAI.
//! When the `tracing` feature is enabled, all `generate()` and `stream()` calls
//! automatically emit spans with GenAI semantic conventions.
//!
//! # Quick Start with Jaeger
//!
//! 1. Start Jaeger:
//!    ```bash
//!    docker compose -f docker-compose.tracing.yml up -d
//!    ```
//!
//! 2. Run this example (requires additional dependencies - see setup below)
//!
//! 3. View traces at http://localhost:16686
//!
//! # Setup
//!
//! Add these dependencies to your Cargo.toml:
//! ```toml
//! [dependencies]
//! stakai = { version = "0.3", features = ["tracing"] }
//! tracing = "0.1"
//! tracing-subscriber = { version = "0.3", features = ["env-filter"] }
//! tracing-opentelemetry = "0.28"
//! opentelemetry = "0.28"
//! opentelemetry_sdk = { version = "0.28", features = ["rt-tokio"] }
//! opentelemetry-otlp = "0.28"
//! ```
//!
//! # Span Attributes (GenAI Semantic Conventions)
//!
//! The following attributes are automatically added to spans:
//!
//! | Attribute | Description |
//! |-----------|-------------|
//! | `gen_ai.operation.name` | "chat" or "stream" |
//! | `gen_ai.system` | Provider name (e.g., "openai") |
//! | `gen_ai.request.model` | Model identifier |
//! | `gen_ai.request.temperature` | Temperature setting (if set) |
//! | `gen_ai.request.max_tokens` | Max tokens (if set) |
//! | `gen_ai.usage.input_tokens` | Prompt tokens used |
//! | `gen_ai.usage.output_tokens` | Completion tokens used |
//! | `gen_ai.response.finish_reasons` | Finish reason |

fn main() {
    println!("OpenTelemetry Tracing Example");
    println!("=============================\n");
    println!("This example demonstrates the tracing pattern for StakAI.\n");
    println!("Quick Start:");
    println!("1. Start Jaeger: docker compose -f docker-compose.tracing.yml up -d");
    println!("2. Add the required dependencies (see source code)");
    println!("3. View traces at http://localhost:16686\n");
    println!("Example setup code:\n");

    print_example_code();
}

fn print_example_code() {
    let code = r#"
// ============================================================================
// Complete Example: StakAI with OpenTelemetry + Jaeger
// ============================================================================

use stakai::{GenerateRequest, Inference, Message, Role};
use tracing_subscriber::prelude::*;

fn init_tracing() -> Result<(), Box<dyn std::error::Error>> {
    // Set up OTLP exporter pointing to Jaeger
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint("http://localhost:4317");

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                .with_resource(opentelemetry_sdk::Resource::new(vec![
                    opentelemetry::KeyValue::new("service.name", "my-ai-app"),
                ]))
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    // Bridge tracing -> OpenTelemetry
    tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(tracing_subscriber::fmt::layer()) // Also log to console
        .with(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("stakai=debug".parse()?))
        .init();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing()?;

    let client = Inference::new();

    // All calls are automatically traced with GenAI semantic conventions!
    let request = GenerateRequest::new(
        "gpt-4",
        vec![Message::new(Role::User, "What is Rust?")]
    );

    let response = client.generate(&request).await?;
    println!("Response: {}", response.text());

    // Ensure spans are flushed before exit
    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}

// ============================================================================
// Adding Custom Attributes via Parent Spans
// ============================================================================
//
// You can add custom attributes (like Axiom GenAI fields) by wrapping
// your handler in an instrumented function:

#[tracing::instrument(fields(
    user_id = %user_id,
    session_id = %session_id,
    // Axiom GenAI fields
    "gen_ai.capability.name" = "customer_support",
    "gen_ai.step.name" = "initial_response",
))]
async fn handle_chat(
    client: &Inference,
    user_id: &str,
    session_id: &str,
    message: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let request = GenerateRequest::new(
        "gpt-4",
        vec![Message::new(Role::User, message)]
    );

    // This generates a child span with GenAI attributes
    let response = client.generate(&request).await?;

    Ok(response.text().to_string())
}

// ============================================================================
// Streaming Example
// ============================================================================
//
// Streaming also captures token usage when the stream completes:

use futures::StreamExt;
use stakai::StreamEvent;

async fn stream_example(client: &Inference) -> Result<(), Box<dyn std::error::Error>> {
    let request = GenerateRequest::new(
        "gpt-4",
        vec![Message::new(Role::User, "Count to 5")]
    );

    let mut stream = client.stream(&request).await?;

    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::TextDelta { delta, .. } => print!("{}", delta),
            StreamEvent::Finish { usage, .. } => {
                // Token usage is automatically recorded on the span here
                println!("\n[Used {} input, {} output tokens]",
                    usage.prompt_tokens, usage.completion_tokens);
            }
            _ => {}
        }
    }

    Ok(())
}
"#;
    println!("{}", code);
}
