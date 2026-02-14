//! Bedrock model ID mapping
//!
//! Bedrock uses a different model ID format than direct Anthropic:
//! - Direct Anthropic: `claude-sonnet-4-5-20250929`
//! - Bedrock: `anthropic.claude-sonnet-4-5-20250929-v1:0`
//! - Cross-region: `us.anthropic.claude-sonnet-4-5-20250929-v1:0`
//!
//! This module accepts both Anthropic-style and Bedrock-style model IDs, mapping when needed.
//! - If the ID already looks like a Bedrock ID (contains `anthropic.`), pass through
//! - If the ID has a region prefix (`us.`, `eu.`, `global.`), pass through
//! - Otherwise, map from Anthropic-style to Bedrock format

use std::collections::HashMap;
use std::sync::LazyLock;

/// Known mappings from Anthropic model IDs to Bedrock model IDs
///
/// Uses cross-region inference profile IDs (e.g., `us.anthropic.`) by default,
/// since on-demand invocation of base model IDs is not supported in most regions.
/// Cross-region IDs work everywhere and automatically route to the optimal region.
///
/// This table covers the common models. Unknown IDs fall back to
/// `us.anthropic.{id}-v1:0` which works for most Anthropic models on Bedrock.
static ANTHROPIC_TO_BEDROCK: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // Claude 4.6
        ("claude-opus-4-6", "us.anthropic.claude-opus-4-6-v1"),
        // Claude 4.5
        (
            "claude-sonnet-4-5-20250929",
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
        ),
        (
            "claude-sonnet-4-5",
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
        ),
        (
            "claude-opus-4-5-20251101",
            "us.anthropic.claude-opus-4-5-20251101-v1:0",
        ),
        (
            "claude-opus-4-5",
            "us.anthropic.claude-opus-4-5-20251101-v1:0",
        ),
        (
            "claude-haiku-4-5-20251001",
            "us.anthropic.claude-haiku-4-5-20251001-v1:0",
        ),
        (
            "claude-haiku-4-5",
            "us.anthropic.claude-haiku-4-5-20251001-v1:0",
        ),
        // Claude 4.1
        (
            "claude-opus-4-1-20250805",
            "us.anthropic.claude-opus-4-1-20250805-v1:0",
        ),
        (
            "claude-opus-4-1",
            "us.anthropic.claude-opus-4-1-20250805-v1:0",
        ),
        // Claude 4
        (
            "claude-sonnet-4-20250514",
            "us.anthropic.claude-sonnet-4-20250514-v1:0",
        ),
        (
            "claude-sonnet-4-0",
            "us.anthropic.claude-sonnet-4-20250514-v1:0",
        ),
        (
            "claude-opus-4-20250514",
            "us.anthropic.claude-opus-4-20250514-v1:0",
        ),
        (
            "claude-opus-4-0",
            "us.anthropic.claude-opus-4-20250514-v1:0",
        ),
        // Claude 3.7
        (
            "claude-3-7-sonnet-20250219",
            "us.anthropic.claude-3-7-sonnet-20250219-v1:0",
        ),
        (
            "claude-3-7-sonnet-latest",
            "us.anthropic.claude-3-7-sonnet-20250219-v1:0",
        ),
        // Claude 3.5
        (
            "claude-3-5-sonnet-20241022",
            "us.anthropic.claude-3-5-sonnet-20241022-v2:0",
        ),
        (
            "claude-3-5-sonnet-20240620",
            "us.anthropic.claude-3-5-sonnet-20240620-v1:0",
        ),
        (
            "claude-3-5-haiku-20241022",
            "us.anthropic.claude-3-5-haiku-20241022-v1:0",
        ),
        (
            "claude-3-5-haiku-latest",
            "us.anthropic.claude-3-5-haiku-20241022-v1:0",
        ),
        // Claude 3
        (
            "claude-3-opus-20240229",
            "us.anthropic.claude-3-opus-20240229-v1:0",
        ),
        (
            "claude-3-sonnet-20240229",
            "us.anthropic.claude-3-sonnet-20240229-v1:0",
        ),
        (
            "claude-3-haiku-20240307",
            "us.anthropic.claude-3-haiku-20240307-v1:0",
        ),
    ])
});

/// Resolve a model ID to a Bedrock-compatible model ID
///
/// Accepts both Anthropic-style and Bedrock-style model IDs:
/// 1. If the ID already contains `anthropic.` → passthrough (already a Bedrock ID)
/// 2. If the ID has a region prefix (`us.`, `eu.`, `ap.`, `global.`) → passthrough
/// 3. If the ID is in the known mapping table → use the mapped value
/// 4. Otherwise → best-effort: `anthropic.{id}-v1:0`
///
/// # Examples
///
/// ```
/// use stakai::providers::bedrock::models::resolve_bedrock_model_id;
///
/// // Friendly alias → mapped to cross-region Bedrock format
/// assert_eq!(
///     resolve_bedrock_model_id("claude-sonnet-4-5"),
///     "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
/// );
///
/// // Anthropic-style with date → mapped to cross-region Bedrock format
/// assert_eq!(
///     resolve_bedrock_model_id("claude-sonnet-4-5-20250929"),
///     "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
/// );
///
/// // Already a Bedrock ID → passthrough
/// assert_eq!(
///     resolve_bedrock_model_id("anthropic.claude-sonnet-4-5-20250929-v1:0"),
///     "anthropic.claude-sonnet-4-5-20250929-v1:0"
/// );
///
/// // Cross-region ID → passthrough
/// assert_eq!(
///     resolve_bedrock_model_id("us.anthropic.claude-sonnet-4-5-20250929-v1:0"),
///     "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
/// );
/// ```
pub fn resolve_bedrock_model_id(model_id: &str) -> String {
    // 1. Already a Bedrock ID (contains "anthropic." anywhere)
    if model_id.contains("anthropic.") {
        return model_id.to_string();
    }

    // 2. Has a region prefix (e.g., "us.", "eu.", "ap.", "global.")
    // These are cross-region inference profile IDs
    if has_region_prefix(model_id) {
        return model_id.to_string();
    }

    // 3. Known mapping
    if let Some(&bedrock_id) = ANTHROPIC_TO_BEDROCK.get(model_id) {
        return bedrock_id.to_string();
    }

    // 4. Best-effort fallback: assume it's an Anthropic model ID
    // Use cross-region prefix for maximum compatibility
    format!("us.anthropic.{}-v1:0", model_id)
}

/// Check if a model ID has a region prefix (cross-region inference)
fn has_region_prefix(model_id: &str) -> bool {
    // AWS region prefixes used in cross-region inference profile IDs
    let prefixes = ["us.", "eu.", "ap.", "global."];
    prefixes.iter().any(|p| model_id.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_style_maps_to_bedrock() {
        assert_eq!(
            resolve_bedrock_model_id("claude-sonnet-4-5-20250929"),
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-3-5-haiku-20241022"),
            "us.anthropic.claude-3-5-haiku-20241022-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-3-opus-20240229"),
            "us.anthropic.claude-3-opus-20240229-v1:0"
        );
    }

    #[test]
    fn test_latest_aliases_map_correctly() {
        assert_eq!(
            resolve_bedrock_model_id("claude-sonnet-4-5"),
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-haiku-4-5"),
            "us.anthropic.claude-haiku-4-5-20251001-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-opus-4-0"),
            "us.anthropic.claude-opus-4-20250514-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-3-5-haiku-latest"),
            "us.anthropic.claude-3-5-haiku-20241022-v1:0"
        );
    }

    #[test]
    fn test_claude_4_family() {
        assert_eq!(
            resolve_bedrock_model_id("claude-sonnet-4-20250514"),
            "us.anthropic.claude-sonnet-4-20250514-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-opus-4-20250514"),
            "us.anthropic.claude-opus-4-20250514-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-opus-4-1-20250805"),
            "us.anthropic.claude-opus-4-1-20250805-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("claude-opus-4-5-20251101"),
            "us.anthropic.claude-opus-4-5-20251101-v1:0"
        );
    }

    #[test]
    fn test_v2_model_mapping() {
        // claude-3-5-sonnet-20241022 maps to v2:0 (not v1:0)
        assert_eq!(
            resolve_bedrock_model_id("claude-3-5-sonnet-20241022"),
            "us.anthropic.claude-3-5-sonnet-20241022-v2:0"
        );
    }

    #[test]
    fn test_bedrock_id_passthrough() {
        assert_eq!(
            resolve_bedrock_model_id("anthropic.claude-sonnet-4-5-20250929-v1:0"),
            "anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("anthropic.claude-3-5-haiku-20241022-v1:0"),
            "anthropic.claude-3-5-haiku-20241022-v1:0"
        );
    }

    #[test]
    fn test_cross_region_passthrough() {
        assert_eq!(
            resolve_bedrock_model_id("us.anthropic.claude-sonnet-4-5-20250929-v1:0"),
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("eu.anthropic.claude-3-5-sonnet-20241022-v2:0"),
            "eu.anthropic.claude-3-5-sonnet-20241022-v2:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("ap.anthropic.claude-3-haiku-20240307-v1:0"),
            "ap.anthropic.claude-3-haiku-20240307-v1:0"
        );
        assert_eq!(
            resolve_bedrock_model_id("global.anthropic.claude-sonnet-4-5-20250929-v1:0"),
            "global.anthropic.claude-sonnet-4-5-20250929-v1:0"
        );
    }

    #[test]
    fn test_unknown_model_fallback() {
        assert_eq!(
            resolve_bedrock_model_id("claude-future-model-20260101"),
            "us.anthropic.claude-future-model-20260101-v1:0"
        );
    }
}
