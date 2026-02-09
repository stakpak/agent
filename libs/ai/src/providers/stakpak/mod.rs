//! Stakpak provider implementation
//!
//! Stakpak provides an OpenAI-compatible API at `/v1/chat/completions`.
//! This provider routes inference requests through Stakpak's infrastructure.

mod convert;
mod provider;
mod stream;
mod types;

pub use provider::StakpakProvider;
pub use types::StakpakProviderConfig;
