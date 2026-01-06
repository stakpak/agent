//! OAuth 2.0 authentication support for LLM providers
//!
//! This module provides OAuth authentication support for various LLM providers,
//! starting with Anthropic Claude Pro/Max subscriptions.
//!
//! # Architecture
//!
//! - `config`: OAuth configuration types
//! - `error`: Error types for OAuth operations
//! - `flow`: OAuth 2.0 authorization code flow implementation
//! - `pkce`: PKCE (Proof Key for Code Exchange) implementation
//! - `provider`: OAuth provider trait and types
//! - `providers`: Concrete provider implementations
//! - `registry`: Provider registry for managing providers
//!
//! # Example
//!
//! ```rust,ignore
//! use stakpak_shared::oauth::{OAuthFlow, ProviderRegistry};
//!
//! // Get the Anthropic provider
//! let registry = ProviderRegistry::new();
//! let provider = registry.get("anthropic").unwrap();
//!
//! // Get OAuth config for Claude Pro/Max
//! let config = provider.oauth_config("claude-max").unwrap();
//!
//! // Start the OAuth flow
//! let mut flow = OAuthFlow::new(config);
//! let auth_url = flow.generate_auth_url();
//!
//! // User visits auth_url, gets code, then:
//! // let tokens = flow.exchange_code(code).await?;
//! // let auth = provider.post_authorize("claude-max", &tokens).await?;
//! ```

pub mod config;
pub mod error;
pub mod flow;
pub mod pkce;
pub mod provider;
pub mod providers;
pub mod registry;

// Re-export commonly used types
pub use config::OAuthConfig;
pub use error::{OAuthError, OAuthResult};
pub use flow::{OAuthFlow, TokenResponse};
pub use pkce::PkceChallenge;
pub use provider::{AuthMethod, AuthMethodType, OAuthProvider};
pub use providers::AnthropicProvider;
pub use registry::ProviderRegistry;
