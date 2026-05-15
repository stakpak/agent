//! MiniMax provider implementation
//!
//! MiniMax provides an OpenAI-compatible API at `https://api.m/minimax.io/v1`.
//! This provider supports MiniMax-M2.7, MiniMax-M2.7-highspeed, MiniMax-M2.5,
//! and MiniMax-M2.5-highspeed models.

mod convert;
mod provider;
mod stream;
mod types;

pub use provider::MiniMaxProvider;
pub use types::MiniMaxConfig;
