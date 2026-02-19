//! Comment data model for plan review comments.
//!
//! Comments are kept in-memory during the review session and discarded
//! when the review is closed. They are formatted into feedback text
//! before being sent to the agent.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─── Types ───────────────────────────────────────────────────────────────────

/// Author of a comment or reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentAuthor {
    User,
    Agent,
}

/// How a comment is anchored to the plan content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnchorType {
    Heading,
    Line,
}

/// The anchor point connecting a comment to plan content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentAnchor {
    pub anchor_type: AnchorType,
    /// The text content at the anchor point (heading text or line text).
    pub text: String,
}

/// A top-level comment on the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanComment {
    /// Comment ID, e.g. `cmt_01`, `cmt_02`.
    pub id: String,
    pub anchor: CommentAnchor,
    pub author: CommentAuthor,
    pub text: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub resolved: bool,
}

/// Root container for all comments on a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanComments {
    /// Always `"plan.md"`.
    pub plan_file: String,
    /// SHA-256 hash of the plan content when comments were last synced.
    pub plan_hash: String,
    #[serde(default)]
    pub comments: Vec<PlanComment>,
}

// ─── Comment Operations ──────────────────────────────────────────────────────

/// Generate the next comment ID based on existing comments.
///
/// Returns `cmt_01`, `cmt_02`, etc.
fn next_comment_id(comments: &[PlanComment]) -> String {
    let max_num = comments
        .iter()
        .filter_map(|c| {
            c.id.strip_prefix("cmt_")
                .and_then(|s| s.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);
    format!("cmt_{:02}", max_num + 1)
}

/// Add a new top-level comment. Returns the generated comment ID.
pub fn add_comment(
    plan_comments: &mut PlanComments,
    anchor: CommentAnchor,
    author: CommentAuthor,
    text: String,
) -> String {
    let id = next_comment_id(&plan_comments.comments);
    plan_comments.comments.push(PlanComment {
        id: id.clone(),
        anchor,
        author,
        text,
        created_at: Utc::now(),
        resolved: false,
    });
    id
}

/// Mark a comment as resolved.
///
/// Returns `true` if the comment was found and updated, `false` otherwise.
pub fn resolve_comment(plan_comments: &mut PlanComments, comment_id: &str) -> bool {
    if let Some(comment) = plan_comments
        .comments
        .iter_mut()
        .find(|c| c.id == comment_id)
    {
        comment.resolved = true;
        true
    } else {
        false
    }
}

// ─── Anchor Matching ─────────────────────────────────────────────────────────

/// Minimum normalized similarity for a fuzzy match to be accepted.
const FUZZY_MATCH_THRESHOLD: f64 = 0.7;

/// Quality of an anchor-to-line match.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchQuality {
    /// Exact text match.
    Exact,
    /// Fuzzy match with similarity score (0.0–1.0).
    Fuzzy(f64),
    /// No acceptable match found.
    Orphaned,
}

/// A comment anchor resolved to a specific line in the plan.
#[derive(Debug, Clone)]
pub struct ResolvedAnchor {
    /// 0-indexed line number in the plan content.
    pub line_number: usize,
    /// How well the anchor matched.
    pub match_quality: MatchQuality,
}

/// Compute normalized Levenshtein similarity between two strings.
///
/// Returns a value in `[0.0, 1.0]` where 1.0 means identical.
/// Both strings are trimmed and compared case-insensitively.
pub fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();

    if a == b {
        return 1.0;
    }

    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 && b_len == 0 {
        return 1.0;
    }
    if a_len == 0 || b_len == 0 {
        return 0.0;
    }

    // Standard DP Levenshtein with two-row optimization
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for (i, a_ch) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = if a_ch == b_ch { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    let distance = prev[b_len];
    let max_len = a_len.max(b_len);
    1.0 - (distance as f64 / max_len as f64)
}

/// Resolve comment anchors to line numbers in the plan content.
///
/// For each comment, tries:
/// 1. Exact match against candidate lines
/// 2. Best fuzzy match above [`FUZZY_MATCH_THRESHOLD`]
/// 3. Falls back to Orphaned (line_number = 0)
///
/// Results are sorted by line_number.
pub fn resolve_anchors(
    plan_content: &str,
    comments: &[PlanComment],
) -> Vec<(String, ResolvedAnchor)> {
    let lines: Vec<&str> = plan_content.lines().collect();

    let mut results: Vec<(String, ResolvedAnchor)> = comments
        .iter()
        .map(|comment| {
            let anchor_text = comment.anchor.text.trim();

            // Determine which lines are candidates based on anchor type
            let candidates: Vec<(usize, &str)> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        return false;
                    }
                    match comment.anchor.anchor_type {
                        AnchorType::Heading => trimmed.starts_with('#'),
                        AnchorType::Line => true,
                    }
                })
                .map(|(i, line)| (i, *line))
                .collect();

            // 1. Exact match
            for &(line_num, line) in &candidates {
                if line.trim() == anchor_text {
                    return (
                        comment.id.clone(),
                        ResolvedAnchor {
                            line_number: line_num,
                            match_quality: MatchQuality::Exact,
                        },
                    );
                }
            }

            // 2. Fuzzy match — find best above threshold
            let mut best_score = 0.0_f64;
            let mut best_line = 0;

            for &(line_num, line) in &candidates {
                let score = levenshtein_similarity(anchor_text, line.trim());
                if score > best_score {
                    best_score = score;
                    best_line = line_num;
                }
            }

            if best_score >= FUZZY_MATCH_THRESHOLD {
                return (
                    comment.id.clone(),
                    ResolvedAnchor {
                        line_number: best_line,
                        match_quality: MatchQuality::Fuzzy(best_score),
                    },
                );
            }

            // 3. Orphaned
            (
                comment.id.clone(),
                ResolvedAnchor {
                    line_number: 0,
                    match_quality: MatchQuality::Orphaned,
                },
            )
        })
        .collect();

    results.sort_by_key(|(_, anchor)| anchor.line_number);
    results
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plan::compute_plan_hash;

    fn sample_anchor() -> CommentAnchor {
        CommentAnchor {
            anchor_type: AnchorType::Heading,
            text: "## Overview".to_string(),
        }
    }

    fn sample_plan_comments() -> PlanComments {
        PlanComments {
            plan_file: "plan.md".to_string(),
            plan_hash: compute_plan_hash("test content"),
            comments: Vec::new(),
        }
    }

    // ── Serialization ────────────────────────────────────────────────────

    #[test]
    fn test_serde_roundtrip_empty() {
        let pc = sample_plan_comments();
        let json = serde_json::to_string_pretty(&pc).unwrap();
        let parsed: PlanComments = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.plan_file, "plan.md");
        assert_eq!(parsed.plan_hash, pc.plan_hash);
        assert!(parsed.comments.is_empty());
    }

    #[test]
    fn test_serde_roundtrip_with_comments() {
        let mut pc = sample_plan_comments();
        add_comment(
            &mut pc,
            sample_anchor(),
            CommentAuthor::User,
            "Need more detail here".to_string(),
        );
        add_comment(
            &mut pc,
            CommentAnchor {
                anchor_type: AnchorType::Line,
                text: "Use RDS PostgreSQL".to_string(),
            },
            CommentAuthor::Agent,
            "Should we consider Aurora?".to_string(),
        );

        let json = serde_json::to_string_pretty(&pc).unwrap();
        let parsed: PlanComments = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.comments.len(), 2);
        assert_eq!(parsed.comments[0].id, "cmt_01");
        assert_eq!(parsed.comments[1].id, "cmt_02");
        assert_eq!(parsed.comments[1].anchor.anchor_type, AnchorType::Line);
    }

    // ── Comment IDs ──────────────────────────────────────────────────────

    #[test]
    fn test_add_comment_ids() {
        let mut pc = sample_plan_comments();
        let id1 = add_comment(
            &mut pc,
            sample_anchor(),
            CommentAuthor::User,
            "First".to_string(),
        );
        let id2 = add_comment(
            &mut pc,
            sample_anchor(),
            CommentAuthor::User,
            "Second".to_string(),
        );
        let id3 = add_comment(
            &mut pc,
            sample_anchor(),
            CommentAuthor::Agent,
            "Third".to_string(),
        );

        assert_eq!(id1, "cmt_01");
        assert_eq!(id2, "cmt_02");
        assert_eq!(id3, "cmt_03");
        assert_eq!(pc.comments.len(), 3);
    }

    #[test]
    fn test_add_comment_fields() {
        let mut pc = sample_plan_comments();
        add_comment(
            &mut pc,
            sample_anchor(),
            CommentAuthor::User,
            "Some feedback".to_string(),
        );

        let comment = &pc.comments[0];
        assert_eq!(comment.author, CommentAuthor::User);
        assert_eq!(comment.text, "Some feedback");
        assert!(!comment.resolved);
        assert_eq!(comment.anchor.text, "## Overview");
    }

    // ── Resolve ──────────────────────────────────────────────────────────

    #[test]
    // ── Resolve ──────────────────────────────────────────────────────────
    #[test]
    fn test_resolve_comment() {
        let mut pc = sample_plan_comments();
        add_comment(
            &mut pc,
            sample_anchor(),
            CommentAuthor::User,
            "Fix this".to_string(),
        );

        assert!(!pc.comments[0].resolved);
        assert!(resolve_comment(&mut pc, "cmt_01"));
        assert!(pc.comments[0].resolved);
    }

    #[test]
    fn test_resolve_nonexistent_comment() {
        let mut pc = sample_plan_comments();
        assert!(!resolve_comment(&mut pc, "cmt_99"));
    }

    // ── Author enum ──────────────────────────────────────────────────────

    #[test]
    fn test_comment_author_serde() {
        let json_user = serde_json::to_string(&CommentAuthor::User).unwrap();
        assert_eq!(json_user, "\"user\"");
        let json_agent = serde_json::to_string(&CommentAuthor::Agent).unwrap();
        assert_eq!(json_agent, "\"agent\"");

        let parsed: CommentAuthor = serde_json::from_str("\"user\"").unwrap();
        assert_eq!(parsed, CommentAuthor::User);
    }

    // ── AnchorType enum ──────────────────────────────────────────────────

    #[test]
    fn test_anchor_type_serde() {
        let json = serde_json::to_string(&AnchorType::Heading).unwrap();
        assert_eq!(json, "\"heading\"");
        let parsed: AnchorType = serde_json::from_str("\"line\"").unwrap();
        assert_eq!(parsed, AnchorType::Line);
    }

    // ── ID Generation Edge Cases ─────────────────────────────────────────

    #[test]
    fn test_next_comment_id_with_gaps() {
        let comments = vec![
            PlanComment {
                id: "cmt_01".to_string(),
                anchor: sample_anchor(),
                author: CommentAuthor::User,
                text: "A".to_string(),
                created_at: Utc::now(),
                resolved: false,
            },
            PlanComment {
                id: "cmt_05".to_string(),
                anchor: sample_anchor(),
                author: CommentAuthor::User,
                text: "B".to_string(),
                created_at: Utc::now(),
                resolved: false,
            },
        ];
        assert_eq!(next_comment_id(&comments), "cmt_06");
    }

    // ── Levenshtein Similarity ───────────────────────────────────────────

    #[test]
    fn test_levenshtein_identical() {
        assert!((levenshtein_similarity("hello", "hello") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_levenshtein_case_insensitive() {
        assert!((levenshtein_similarity("Hello", "hello") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_levenshtein_completely_different() {
        let score = levenshtein_similarity("abc", "xyz");
        assert!(score < 0.1, "Expected very low similarity, got {score}");
    }

    #[test]
    fn test_levenshtein_empty_strings() {
        assert!((levenshtein_similarity("", "") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_levenshtein_one_empty() {
        assert!((levenshtein_similarity("hello", "")).abs() < f64::EPSILON);
        assert!((levenshtein_similarity("", "hello")).abs() < f64::EPSILON);
    }

    #[test]
    fn test_levenshtein_similar_heading() {
        // "Step 2: Design schema" vs "Step 2: Design database schema"
        let score = levenshtein_similarity(
            "## Step 2: Design schema",
            "## Step 2: Design database schema",
        );
        assert!(
            score >= 0.7,
            "Expected fuzzy match above threshold, got {score}"
        );
    }

    #[test]
    fn test_levenshtein_trims_whitespace() {
        assert!((levenshtein_similarity("  hello  ", "hello") - 1.0).abs() < f64::EPSILON);
    }

    // ── Anchor Resolution ────────────────────────────────────────────────

    const SAMPLE_PLAN: &str = "\
# Deploy Auth Service

## Overview

Implement OAuth-based authentication for the API gateway.

## Step 1: Set up database

Use PostgreSQL on RDS.

## Step 2: Design schema

Create users and sessions tables.

## Step 3: Implement endpoints

Build login, logout, and refresh token endpoints.
";

    fn make_comment(id: &str, anchor_type: AnchorType, text: &str) -> PlanComment {
        PlanComment {
            id: id.to_string(),
            anchor: CommentAnchor {
                anchor_type,
                text: text.to_string(),
            },
            author: CommentAuthor::User,
            text: "Some feedback".to_string(),
            created_at: Utc::now(),
            resolved: false,
        }
    }

    #[test]
    fn test_resolve_exact_heading() {
        let comments = vec![make_comment("cmt_01", AnchorType::Heading, "## Overview")];
        let resolved = resolve_anchors(SAMPLE_PLAN, &comments);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, "cmt_01");
        assert_eq!(resolved[0].1.match_quality, MatchQuality::Exact);
        // "## Overview" is line index 2
        assert_eq!(resolved[0].1.line_number, 2);
    }

    #[test]
    fn test_resolve_exact_line() {
        let comments = vec![make_comment(
            "cmt_01",
            AnchorType::Line,
            "Use PostgreSQL on RDS.",
        )];
        let resolved = resolve_anchors(SAMPLE_PLAN, &comments);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].1.match_quality, MatchQuality::Exact);
    }

    #[test]
    fn test_resolve_fuzzy_heading() {
        // Heading was edited: "Step 2: Design schema" → "Step 2: Design database schema"
        let plan_edited = SAMPLE_PLAN.replace(
            "## Step 2: Design schema",
            "## Step 2: Design database schema",
        );
        let comments = vec![make_comment(
            "cmt_01",
            AnchorType::Heading,
            "## Step 2: Design schema",
        )];
        let resolved = resolve_anchors(&plan_edited, &comments);

        assert_eq!(resolved.len(), 1);
        match &resolved[0].1.match_quality {
            MatchQuality::Fuzzy(score) => {
                assert!(*score >= 0.7, "Expected fuzzy score >= 0.7, got {score}");
            }
            other => panic!("Expected Fuzzy match, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_orphaned() {
        let comments = vec![make_comment(
            "cmt_01",
            AnchorType::Heading,
            "## Completely Removed Section",
        )];
        let resolved = resolve_anchors(SAMPLE_PLAN, &comments);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].1.match_quality, MatchQuality::Orphaned);
    }

    #[test]
    fn test_resolve_multiple_same_line() {
        let comments = vec![
            make_comment("cmt_01", AnchorType::Heading, "## Overview"),
            make_comment("cmt_02", AnchorType::Heading, "## Overview"),
        ];
        let resolved = resolve_anchors(SAMPLE_PLAN, &comments);

        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].1.line_number, resolved[1].1.line_number);
        assert_eq!(resolved[0].1.match_quality, MatchQuality::Exact);
        assert_eq!(resolved[1].1.match_quality, MatchQuality::Exact);
    }

    #[test]
    fn test_resolve_sorted_by_line_number() {
        let comments = vec![
            make_comment(
                "cmt_01",
                AnchorType::Heading,
                "## Step 3: Implement endpoints",
            ),
            make_comment("cmt_02", AnchorType::Heading, "## Overview"),
        ];
        let resolved = resolve_anchors(SAMPLE_PLAN, &comments);

        assert_eq!(resolved.len(), 2);
        // Overview (line 2) should come before Step 3 (line 14)
        assert_eq!(resolved[0].0, "cmt_02");
        assert_eq!(resolved[1].0, "cmt_01");
        assert!(resolved[0].1.line_number < resolved[1].1.line_number);
    }

    #[test]
    fn test_resolve_heading_anchor_ignores_non_headings() {
        // A heading anchor should NOT match a regular line even if text matches
        let plan = "# Title\n\nSome heading-like text\n\n## Real Heading\n";
        let comments = vec![make_comment(
            "cmt_01",
            AnchorType::Heading,
            "Some heading-like text",
        )];
        let resolved = resolve_anchors(plan, &comments);

        // Should be orphaned because "Some heading-like text" isn't a # line
        assert_eq!(resolved[0].1.match_quality, MatchQuality::Orphaned);
    }

    #[test]
    fn test_resolve_empty_plan() {
        let comments = vec![make_comment("cmt_01", AnchorType::Line, "anything")];
        let resolved = resolve_anchors("", &comments);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].1.match_quality, MatchQuality::Orphaned);
    }

    #[test]
    fn test_resolve_no_comments() {
        let resolved = resolve_anchors(SAMPLE_PLAN, &[]);
        assert!(resolved.is_empty());
    }
}
