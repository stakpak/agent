//! Cache control validation for Anthropic-style prompt caching
//!
//! This module provides validation similar to Vercel AI SDK's `CacheControlValidator`.
//! It enforces provider-specific limits like Anthropic's 4 cache breakpoint maximum.

use super::cache::{CacheControl, CacheWarning};

/// Context information for cache validation
///
/// Describes where cache control is being applied and whether
/// that context supports caching.
#[derive(Debug, Clone)]
pub struct CacheContext {
    /// Type name for error messages (e.g., "system message", "tool")
    pub type_name: &'static str,

    /// Whether this context supports caching
    pub can_cache: bool,
}

impl CacheContext {
    /// Create a custom cache context
    pub fn new(type_name: &'static str, can_cache: bool) -> Self {
        Self {
            type_name,
            can_cache,
        }
    }

    /// System message context (cacheable)
    pub fn system_message() -> Self {
        Self {
            type_name: "system message",
            can_cache: true,
        }
    }

    /// User message context (cacheable)
    pub fn user_message() -> Self {
        Self {
            type_name: "user message",
            can_cache: true,
        }
    }

    /// User message part/content block context (cacheable)
    pub fn user_message_part() -> Self {
        Self {
            type_name: "user message part",
            can_cache: true,
        }
    }

    /// Assistant message context (cacheable)
    pub fn assistant_message() -> Self {
        Self {
            type_name: "assistant message",
            can_cache: true,
        }
    }

    /// Assistant message part context (cacheable)
    pub fn assistant_message_part() -> Self {
        Self {
            type_name: "assistant message part",
            can_cache: true,
        }
    }

    /// Tool result context (cacheable)
    pub fn tool_result() -> Self {
        Self {
            type_name: "tool result",
            can_cache: true,
        }
    }

    /// Tool result part context (cacheable)
    pub fn tool_result_part() -> Self {
        Self {
            type_name: "tool result part",
            can_cache: true,
        }
    }

    /// Tool definition context (cacheable)
    pub fn tool_definition() -> Self {
        Self {
            type_name: "tool definition",
            can_cache: true,
        }
    }

    /// Thinking/reasoning block context (NOT cacheable)
    ///
    /// Thinking blocks cannot have cache_control directly set.
    /// They are cached implicitly when in previous assistant turns.
    pub fn thinking_block() -> Self {
        Self {
            type_name: "thinking block",
            can_cache: false,
        }
    }

    /// Redacted thinking block context (NOT cacheable)
    pub fn redacted_thinking_block() -> Self {
        Self {
            type_name: "redacted thinking block",
            can_cache: false,
        }
    }

    /// Image content context (cacheable)
    pub fn image_content() -> Self {
        Self {
            type_name: "image content",
            can_cache: true,
        }
    }

    /// Document/file content context (cacheable)
    pub fn document_content() -> Self {
        Self {
            type_name: "document content",
            can_cache: true,
        }
    }
}

/// Validator for cache control breakpoints
///
/// Anthropic allows a maximum of 4 cache breakpoints per request.
/// This validator tracks breakpoints and generates warnings when limits are exceeded.
///
/// # Example
///
/// ```rust
/// use stakai::types::{CacheControlValidator, CacheContext, CacheControl};
///
/// let mut validator = CacheControlValidator::new();
///
/// // Validate cache control on a user message
/// let cache = CacheControl::ephemeral();
/// let validated = validator.validate(Some(&cache), CacheContext::user_message());
///
/// // Check if validation passed
/// if validated.is_some() {
///     println!("Cache control accepted");
/// }
///
/// // Check for warnings after processing all content
/// for warning in validator.warnings() {
///     eprintln!("Warning: {}", warning);
/// }
/// ```
#[derive(Debug)]
pub struct CacheControlValidator {
    /// Current count of cache breakpoints
    breakpoint_count: usize,

    /// Accumulated validation warnings
    warnings: Vec<CacheWarning>,
}

impl CacheControlValidator {
    /// Maximum cache breakpoints allowed by Anthropic
    pub const MAX_BREAKPOINTS: usize = 4;

    /// Create a new cache control validator
    pub fn new() -> Self {
        Self {
            breakpoint_count: 0,
            warnings: Vec::new(),
        }
    }

    /// Validate cache control for a given context
    ///
    /// Returns `Some(CacheControl)` if the cache control is valid for the context,
    /// or `None` if it was rejected (with a warning added).
    ///
    /// # Arguments
    ///
    /// * `cache_control` - The cache control to validate (None is always valid)
    /// * `context` - The context where cache control is being applied
    ///
    /// # Returns
    ///
    /// The validated cache control, or None if rejected
    pub fn validate(
        &mut self,
        cache_control: Option<&CacheControl>,
        context: CacheContext,
    ) -> Option<CacheControl> {
        // No cache control specified - nothing to validate
        let cache_control = cache_control?;

        // Check if caching is allowed in this context
        if !context.can_cache {
            self.warnings
                .push(CacheWarning::unsupported_context(context.type_name));
            return None;
        }

        // Check breakpoint limit
        self.breakpoint_count += 1;
        if self.breakpoint_count > Self::MAX_BREAKPOINTS {
            self.warnings.push(CacheWarning::breakpoint_limit_exceeded(
                self.breakpoint_count,
                Self::MAX_BREAKPOINTS,
            ));
            return None;
        }

        Some(cache_control.clone())
    }

    /// Validate cache control, taking ownership of part-level and falling back to message-level
    ///
    /// This implements the priority: part-level cache control takes precedence over message-level.
    /// Only the first non-None value is used.
    ///
    /// # Arguments
    ///
    /// * `part_cache` - Cache control from the content part (higher priority)
    /// * `message_cache` - Cache control from the message (fallback)
    /// * `context` - The context for validation
    ///
    /// # Returns
    ///
    /// The validated cache control, or None if both were None or rejected
    pub fn validate_with_fallback(
        &mut self,
        part_cache: Option<&CacheControl>,
        message_cache: Option<&CacheControl>,
        context: CacheContext,
    ) -> Option<CacheControl> {
        // Part-level cache control takes precedence
        if part_cache.is_some() {
            return self.validate(part_cache, context);
        }

        // Fall back to message-level cache control
        self.validate(message_cache, context)
    }

    /// Get all accumulated warnings
    pub fn warnings(&self) -> &[CacheWarning] {
        &self.warnings
    }

    /// Take ownership of warnings, leaving the validator with an empty warning list
    pub fn take_warnings(&mut self) -> Vec<CacheWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// Check if any warnings were generated
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Get current breakpoint count
    pub fn breakpoint_count(&self) -> usize {
        self.breakpoint_count
    }

    /// Check if the breakpoint limit has been reached
    pub fn is_at_limit(&self) -> bool {
        self.breakpoint_count >= Self::MAX_BREAKPOINTS
    }

    /// Get remaining available breakpoints
    pub fn remaining_breakpoints(&self) -> usize {
        Self::MAX_BREAKPOINTS.saturating_sub(self.breakpoint_count)
    }

    /// Reset the validator for reuse
    pub fn reset(&mut self) {
        self.breakpoint_count = 0;
        self.warnings.clear();
    }
}

impl Default for CacheControlValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_allows_up_to_4_breakpoints() {
        let mut validator = CacheControlValidator::new();
        let cache = CacheControl::ephemeral();

        // First 4 breakpoints should be allowed
        for i in 1..=4 {
            let result = validator.validate(Some(&cache), CacheContext::user_message());
            assert!(result.is_some(), "Breakpoint {} should be allowed", i);
            assert_eq!(validator.breakpoint_count(), i);
        }

        assert!(validator.is_at_limit());
        assert_eq!(validator.remaining_breakpoints(), 0);

        // 5th breakpoint should be rejected
        let result = validator.validate(Some(&cache), CacheContext::user_message());
        assert!(result.is_none(), "5th breakpoint should be rejected");
        assert_eq!(validator.warnings().len(), 1);
    }

    #[test]
    fn test_validator_rejects_non_cacheable_context() {
        let mut validator = CacheControlValidator::new();
        let cache = CacheControl::ephemeral();

        let result = validator.validate(Some(&cache), CacheContext::thinking_block());
        assert!(result.is_none());
        assert_eq!(validator.warnings().len(), 1);

        let warning = &validator.warnings()[0];
        assert_eq!(
            warning.warning_type,
            super::super::cache::CacheWarningType::UnsupportedContext
        );
        assert!(warning.message.contains("thinking block"));
    }

    #[test]
    fn test_validator_passes_through_none() {
        let mut validator = CacheControlValidator::new();

        let result = validator.validate(None, CacheContext::user_message());
        assert!(result.is_none());
        assert_eq!(validator.breakpoint_count(), 0);
        assert!(!validator.has_warnings());
    }

    #[test]
    fn test_validate_with_fallback() {
        let mut validator = CacheControlValidator::new();
        let part_cache = CacheControl::ephemeral_with_ttl("1h");
        let msg_cache = CacheControl::ephemeral();

        // Part-level takes precedence
        let result = validator.validate_with_fallback(
            Some(&part_cache),
            Some(&msg_cache),
            CacheContext::user_message_part(),
        );
        assert_eq!(result, Some(part_cache.clone()));

        // Falls back to message-level when part is None
        let result = validator.validate_with_fallback(
            None,
            Some(&msg_cache),
            CacheContext::user_message_part(),
        );
        assert_eq!(result, Some(msg_cache.clone()));
    }

    #[test]
    fn test_validator_reset() {
        let mut validator = CacheControlValidator::new();
        let cache = CacheControl::ephemeral();

        // Add some breakpoints and a warning
        for _ in 0..5 {
            validator.validate(Some(&cache), CacheContext::user_message());
        }
        assert_eq!(validator.breakpoint_count(), 5);
        assert!(validator.has_warnings());

        // Reset
        validator.reset();
        assert_eq!(validator.breakpoint_count(), 0);
        assert!(!validator.has_warnings());
    }

    #[test]
    fn test_take_warnings() {
        let mut validator = CacheControlValidator::new();
        let cache = CacheControl::ephemeral();

        // Generate a warning by exceeding the limit
        for _ in 0..5 {
            validator.validate(Some(&cache), CacheContext::user_message());
        }

        let warnings = validator.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(validator.warnings().is_empty());
    }

    #[test]
    fn test_all_cacheable_contexts() {
        let cacheable_contexts = vec![
            CacheContext::system_message(),
            CacheContext::user_message(),
            CacheContext::user_message_part(),
            CacheContext::assistant_message(),
            CacheContext::assistant_message_part(),
            CacheContext::tool_result(),
            CacheContext::tool_result_part(),
            CacheContext::tool_definition(),
            CacheContext::image_content(),
            CacheContext::document_content(),
        ];

        for context in cacheable_contexts {
            assert!(
                context.can_cache,
                "{} should be cacheable",
                context.type_name
            );
        }
    }

    #[test]
    fn test_non_cacheable_contexts() {
        let non_cacheable_contexts = vec![
            CacheContext::thinking_block(),
            CacheContext::redacted_thinking_block(),
        ];

        for context in non_cacheable_contexts {
            assert!(
                !context.can_cache,
                "{} should not be cacheable",
                context.type_name
            );
        }
    }
}
