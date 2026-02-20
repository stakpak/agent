//! AWS Bedrock provider module
//!
//! Provides access to Anthropic Claude models through AWS Bedrock's InvokeModel API.
//! Authentication is handled entirely by the AWS credential chain — no API keys needed.
//!
//! # Features
//!
//! - Full feature parity with the direct Anthropic provider: streaming, tool use,
//!   prompt caching, and extended thinking
//! - Uses `aws-sdk-bedrockruntime` for SigV4 auth and EventStream transport
//! - Reuses the Anthropic conversion layer — no duplicated message/tool logic

mod convert;
pub mod models;
mod provider;
mod stream;
mod types;

pub use provider::BedrockProvider;
pub use types::BedrockConfig;
