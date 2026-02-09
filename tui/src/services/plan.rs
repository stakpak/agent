//! Plan mode utilities: front matter parsing, metadata, and file I/O.
//!
//! The plan is stored at `.stakpak/session/plan.md` with YAML front matter
//! containing metadata (title, status, version, timestamps). This module
//! provides parsing, reading, and status types for the TUI to consume.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};

/// Plan file path relative to session directory.
pub const PLAN_FILENAME: &str = "plan.md";

// ─── PlanStatus ──────────────────────────────────────────────────────────────

/// Status of the plan as set in the YAML front matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    #[default]
    #[serde(alias = "draft")]
    Drafting,
    #[serde(alias = "reviewing")]
    PendingReview,
    #[serde(alias = "approved")]
    Approved,
}

impl fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanStatus::Drafting => write!(f, "drafting"),
            PlanStatus::PendingReview => write!(f, "pending_review"),
            PlanStatus::Approved => write!(f, "approved"),
        }
    }
}

// ─── PlanMetadata ────────────────────────────────────────────────────────────

/// Metadata extracted from the plan file's YAML front matter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanMetadata {
    pub title: String,
    pub status: PlanStatus,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub created: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated: Option<DateTime<Utc>>,
}

fn default_version() -> u32 {
    1
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

/// Parse YAML front matter from plan.md content.
///
/// Front matter is delimited by `---` markers at the top of the file:
/// ```text
/// ---
/// title: Deploy Auth Service
/// status: reviewing
/// version: 2
/// ---
/// ```
///
/// Returns `None` if:
/// - The content doesn't start with `---`
/// - There is no closing `---`
/// - The YAML is malformed or missing required fields (`title`, `status`)
pub fn parse_plan_front_matter(content: &str) -> Option<PlanMetadata> {
    let trimmed = content.trim_start();

    // Must start with ---
    let after_opening = trimmed.strip_prefix("---")?;
    let closing_pos = after_opening.find("\n---")?;

    // SAFETY: closing_pos from find() is always a valid char boundary
    let yaml_str = after_opening.get(..closing_pos)?.trim();

    if yaml_str.is_empty() {
        return None;
    }

    serde_yaml::from_str(yaml_str).ok()
}

/// Extract the plan body (everything after the front matter).
pub fn extract_plan_body(content: &str) -> &str {
    let trimmed = content.trim_start();

    let Some(after_opening) = trimmed.strip_prefix("---") else {
        return content;
    };

    if let Some(closing_pos) = after_opening.find("\n---") {
        // skip \n--- (4 ASCII bytes, always valid boundary after find)
        let rest_start = closing_pos + "\n---".len();
        let after_closing = after_opening.get(rest_start..).unwrap_or("");
        // Skip the newline after closing ---
        after_closing.strip_prefix('\n').unwrap_or(after_closing)
    } else {
        content
    }
}

// ─── File I/O ────────────────────────────────────────────────────────────────

/// Build the full path to plan.md given a session directory.
pub fn plan_file_path(session_dir: &Path) -> PathBuf {
    session_dir.join(PLAN_FILENAME)
}

/// Check if plan.md exists in the given session directory.
pub fn plan_file_exists(session_dir: &Path) -> bool {
    plan_file_path(session_dir).exists()
}

/// Read and parse plan.md from the session directory.
///
/// Returns `None` if the file doesn't exist, can't be read, or has no
/// valid front matter.
pub fn read_plan_file(session_dir: &Path) -> Option<(PlanMetadata, String)> {
    let path = plan_file_path(session_dir);
    let content = std::fs::read_to_string(path).ok()?;
    let metadata = parse_plan_front_matter(&content)?;
    Some((metadata, content))
}

/// Compute SHA-256 hash of plan content (for drift detection).
pub fn compute_plan_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ─── Archive ─────────────────────────────────────────────────────────────────

/// Archive the existing plan.md by renaming it with its creation timestamp.
///
/// Uses the `created` field from YAML front matter for the suffix. Falls back
/// to file modification time, then current time if neither is available.
///
/// Renames `plan.md` → `plan.<YYYYMMDD_HHMMSS>.md`.
/// Returns the archive path on success, or `None` if no plan file exists.
pub fn archive_plan_file(session_dir: &Path) -> Option<PathBuf> {
    let plan_path = plan_file_path(session_dir);
    if !plan_path.exists() {
        return None;
    }

    let ts = std::fs::read_to_string(&plan_path)
        .ok()
        .and_then(|c| parse_plan_front_matter(&c))
        .and_then(|m| m.created)
        .or_else(|| {
            std::fs::metadata(&plan_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(DateTime::<Utc>::from)
        })
        .unwrap_or_else(Utc::now)
        .format("%Y%m%d_%H%M%S");

    let archive_path = session_dir.join(format!("plan.{ts}.md"));
    std::fs::rename(&plan_path, &archive_path).ok()?;
    Some(archive_path)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_FRONT_MATTER: &str = "\
---
title: Deploy Auth Service
status: pending_review
version: 2
created: 2026-02-07T15:30:00Z
updated: 2026-02-07T16:45:00Z
---

## Overview

Implement OAuth-based authentication.
";

    const MINIMAL_FRONT_MATTER: &str = "\
---
title: Quick Fix
status: drafting
---

Some content.
";

    #[test]
    fn test_parse_valid_front_matter() {
        let meta = parse_plan_front_matter(VALID_FRONT_MATTER);
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.title, "Deploy Auth Service");
        assert_eq!(meta.status, PlanStatus::PendingReview);
        assert_eq!(meta.version, 2);
        assert!(meta.created.is_some());
        assert!(meta.updated.is_some());
    }

    #[test]
    fn test_parse_minimal_front_matter() {
        let meta = parse_plan_front_matter(MINIMAL_FRONT_MATTER);
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.title, "Quick Fix");
        assert_eq!(meta.status, PlanStatus::Drafting);
        assert_eq!(meta.version, 1); // default
        assert!(meta.created.is_none());
        assert!(meta.updated.is_none());
    }

    #[test]
    fn test_parse_no_front_matter() {
        let content = "# Just a heading\n\nSome content.";
        assert!(parse_plan_front_matter(content).is_none());
    }

    #[test]
    fn test_parse_empty_file() {
        assert!(parse_plan_front_matter("").is_none());
    }

    #[test]
    fn test_parse_unclosed_front_matter() {
        let content = "---\ntitle: Broken\nstatus: draft\n";
        assert!(parse_plan_front_matter(content).is_none());
    }

    #[test]
    fn test_parse_empty_front_matter() {
        let content = "---\n---\nSome content.";
        assert!(parse_plan_front_matter(content).is_none());
    }

    #[test]
    fn test_parse_missing_required_title() {
        let content = "---\nstatus: draft\n---\n";
        assert!(parse_plan_front_matter(content).is_none());
    }

    #[test]
    fn test_parse_missing_required_status() {
        let content = "---\ntitle: Something\n---\n";
        assert!(parse_plan_front_matter(content).is_none());
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let content = "---\n: : : broken yaml\n---\n";
        assert!(parse_plan_front_matter(content).is_none());
    }

    #[test]
    fn test_extract_plan_body() {
        let body = extract_plan_body(VALID_FRONT_MATTER);
        let trimmed_body = body.trim_start();
        assert!(
            trimmed_body.starts_with("## Overview"),
            "Expected body to start with '## Overview' but got: {:?}",
            body.chars().take(50).collect::<String>()
        );
        assert!(body.contains("OAuth-based authentication"));
    }

    #[test]
    fn test_extract_plan_body_no_front_matter() {
        let content = "# Just content\nHello";
        assert_eq!(extract_plan_body(content), content);
    }

    #[test]
    fn test_plan_status_display() {
        assert_eq!(PlanStatus::Drafting.to_string(), "drafting");
        assert_eq!(PlanStatus::PendingReview.to_string(), "pending_review");
        assert_eq!(PlanStatus::Approved.to_string(), "approved");
    }

    #[test]
    fn test_plan_status_default() {
        assert_eq!(PlanStatus::default(), PlanStatus::Drafting);
    }

    #[test]
    fn test_compute_plan_hash() {
        let hash1 = compute_plan_hash("hello");
        let hash2 = compute_plan_hash("hello");
        let hash3 = compute_plan_hash("world");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_plan_file_exists_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!plan_file_exists(tmp.path()));
    }

    #[test]
    fn test_read_plan_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_plan_file(tmp.path()).is_none());
    }

    #[test]
    fn test_read_plan_file_valid() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(PLAN_FILENAME), VALID_FRONT_MATTER).unwrap();
        let result = read_plan_file(tmp.path());
        assert!(result.is_some());
        let (meta, content) = result.unwrap();
        assert_eq!(meta.title, "Deploy Auth Service");
        assert_eq!(content, VALID_FRONT_MATTER);
    }

    #[test]
    fn test_read_plan_file_no_front_matter() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(PLAN_FILENAME), "# No front matter").unwrap();
        assert!(read_plan_file(tmp.path()).is_none());
    }

    #[test]
    fn test_plan_status_serde_roundtrip() {
        let meta = PlanMetadata {
            title: "Test Plan".to_string(),
            status: PlanStatus::PendingReview,
            version: 3,
            created: Some(Utc::now()),
            updated: None,
        };
        let yaml = serde_yaml::to_string(&meta).unwrap();
        let parsed: PlanMetadata = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.title, "Test Plan");
        assert_eq!(parsed.status, PlanStatus::PendingReview);
        assert_eq!(parsed.version, 3);
    }

    #[test]
    fn test_parse_front_matter_with_leading_whitespace() {
        let content = "\n\n---\ntitle: Spaced\nstatus: approved\n---\nBody";
        let meta = parse_plan_front_matter(content);
        assert!(meta.is_some());
        assert_eq!(meta.unwrap().status, PlanStatus::Approved);
    }

    #[test]
    fn test_archive_plan_file_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(archive_plan_file(tmp.path()).is_none());
    }

    #[test]
    fn test_archive_plan_file_with_created_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(PLAN_FILENAME), VALID_FRONT_MATTER).unwrap();
        let archive = archive_plan_file(tmp.path());
        assert!(archive.is_some());
        let archive_path = archive.unwrap();
        // created: 2026-02-07T15:30:00Z → plan.20260207_153000.md
        assert_eq!(
            archive_path.file_name().unwrap().to_str().unwrap(),
            "plan.20260207_153000.md"
        );
        assert!(archive_path.exists());
        assert!(!plan_file_exists(tmp.path()));
    }

    #[test]
    fn test_archive_plan_file_no_created_field() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(PLAN_FILENAME), MINIMAL_FRONT_MATTER).unwrap();
        let archive = archive_plan_file(tmp.path());
        assert!(archive.is_some());
        let archive_path = archive.unwrap();
        // No created field → falls back to mtime or now; just check it was renamed
        assert!(archive_path.exists());
        assert!(!plan_file_exists(tmp.path()));
    }
}
