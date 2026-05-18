//! OAuth provider implementations

mod anthropic;
mod gemini;
mod github_copilot;
mod openai_codex;
mod openrouter;
mod stakpak;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use github_copilot::GitHubCopilotProvider;
pub use openai_codex::OpenAICodexProvider;
pub use openrouter::OpenRouterProvider;
pub use stakpak::StakpakProvider;
