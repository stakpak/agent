//! OAuth provider implementations

mod anthropic;
mod github_copilot;
mod stakpak;

pub use anthropic::AnthropicProvider;
pub use github_copilot::GitHubCopilotProvider;
pub use stakpak::StakpakProvider;
