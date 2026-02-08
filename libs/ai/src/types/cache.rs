//! Cache control types for provider-level prompt caching
//!
//! This module provides unified cache control types that work across providers:
//! - **Anthropic**: Explicit cache breakpoints with `CacheControl::Ephemeral`
//! - **OpenAI**: Automatic caching with optional `prompt_cache_key` for better hit rates
//! - **Google**: Implicit caching (Gemini 2.5+) or explicit `cached_content` names

use serde::{Deserialize, Serialize};

/// Cache control configuration for content caching (Anthropic-style)
///
/// Used to mark specific content blocks, messages, or tools for caching.
/// Anthropic allows up to 4 cache breakpoints per request.
///
/// # Example
///
/// ```rust
/// use stakai::CacheControl;
///
/// // Default ephemeral cache (~5 min TTL)
/// let cache = CacheControl::ephemeral();
///
/// // Extended cache with 1-hour TTL
/// let cache = CacheControl::ephemeral_with_ttl("1h");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CacheControl {
    /// Ephemeral cache that persists for a limited time
    ///
    /// Default TTL is approximately 5 minutes. Use `ttl` to specify
    /// a longer duration (e.g., "1h" for 1-hour cache on Anthropic).
    Ephemeral {
        /// Optional TTL duration (e.g., "1h" for 1-hour cache)
        #[serde(skip_serializing_if = "Option::is_none")]
        ttl: Option<String>,
    },
}

impl CacheControl {
    /// Create ephemeral cache control with default TTL (~5 minutes)
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::CacheControl;
    ///
    /// let cache = CacheControl::ephemeral();
    /// ```
    pub fn ephemeral() -> Self {
        Self::Ephemeral { ttl: None }
    }

    /// Create ephemeral cache control with custom TTL
    ///
    /// # Arguments
    ///
    /// * `ttl` - Time-to-live duration (e.g., "1h" for 1 hour)
    ///
    /// # Example
    ///
    /// ```rust
    /// use stakai::CacheControl;
    ///
    /// let cache = CacheControl::ephemeral_with_ttl("1h");
    /// ```
    pub fn ephemeral_with_ttl(ttl: impl Into<String>) -> Self {
        Self::Ephemeral {
            ttl: Some(ttl.into()),
        }
    }

    /// Check if this cache control has an explicit TTL
    pub fn has_ttl(&self) -> bool {
        match self {
            Self::Ephemeral { ttl } => ttl.is_some(),
        }
    }

    /// Get the TTL if specified
    pub fn ttl(&self) -> Option<&str> {
        match self {
            Self::Ephemeral { ttl } => ttl.as_deref(),
        }
    }
}

impl Default for CacheControl {
    fn default() -> Self {
        Self::ephemeral()
    }
}

/// OpenAI prompt cache retention policy
///
/// Controls how long cached prompts are retained by OpenAI.
///
/// # Example
///
/// ```rust
/// use stakai::PromptCacheRetention;
///
/// // Standard in-memory caching (default)
/// let retention = PromptCacheRetention::InMemory;
///
/// // Extended 24-hour caching (GPT-5.1+ only)
/// let retention = PromptCacheRetention::Extended24h;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PromptCacheRetention {
    /// Standard in-memory caching (default)
    ///
    /// Cached prompts may persist for 5-10 minutes during normal operation,
    /// or up to an hour during off-peak periods.
    #[default]
    InMemory,

    /// Extended 24-hour caching
    ///
    /// Keeps cached prefixes active for up to 24 hours.
    /// **Note**: Only available for GPT-5.1 series models.
    #[serde(rename = "24h")]
    Extended24h,
}

impl std::fmt::Display for PromptCacheRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InMemory => write!(f, "in_memory"),
            Self::Extended24h => write!(f, "24h"),
        }
    }
}

/// Type of cache validation warning
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheWarningType {
    /// Cache control was set on a context that doesn't support caching
    UnsupportedContext,

    /// Maximum cache breakpoints exceeded (Anthropic limits to 4)
    BreakpointLimitExceeded,

    /// Cache control is not supported by the target provider
    UnsupportedProvider,
}

impl std::fmt::Display for CacheWarningType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedContext => write!(f, "unsupported_context"),
            Self::BreakpointLimitExceeded => write!(f, "breakpoint_limit_exceeded"),
            Self::UnsupportedProvider => write!(f, "unsupported_provider"),
        }
    }
}

/// Warning generated during cache control validation
///
/// Warnings are non-fatal issues that occurred during request processing.
/// The request will still be sent, but some cache control directives may
/// have been ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheWarning {
    /// Type of warning
    pub warning_type: CacheWarningType,

    /// Human-readable warning message
    pub message: String,
}

impl CacheWarning {
    /// Create a new cache warning
    pub fn new(warning_type: CacheWarningType, message: impl Into<String>) -> Self {
        Self {
            warning_type,
            message: message.into(),
        }
    }

    /// Create an unsupported context warning
    pub fn unsupported_context(context_type: &str) -> Self {
        Self::new(
            CacheWarningType::UnsupportedContext,
            format!(
                "cache_control cannot be set on {}. It will be ignored.",
                context_type
            ),
        )
    }

    /// Create a breakpoint limit exceeded warning
    pub fn breakpoint_limit_exceeded(count: usize, max: usize) -> Self {
        Self::new(
            CacheWarningType::BreakpointLimitExceeded,
            format!(
                "Maximum {} cache breakpoints exceeded (found {}). This breakpoint will be ignored.",
                max, count
            ),
        )
    }

    /// Create an unsupported provider warning
    pub fn unsupported_provider(provider: &str) -> Self {
        Self::new(
            CacheWarningType::UnsupportedProvider,
            format!(
                "cache_control is not supported by provider '{}'. It will be ignored.",
                provider
            ),
        )
    }
}

impl std::fmt::Display for CacheWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.warning_type, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_control_ephemeral() {
        let cache = CacheControl::ephemeral();
        assert!(!cache.has_ttl());
        assert_eq!(cache.ttl(), None);
    }

    #[test]
    fn test_cache_control_with_ttl() {
        let cache = CacheControl::ephemeral_with_ttl("1h");
        assert!(cache.has_ttl());
        assert_eq!(cache.ttl(), Some("1h"));
    }

    #[test]
    fn test_cache_control_serialization() {
        let cache = CacheControl::ephemeral();
        let json = serde_json::to_string(&cache).unwrap();
        assert_eq!(json, r#"{"type":"ephemeral"}"#);

        let cache = CacheControl::ephemeral_with_ttl("1h");
        let json = serde_json::to_string(&cache).unwrap();
        assert_eq!(json, r#"{"type":"ephemeral","ttl":"1h"}"#);
    }

    #[test]
    fn test_prompt_cache_retention_serialization() {
        let retention = PromptCacheRetention::InMemory;
        let json = serde_json::to_string(&retention).unwrap();
        assert_eq!(json, r#""in_memory""#);

        let retention = PromptCacheRetention::Extended24h;
        let json = serde_json::to_string(&retention).unwrap();
        assert_eq!(json, r#""24h""#);
    }

    #[test]
    fn test_cache_warning_display() {
        let warning = CacheWarning::breakpoint_limit_exceeded(5, 4);
        let display = format!("{}", warning);
        assert!(display.contains("breakpoint_limit_exceeded"));
        assert!(display.contains("Maximum 4"));
    }
}
