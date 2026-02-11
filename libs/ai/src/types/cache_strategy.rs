//! Smart caching strategy for optimizing LLM request costs and latency
//!
//! This module provides automatic cache breakpoint placement based on
//! provider best practices:
//!
//! - **Anthropic**: Up to 4 explicit breakpoints on tools, system, and messages
//! - **OpenAI**: Automatic caching with optional `prompt_cache_key` for routing
//! - **Google**: Implicit caching (no configuration needed)
//!
//! # Default Strategy (Anthropic)
//!
//! The default `CacheStrategy::Auto` applies:
//! 1. Cache on **last tool** (caches all tools as a group)
//! 2. Cache on **last system message** block
//! 3. Cache on **last 2 non-system messages** (with remaining budget)
//!
//! This maximizes cache hit rates while staying within the 4 breakpoint limit.
//!
//! # Example
//!
//! ```rust
//! use stakai::{CacheStrategy, GenerateOptions};
//!
//! // Use automatic caching (recommended - this is the default)
//! let options = GenerateOptions::default();
//!
//! // Disable caching for a specific request
//! let options = GenerateOptions::default()
//!     .with_cache_strategy(CacheStrategy::None);
//!
//! // Custom Anthropic configuration: only cache system, 3 tail messages
//! let options = GenerateOptions::default()
//!     .with_cache_strategy(CacheStrategy::anthropic(false, true, 3));
//! ```

use serde::{Deserialize, Serialize};

/// Default placeholder string used to replace trimmed message content during
/// context budget reduction.
///
/// When a context manager trims old messages to stay within the context window,
/// it replaces their text content with this placeholder. The caching layer can
/// then detect these placeholders via [`AnthropicCacheConfig::trim_boundary_hint`]
/// to place a cache breakpoint at the trim boundary, anchoring the stable
/// trimmed prefix for cache reuse.
///
/// This constant is the **single source of truth** — used by context managers
/// for trimming and by the caching layer for trim boundary detection.
pub const TRIMMED_CONTENT_PLACEHOLDER: &str = "[trimmed]";

/// Caching strategy configuration
///
/// Controls how cache breakpoints are applied to requests.
/// Different providers have different caching mechanisms:
///
/// - **Anthropic**: Explicit breakpoints (max 4), applied to tools/system/messages
/// - **OpenAI**: Automatic caching with optional `prompt_cache_key` for routing
/// - **Google**: Implicit caching (no configuration needed)
///
/// # Provider vs Request Configuration
///
/// Cache strategy can be configured at two levels:
/// 1. **Provider level**: Set in provider config as a default
/// 2. **Request level**: Override via `GenerateOptions::with_cache_strategy()`
///
/// Request-level configuration takes precedence over provider defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CacheStrategy {
    /// Automatic caching optimized for the provider (default)
    ///
    /// - **Anthropic**: Caches last tool + last system + last 2 messages
    /// - **OpenAI**: Uses session_id as prompt_cache_key if provided
    /// - **Google**: No-op (implicit caching)
    #[default]
    Auto,

    /// Custom Anthropic-style caching configuration
    Anthropic(AnthropicCacheConfig),

    /// Disable automatic caching
    ///
    /// Note: For OpenAI, caching cannot be fully disabled - it's always on.
    /// This just means we don't set `prompt_cache_key`.
    None,
}

/// Anthropic-specific cache configuration
///
/// Controls which components receive cache breakpoints.
/// Anthropic allows up to 4 breakpoints per request.
///
/// # Breakpoint Budget Allocation
///
/// | Component | Breakpoints | Notes |
/// |-----------|-------------|-------|
/// | Last tool | 1 | Caches ALL tools (they form a prefix) |
/// | Last system | 1 | Caches ALL system messages |
/// | Trim boundary | 0–1 | Anchors stable trimmed prefix (when hint is set) |
/// | Tail messages | 1–2 | Last N non-system messages (reduced by 1 when trim boundary is used) |
///
/// Total: 4 breakpoints (Anthropic's maximum)
///
/// # Trim Boundary Hint
///
/// When `trim_boundary_hint` is set, the caching layer scans the final
/// (post-merge, post-sanitize) message array for the **last** message whose
/// text content consists entirely of the hint string. A cache breakpoint is
/// placed on that message, anchoring the stable trimmed prefix so it can be
/// read from cache on subsequent calls instead of re-written.
///
/// This consumes one breakpoint from the tail budget. When no message matches
/// the hint, the breakpoint is not used and the full tail budget is preserved.
///
/// # Example
///
/// ```rust
/// use stakai::AnthropicCacheConfig;
///
/// // Default: cache tools, system, and 2 tail messages
/// let config = AnthropicCacheConfig::default();
///
/// // Custom: only cache system and 3 tail messages
/// let config = AnthropicCacheConfig {
///     cache_tools: false,
///     cache_system: true,
///     tail_message_count: 3,
///     ..Default::default()
/// };
///
/// // With trim boundary hint: anchor trimmed prefix for cache reuse
/// let config = AnthropicCacheConfig {
///     trim_boundary_hint: Some("[trimmed]".to_string()),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnthropicCacheConfig {
    /// Cache the last tool definition (caches all tools as a group)
    ///
    /// Default: `true`
    #[serde(default = "default_true")]
    pub cache_tools: bool,

    /// Cache the last system message block
    ///
    /// Default: `true`
    #[serde(default = "default_true")]
    pub cache_system: bool,

    /// Number of tail messages to cache (from end, non-system)
    ///
    /// Default: `2` (uses remaining budget after tools/system)
    #[serde(default = "default_tail_count")]
    pub tail_message_count: usize,

    /// Hint string for detecting the trim boundary in the message array.
    ///
    /// When set, the caching layer scans the final message array (after merge
    /// and sanitization) for the last message whose text content consists
    /// entirely of this string. A cache breakpoint is placed on that message
    /// to anchor the stable trimmed prefix for cache reuse.
    ///
    /// This consumes one breakpoint from the tail budget. If no message
    /// matches, the hint is ignored and the full tail budget is used.
    ///
    /// Default: `None` (no trim boundary detection)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim_boundary_hint: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_tail_count() -> usize {
    2
}

impl Default for AnthropicCacheConfig {
    fn default() -> Self {
        Self {
            cache_tools: true,
            cache_system: true,
            tail_message_count: 2,
            trim_boundary_hint: None,
        }
    }
}

impl CacheStrategy {
    /// Create automatic caching strategy (provider-optimized)
    pub fn auto() -> Self {
        Self::Auto
    }

    /// Create custom Anthropic caching configuration
    ///
    /// # Arguments
    ///
    /// * `cache_tools` - Whether to cache the last tool definition
    /// * `cache_system` - Whether to cache the last system message
    /// * `tail_count` - Number of tail messages to cache
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::CacheStrategy;
    ///
    /// // Cache only system prompts and 2 tail messages (no tools)
    /// let strategy = CacheStrategy::anthropic(false, true, 2);
    /// ```
    pub fn anthropic(cache_tools: bool, cache_system: bool, tail_count: usize) -> Self {
        Self::Anthropic(AnthropicCacheConfig {
            cache_tools,
            cache_system,
            tail_message_count: tail_count,
            trim_boundary_hint: None,
        })
    }

    /// Disable automatic caching
    pub fn none() -> Self {
        Self::None
    }

    /// Set the trim boundary hint on this strategy.
    ///
    /// When set, the Anthropic caching layer will scan the final message array
    /// for the last message whose text content consists entirely of this string,
    /// and place a cache breakpoint there to anchor the stable trimmed prefix.
    ///
    /// Only applies to `Auto` and `Anthropic` variants. No-op for `None`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::CacheStrategy;
    ///
    /// let strategy = CacheStrategy::auto()
    ///     .with_trim_boundary_hint("[trimmed]");
    /// ```
    pub fn with_trim_boundary_hint(self, hint: impl Into<String>) -> Self {
        match self {
            Self::Auto => Self::Anthropic(AnthropicCacheConfig {
                trim_boundary_hint: Some(hint.into()),
                ..AnthropicCacheConfig::default()
            }),
            Self::Anthropic(mut config) => {
                config.trim_boundary_hint = Some(hint.into());
                Self::Anthropic(config)
            }
            Self::None => Self::None,
        }
    }

    /// Check if caching is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Get the effective Anthropic config for this strategy
    ///
    /// Returns `Some(config)` for `Auto` and `Anthropic` variants,
    /// `None` for `None` variant.
    pub fn to_anthropic_config(&self) -> Option<AnthropicCacheConfig> {
        match self {
            Self::Auto => Some(AnthropicCacheConfig::default()),
            Self::Anthropic(config) => Some(config.clone()),
            Self::None => None,
        }
    }

    /// Calculate the maximum number of breakpoints this config could use
    ///
    /// Useful for debugging and validation.
    pub fn max_breakpoint_count(&self, has_tools: bool, has_system: bool) -> usize {
        match self.to_anthropic_config() {
            Some(config) => {
                let mut count = 0;
                if config.cache_tools && has_tools {
                    count += 1;
                }
                if config.cache_system && has_system {
                    count += 1;
                }
                if config.trim_boundary_hint.is_some() {
                    count += 1;
                }
                count += config.tail_message_count;
                count.min(4) // Anthropic max
            }
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_strategy_is_auto() {
        assert_eq!(CacheStrategy::default(), CacheStrategy::Auto);
    }

    #[test]
    fn test_auto_returns_default_anthropic_config() {
        let config = CacheStrategy::Auto.to_anthropic_config().unwrap();
        assert!(config.cache_tools);
        assert!(config.cache_system);
        assert_eq!(config.tail_message_count, 2);
    }

    #[test]
    fn test_none_returns_no_config() {
        assert!(CacheStrategy::None.to_anthropic_config().is_none());
    }

    #[test]
    fn test_custom_anthropic_config() {
        let strategy = CacheStrategy::anthropic(false, true, 3);
        let config = strategy.to_anthropic_config().unwrap();
        assert!(!config.cache_tools);
        assert!(config.cache_system);
        assert_eq!(config.tail_message_count, 3);
    }

    #[test]
    fn test_max_breakpoint_count() {
        // Auto with tools and system: 1 + 1 + 2 = 4
        assert_eq!(CacheStrategy::Auto.max_breakpoint_count(true, true), 4);

        // Auto without tools: 0 + 1 + 2 = 3
        assert_eq!(CacheStrategy::Auto.max_breakpoint_count(false, true), 3);

        // None: always 0
        assert_eq!(CacheStrategy::None.max_breakpoint_count(true, true), 0);

        // Custom with high tail count: capped at 4
        let custom = CacheStrategy::anthropic(false, true, 5);
        assert_eq!(custom.max_breakpoint_count(true, true), 4);
    }

    #[test]
    fn test_is_enabled() {
        assert!(CacheStrategy::Auto.is_enabled());
        assert!(CacheStrategy::anthropic(true, true, 2).is_enabled());
        assert!(!CacheStrategy::None.is_enabled());
    }

    #[test]
    fn test_serialization_auto() {
        let strategy = CacheStrategy::Auto;
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("auto"));

        let deserialized: CacheStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, CacheStrategy::Auto);
    }

    #[test]
    fn test_serialization_anthropic() {
        let strategy = CacheStrategy::anthropic(true, false, 1);
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("anthropic"));
        assert!(json.contains("cache_tools"));

        let deserialized: CacheStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, strategy);
    }

    #[test]
    fn test_serialization_none() {
        let strategy = CacheStrategy::None;
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("none"));

        let deserialized: CacheStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, CacheStrategy::None);
    }

    #[test]
    fn test_anthropic_cache_config_default() {
        let config = AnthropicCacheConfig::default();
        assert!(config.cache_tools);
        assert!(config.cache_system);
        assert_eq!(config.tail_message_count, 2);
        assert!(config.trim_boundary_hint.is_none());
    }

    #[test]
    fn test_with_trim_boundary_hint_on_auto() {
        let strategy = CacheStrategy::auto().with_trim_boundary_hint("[trimmed]");
        let config = strategy.to_anthropic_config().unwrap();
        assert_eq!(config.trim_boundary_hint.as_deref(), Some("[trimmed]"));
        // Should preserve Auto defaults
        assert!(config.cache_tools);
        assert!(config.cache_system);
        assert_eq!(config.tail_message_count, 2);
    }

    #[test]
    fn test_with_trim_boundary_hint_on_custom() {
        let strategy =
            CacheStrategy::anthropic(false, true, 3).with_trim_boundary_hint("[redacted]");
        let config = strategy.to_anthropic_config().unwrap();
        assert_eq!(config.trim_boundary_hint.as_deref(), Some("[redacted]"));
        // Should preserve custom settings
        assert!(!config.cache_tools);
        assert!(config.cache_system);
        assert_eq!(config.tail_message_count, 3);
    }

    #[test]
    fn test_with_trim_boundary_hint_on_none_is_noop() {
        let strategy = CacheStrategy::none().with_trim_boundary_hint("[trimmed]");
        assert_eq!(strategy, CacheStrategy::None);
        assert!(strategy.to_anthropic_config().is_none());
    }

    #[test]
    fn test_max_breakpoint_count_with_trim_hint() {
        // Auto + trim hint: 1 (tools) + 1 (system) + 1 (trim) + 2 (tail) = 5, capped at 4
        let strategy = CacheStrategy::auto().with_trim_boundary_hint("[trimmed]");
        assert_eq!(strategy.max_breakpoint_count(true, true), 4);

        // No tools + trim hint: 0 + 1 (system) + 1 (trim) + 2 (tail) = 4
        assert_eq!(strategy.max_breakpoint_count(false, true), 4);
    }

    #[test]
    fn test_serialization_with_trim_boundary_hint() {
        let strategy = CacheStrategy::auto().with_trim_boundary_hint("[trimmed]");
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("trim_boundary_hint"));
        assert!(json.contains("[trimmed]"));

        let deserialized: CacheStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, strategy);
    }

    #[test]
    fn test_deserialization_without_trim_boundary_hint() {
        // Existing configs without the field should deserialize with None
        let json =
            r#"{"type":"anthropic","cache_tools":true,"cache_system":true,"tail_message_count":2}"#;
        let strategy: CacheStrategy = serde_json::from_str(json).unwrap();
        let config = strategy.to_anthropic_config().unwrap();
        assert!(config.trim_boundary_hint.is_none());
    }
}
