# StakAI

<div align="center">

**A provider-agnostic Rust SDK for AI completions with streaming support**

Built by [Stakpak](https://stakpak.dev) 🚀

</div>

## Features

- 🔌 **Multi-provider**: Unified interface for OpenAI, Anthropic, and Google Gemini
- 🌊 **Streaming support**: Real-time streaming responses with unified event types
- 🦀 **Type-safe**: Strong typing with compile-time guarantees
- ⚡ **Zero-cost abstractions**: Static dispatch for optimal performance
- 🎯 **Ergonomic API**: Builder patterns and intuitive interfaces
- 🔧 **Custom headers**: Full control over HTTP headers for all providers
- 🔄 **Auto-registration**: Providers automatically registered from environment variables
- 📊 **OpenTelemetry**: Built-in observability with GenAI semantic conventions

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
stakai = "0.1"
tokio = { version = "1", features = ["full"] }
```

### Basic Usage

```rust
use stakai::{Inference, GenerateRequest, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Inference::new();
    
    let request = GenerateRequest::builder()
        .add_message(Message::user("What is Rust?"))
        .temperature(0.7)
        .build();
    
    let response = client.generate("gpt-5", request).await?;
    println!("Response: {}", response.text());
    
    Ok(())
}
```

### Streaming

```rust
use stakai::{Inference, GenerateRequest, StreamEvent};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Inference::new();
    let request = GenerateRequest::simple("Write a haiku");
    
    let mut stream = client.stream("gpt-5", request).await?;
    
    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::TextDelta { delta, .. } => print!("{}", delta),
            StreamEvent::Finish { .. } => break,
            _ => {}
        }
    }
    
    Ok(())
}
```

## Supported Providers

| Provider | Status | Models | Features |
|----------|--------|--------|----------|
| **OpenAI** | ✅ | GPT-5, GPT-4.1, o3/o4, GPT-4o | Streaming, Tools, Vision, Reasoning |
| **Anthropic** | ✅ | Claude 4.5, Claude 4.1 | Streaming, Extended Thinking |
| **Google Gemini** | ✅ | Gemini 3, Gemini 2.5, Gemini 2.0 | Streaming, Vision, Agentic Coding |

See [PROVIDERS.md](PROVIDERS.md) for detailed provider documentation.

## Configuration

### Environment Variables

The SDK automatically registers providers when their API keys are found:

```bash
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export GEMINI_API_KEY="..."
```

### Custom Configuration

```rust
use stakai::{
    Inference,
    providers::anthropic::{AnthropicProvider, AnthropicConfig},
    registry::ProviderRegistry,
};

// Custom provider configuration
let config = AnthropicConfig::new("your-api-key")
    .with_version("2023-06-01")
    .with_beta_feature("prompt-caching-2024-07-31");

let provider = AnthropicProvider::new(config)?;

// Custom registry
let registry = ProviderRegistry::new()
    .register("anthropic", provider);

let client = Inference::builder()
    .with_registry(registry)
    .build();
```

### Custom Headers

```rust
use stakai::{GenerateRequest, Message};

let request = GenerateRequest::builder()
    .add_message(Message::user("Hello"))
    .add_header("X-Request-ID", "12345")
    .add_header("X-Custom-Header", "value")
    .build();
```

## Examples

### OpenAI

```rust
use stakai::{Inference, GenerateRequest, Message};

let client = Inference::new();
let request = GenerateRequest::builder()
    .add_message(Message::user("Explain quantum computing"))
    .temperature(0.7)
    .build();

let response = client.generate("gpt-5", request).await?;
println!("{}", response.text());
```

### Anthropic (Claude)

```rust
use stakai::{Inference, GenerateRequest, Message};

let client = Inference::new();
let request = GenerateRequest::builder()
    .add_message(Message::user("Write a poem about Rust"))
    .max_tokens(500)  // Required for Anthropic
    .build();

let response = client.generate("claude-sonnet-4-5-20250929", request).await?;
println!("{}", response.text());
```

### Google Gemini

```rust
use stakai::{Inference, GenerateRequest, Message};

let client = Inference::new();
let request = GenerateRequest::builder()
    .add_message(Message::user("What causes the northern lights?"))
    .temperature(0.7)
    .build();

let response = client.generate("gemini-2.5-flash", request).await?;
println!("{}", response.text());
```

### Multi-Provider Comparison

```rust
let question = "What is the meaning of life?";
let request = GenerateRequest::builder()
    .add_message(Message::user(question))
    .build();

// Try all providers
for model in ["gpt-5", "claude-sonnet-4-5-20250929", "gemini-2.5-flash"] {
    if let Ok(response) = client.generate(model, request.clone()).await {
        println!("{}: {}", model, response.text());
    }
}
```

### Provider Options (Reasoning/Thinking)

Provider-specific options follow the Vercel AI SDK pattern using an enum:

```rust
use stakai::{
    Inference, GenerateRequest, Message,
    ProviderOptions, AnthropicOptions, ThinkingOptions,
};

let client = Inference::new();

// Anthropic extended thinking
let request = GenerateRequest::new(
    "anthropic:claude-opus-4-5-20250514",
    vec![Message::user("Solve this complex problem...")]
)
.with_provider_options(ProviderOptions::Anthropic(AnthropicOptions {
    thinking: Some(ThinkingOptions::new(12000)),
    effort: None,
}));

let response = client.generate(&request).await?;

// Access reasoning output
if let Some(reasoning) = response.reasoning() {
    println!("Reasoning: {}", reasoning);
}
println!("Response: {}", response.text());
```

For OpenAI reasoning models:

```rust
use stakai::{ProviderOptions, OpenAIOptions, ReasoningEffort};

let request = GenerateRequest::new(
    "openai:o3",
    vec![Message::user("Complex reasoning task...")]
)
.with_provider_options(ProviderOptions::OpenAI(OpenAIOptions {
    reasoning_effort: Some(ReasoningEffort::High),
    ..Default::default()
}));
```

For streaming, reasoning is delivered via `ReasoningDelta` events:

```rust
while let Some(event) = stream.next().await {
    match event? {
        StreamEvent::TextDelta { delta, .. } => print!("{}", delta),
        StreamEvent::ReasoningDelta { delta, .. } => {
            println!("[Reasoning: {}]", delta);
        }
        _ => {}
    }
}
```

### Run Examples

```bash
# Set your API keys
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export GEMINI_API_KEY="..."

# Run examples
cargo run --example openai_generate
cargo run --example anthropic_generate
cargo run --example anthropic_stream
cargo run --example gemini_generate
cargo run --example gemini_stream
cargo run --example custom_headers
cargo run --example multi_provider
cargo run --example provider_config

# With telemetry feature
cargo run --example telemetry_basic --features telemetry
```

## OpenTelemetry Instrumentation

StakAI includes optional OpenTelemetry instrumentation following GenAI semantic conventions.

### Enabling Telemetry

Add the `telemetry` feature to your `Cargo.toml`:

```toml
[dependencies]
stakai = { version = "0.1", features = ["telemetry"] }
```

### Basic Usage

```rust
use stakai::{Inference, GenerateRequest, Message, Role};
use stakai::telemetry::TelemetrySettings;

let client = Inference::new();

// Enable telemetry
let telemetry = TelemetrySettings::enabled()
    .with_function_id("my-chat-handler");

let request = GenerateRequest::new(
    "gpt-4",
    vec![Message::new(Role::User, "Hello!")],
);

// Generate with telemetry
let response = client.generate_with_telemetry(&request, &telemetry).await?;
```

### With Axiom Adapter

```rust
use stakai::telemetry::{TelemetrySettings, adapters::AxiomAdapter};

let telemetry = TelemetrySettings::enabled()
    .with_function_id("support-handler")
    .with_metadata("user_id", "user-123")
    .with_adapter(
        AxiomAdapter::new()
            .with_capability("customer_support")
            .with_step("initial_response")
    );

let response = client.generate_with_telemetry(&request, &telemetry).await?;
```

### Span Attributes

The telemetry follows OpenTelemetry GenAI semantic conventions:

| Attribute | Description |
|-----------|-------------|
| `gen_ai.operation.name` | Operation type ("chat") |
| `gen_ai.system` | Provider ("openai", "anthropic", etc.) |
| `gen_ai.request.model` | Requested model |
| `gen_ai.usage.input_tokens` | Input token count |
| `gen_ai.usage.output_tokens` | Output token count |
| `gen_ai.response.finish_reasons` | Why generation stopped |
| `gen_ai.input.messages` | Input messages (JSON) |
| `gen_ai.output.messages` | Output content (JSON) |

See the [OpenTelemetry GenAI conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/) for full details.

### Custom Adapters

Implement `TelemetryAdapter` to add custom attributes for your observability backend:

```rust
use stakai::telemetry::TelemetryAdapter;
use opentelemetry::KeyValue;

struct MyAdapter {
    env: String,
}

impl TelemetryAdapter for MyAdapter {
    fn enrich_attributes(&self, attributes: &mut Vec<KeyValue>) {
        attributes.push(KeyValue::new("deployment.environment", self.env.clone()));
    }
}
```

## Architecture

The SDK uses a provider-agnostic design:

```
Client API → Provider Registry → Provider Trait → OpenAI/Anthropic/etc.
```

- **Client**: High-level ergonomic API
- **Registry**: Runtime provider management
- **Provider Trait**: Unified interface for all providers
- **Providers**: Concrete implementations (OpenAI, Anthropic, etc.)

## Roadmap

### Completed ✅

- [x] OpenAI provider with full support
- [x] Anthropic provider (Claude) with full support
- [x] Google Gemini provider with full support
- [x] Streaming support for all providers
- [x] Tool/function calling for all providers
- [x] Multi-modal support (vision/images)
- [x] Extended thinking/reasoning support (Anthropic)
- [x] Provider-specific options (Vercel AI SDK pattern)
- [x] Custom headers support
- [x] Auto-registration from environment
- [x] Unified error handling
- [x] Provider-specific configurations
- [x] OpenTelemetry instrumentation (GenAI semantic conventions)
- [x] Extensible telemetry adapters (Axiom)

### Planned 📋

- [ ] OpenAI reasoning effort support (o1/o3/o4 models)
- [ ] Gemini thinking config support
- [ ] Embeddings API
- [ ] Rate limiting & retries
- [ ] Response caching
- [ ] Prompt caching (Anthropic)
- [ ] Audio support
- [ ] Batch API support
- [ ] More providers (Cohere, Mistral, xAI, etc.)

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## About Stakpak

This SDK is built and maintained by [Stakpak](https://stakpak.dev) - DevOps automation and infrastructure management platform.

## License

MIT OR Apache-2.0

---

<div align="center">
Made with ❤️ by <a href="https://stakpak.dev">Stakpak</a>
</div>
