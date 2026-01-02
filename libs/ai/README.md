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
