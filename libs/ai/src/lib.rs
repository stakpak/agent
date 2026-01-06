//! # AI SDK
//!
//! A provider-agnostic Rust SDK for AI completions with streaming support.
//!
//! Built by [Stakpak](https://stakpak.dev) 🚀
//!
//! ## Features
//!
//! - **Provider-agnostic**: Unified interface for multiple AI providers (OpenAI, Anthropic, etc.)
//! - **Streaming support**: Real-time streaming responses with unified event types
//! - **Type-safe**: Strong typing with compile-time guarantees
//! - **Zero-cost abstractions**: Static dispatch for optimal performance
//! - **Ergonomic API**: Builder patterns and intuitive interfaces
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use stakai::{Inference, GenerateRequest, Message, Role};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Inference::new();
//!     
//!     let request = GenerateRequest::new(
//!         "gpt-4",
//!         vec![Message::new(Role::User, "What is Rust?")]
//!     );
//!     
//!     let response = client.generate(&request).await?;
//!     println!("Response: {}", response.text());
//!     
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod error;
pub mod provider;
pub mod providers;
pub mod registry;
pub mod types;

// Re-export commonly used types
pub use client::{Inference, InferenceConfig};
pub use error::{Error, Result};
pub use types::{
    // Cache control types
    CacheContext, CacheControl, CacheControlValidator, CacheWarning, CacheWarningType,
    PromptCacheRetention,
    // Message types
    AnthropicContentPartOptions, AnthropicMessageOptions, ContentPart, ContentPartProviderOptions,
    ImageDetail, Message, MessageContent, MessageProviderOptions, Role,
    // Options types
    AnthropicToolOptions, GenerateOptions, Tool, ToolChoice, ToolFunction, ToolProviderOptions,
    // Request types
    AnthropicOptions, GenerateRequest, GoogleOptions, Headers, OpenAIOptions, ProviderOptions,
    ReasoningEffort, ReasoningSummary, SystemMessageMode, ThinkingOptions,
    // Response types
    FinishReason, FinishReasonKind, GenerateResponse, GenerateStream, InputTokenDetails,
    OutputTokenDetails, ResponseContent, ResponseWarning, StreamEvent, ToolCall, Usage,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::client::Inference;
    pub use crate::error::{Error, Result};
    pub use crate::provider::Provider;
    pub use crate::types::*;
}
