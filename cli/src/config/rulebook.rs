//! Rulebook filtering configuration.

use serde::{Deserialize, Serialize};
use stakpak_api::models::ListRuleBook;

/// Check if a string matches a glob pattern.
/// Supports: `*` (any chars), `?` (single char), `[abc]` (char class).
/// Falls back to exact match if pattern is invalid.
pub(crate) fn matches_pattern(value: &str, pattern: &str) -> bool {
    if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
        glob_pattern.matches(value)
    } else {
        // Fallback to exact match if glob pattern is invalid
        value == pattern
    }
}

/// Configuration for filtering which rulebooks are loaded.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RulebookConfig {
    /// Include only these rulebooks by URI (supports wildcards, empty = all allowed)
    pub include: Option<Vec<String>>,
    /// Exclude specific rulebooks (supports wildcards, empty = none excluded)
    pub exclude: Option<Vec<String>>,
    /// Filter by tags to include
    pub include_tags: Option<Vec<String>>,
    /// Filter by tags to exclude
    pub exclude_tags: Option<Vec<String>>,
}

impl RulebookConfig {
    /// Filter rulebooks based on the configuration rules.
    pub fn filter_rulebooks(&self, rulebooks: Vec<ListRuleBook>) -> Vec<ListRuleBook> {
        rulebooks
            .into_iter()
            .filter(|rulebook| self.should_keep(rulebook))
            .collect()
    }

    fn should_keep(&self, rulebook: &ListRuleBook) -> bool {
        self.matches_uri_filters(rulebook) && self.matches_tag_filters(rulebook)
    }

    fn matches_uri_filters(&self, rulebook: &ListRuleBook) -> bool {
        self.matches_include_patterns(rulebook) && self.matches_exclude_patterns(rulebook)
    }

    fn matches_include_patterns(&self, rulebook: &ListRuleBook) -> bool {
        match &self.include {
            Some(patterns) if !patterns.is_empty() => patterns
                .iter()
                .any(|pattern| matches_pattern(&rulebook.uri, pattern)),
            _ => true,
        }
    }

    fn matches_exclude_patterns(&self, rulebook: &ListRuleBook) -> bool {
        match &self.exclude {
            Some(patterns) if !patterns.is_empty() => !patterns
                .iter()
                .any(|pattern| matches_pattern(&rulebook.uri, pattern)),
            _ => true,
        }
    }

    fn matches_tag_filters(&self, rulebook: &ListRuleBook) -> bool {
        self.matches_include_tags(rulebook) && self.matches_exclude_tags(rulebook)
    }

    fn matches_include_tags(&self, rulebook: &ListRuleBook) -> bool {
        match &self.include_tags {
            Some(tags) if !tags.is_empty() => tags.iter().any(|tag| rulebook.tags.contains(tag)),
            _ => true,
        }
    }

    fn matches_exclude_tags(&self, rulebook: &ListRuleBook) -> bool {
        match &self.exclude_tags {
            Some(tags) if !tags.is_empty() => !tags.iter().any(|tag| rulebook.tags.contains(tag)),
            _ => true,
        }
    }
}
