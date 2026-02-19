use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct SkillFrontmatter {
    pub name: String,
    /// Should describe both what the skill does and when to use it
    pub description: String,
    /// License name or reference to a bundled license file.
    pub license: Option<String>,
    /// Environment requirements (intended product, system packages, network access, etc.). Max 500 chars.
    pub compatibility: Option<String>,
    /// Arbitrary key-value mapping for additional metadata.
    pub metadata: Option<HashMap<String, String>>,
    /// Space-delimited list of pre-approved tools the skill may use. (Experimental)
    #[serde(default, rename = "allowed-tools")]
    pub allowed_tools: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Validate that a skill name conforms to the Agent Skills spec:
/// - 1-64 characters
/// - Lowercase alphanumeric and hyphens only
/// - Must not start or end with a hyphen
/// - Must not contain consecutive hyphens
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Skill name must not be empty".to_string());
    }
    if name.len() > 64 {
        return Err(format!(
            "Skill name must be at most 64 characters, got {}",
            name.len()
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(format!(
            "Skill name '{}' must not start or end with a hyphen",
            name
        ));
    }
    if name.contains("--") {
        return Err(format!(
            "Skill name '{}' must not contain consecutive hyphens",
            name
        ));
    }
    for ch in name.chars() {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' {
            return Err(format!(
                "Skill name '{}' contains invalid character '{}'. Only lowercase letters, numbers, and hyphens are allowed",
                name, ch
            ));
        }
    }
    Ok(())
}

/// Validate that the description is non-empty and within the max length.
fn validate_description(description: &str) -> Result<(), String> {
    if description.is_empty() {
        return Err("Skill description must not be empty".to_string());
    }
    if description.len() > 1024 {
        return Err(format!(
            "Skill description must be at most 1024 characters, got {}",
            description.len()
        ));
    }
    Ok(())
}

/// Validate that the compatibility field (if present) is within the max length.
fn validate_compatibility(compatibility: &Option<String>) -> Result<(), String> {
    if let Some(compat) = compatibility {
        if compat.is_empty() {
            return Err("Skill compatibility field, if provided, must not be empty".to_string());
        }
        if compat.len() > 500 {
            return Err(format!(
                "Skill compatibility must be at most 500 characters, got {}",
                compat.len()
            ));
        }
    }
    Ok(())
}

/// Validate that the skill name matches the parent directory name.
pub fn validate_name_matches_directory(name: &str, dir_name: &str) -> Result<(), String> {
    if name != dir_name {
        return Err(format!(
            "Skill name '{}' must match the parent directory name '{}'",
            name, dir_name
        ));
    }
    Ok(())
}

pub fn parse_skill_md(content: &str) -> Result<(SkillFrontmatter, String), String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err("SKILL.md must start with YAML frontmatter (---)".to_string());
    }

    // Find the closing ---
    // SAFETY: trimmed starts with "---" (3 ASCII bytes), so index 3 is always a valid char boundary.
    let after_first = trimmed
        .get(3..)
        .ok_or_else(|| "SKILL.md frontmatter unexpectedly short".to_string())?;
    let end_idx = after_first
        .find("\n---")
        .ok_or_else(|| "SKILL.md frontmatter missing closing ---".to_string())?;

    // end_idx comes from .find() on after_first, so it's a valid char boundary.
    let yaml_str = after_first
        .get(..end_idx)
        .ok_or_else(|| "SKILL.md frontmatter invalid boundary".to_string())?;
    let body_start = end_idx + 4; // skip "\n---"
    let body = after_first
        .get(body_start..)
        .unwrap_or("")
        .trim_start_matches('\n')
        .to_string();

    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_str)
        .map_err(|e| format!("Failed to parse frontmatter: {}", e))?;

    // Validate required fields
    validate_name(&frontmatter.name)?;
    validate_description(&frontmatter.description)?;

    // Validate optional fields
    validate_compatibility(&frontmatter.compatibility)?;

    Ok((frontmatter, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_skill_md() {
        let content = r#"---
name: terraform-aws
description: Best practices for Terraform on AWS
tags: [terraform, aws, iac]
---

# Terraform AWS Instructions

Step-by-step guidance here...
"#;
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "terraform-aws");
        assert_eq!(fm.description, "Best practices for Terraform on AWS");
        assert_eq!(fm.tags, vec!["terraform", "aws", "iac"]);
        assert!(body.starts_with("# Terraform AWS Instructions"));
    }

    #[test]
    fn test_parse_all_optional_fields() {
        let content = r#"---
name: pdf-processing
description: Extract text and tables from PDF files, fill forms, merge documents.
license: Apache-2.0
compatibility: Requires poppler-utils and python3
metadata:
  author: example-org
  version: "1.0"
allowed-tools: Bash(git:*) Bash(jq:*) Read
tags: [pdf, extraction]
---

# PDF Processing

Instructions here.
"#;
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "pdf-processing");
        assert_eq!(fm.license, Some("Apache-2.0".to_string()));
        assert_eq!(
            fm.compatibility,
            Some("Requires poppler-utils and python3".to_string())
        );
        let metadata = fm.metadata.as_ref().unwrap();
        assert_eq!(metadata.get("author"), Some(&"example-org".to_string()));
        assert_eq!(metadata.get("version"), Some(&"1.0".to_string()));
        assert_eq!(
            fm.allowed_tools,
            Some("Bash(git:*) Bash(jq:*) Read".to_string())
        );
        assert_eq!(fm.tags, vec!["pdf", "extraction"]);
        assert!(body.starts_with("# PDF Processing"));
    }

    #[test]
    fn test_parse_no_tags() {
        let content = "---\nname: simple\ndescription: A simple skill\n---\n\nBody here.\n";
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "simple");
        assert!(fm.tags.is_empty());
        assert!(fm.license.is_none());
        assert!(fm.compatibility.is_none());
        assert!(fm.metadata.is_none());
        assert!(fm.allowed_tools.is_none());
        assert_eq!(body, "Body here.\n");
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# No frontmatter\n\nJust markdown.";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_closing() {
        let content = "---\nname: broken\ndescription: oops\n";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_name() {
        let content = "---\nname: \"\"\ndescription: has desc\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not be empty"));
    }

    #[test]
    fn test_parse_empty_description() {
        let content = "---\nname: test\ndescription: \"\"\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not be empty"));
    }

    // --- Name validation tests ---

    #[test]
    fn test_name_uppercase_rejected() {
        let content = "---\nname: PDF-Processing\ndescription: A skill\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid character"));
    }

    #[test]
    fn test_name_starts_with_hyphen_rejected() {
        let content = "---\nname: -pdf\ndescription: A skill\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not start or end"));
    }

    #[test]
    fn test_name_ends_with_hyphen_rejected() {
        let content = "---\nname: pdf-\ndescription: A skill\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not start or end"));
    }

    #[test]
    fn test_name_consecutive_hyphens_rejected() {
        let content = "---\nname: pdf--processing\ndescription: A skill\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("consecutive hyphens"));
    }

    #[test]
    fn test_name_too_long_rejected() {
        let long_name = "a".repeat(65);
        let content = format!(
            "---\nname: {}\ndescription: A skill\n---\n\nBody",
            long_name
        );
        let result = parse_skill_md(&content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("at most 64 characters"));
    }

    #[test]
    fn test_name_max_length_accepted() {
        let max_name = "a".repeat(64);
        let content = format!("---\nname: {}\ndescription: A skill\n---\n\nBody", max_name);
        let result = parse_skill_md(&content);
        assert!(result.is_ok());
    }

    #[test]
    fn test_name_with_numbers_accepted() {
        let content = "---\nname: skill-v2\ndescription: A skill\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.name, "skill-v2");
    }

    #[test]
    fn test_name_with_spaces_rejected() {
        let content = "---\nname: \"my skill\"\ndescription: A skill\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid character"));
    }

    #[test]
    fn test_name_with_underscores_rejected() {
        let content = "---\nname: my_skill\ndescription: A skill\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid character"));
    }

    // --- Description validation tests ---

    #[test]
    fn test_description_too_long_rejected() {
        let long_desc = "a".repeat(1025);
        let content = format!("---\nname: test\ndescription: {}\n---\n\nBody", long_desc);
        let result = parse_skill_md(&content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("at most 1024 characters"));
    }

    #[test]
    fn test_description_max_length_accepted() {
        let max_desc = "a".repeat(1024);
        let content = format!("---\nname: test\ndescription: {}\n---\n\nBody", max_desc);
        let result = parse_skill_md(&content);
        assert!(result.is_ok());
    }

    // --- Compatibility validation tests ---

    #[test]
    fn test_compatibility_too_long_rejected() {
        let long_compat = "a".repeat(501);
        let content = format!(
            "---\nname: test\ndescription: A skill\ncompatibility: {}\n---\n\nBody",
            long_compat
        );
        let result = parse_skill_md(&content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("at most 500 characters"));
    }

    #[test]
    fn test_compatibility_max_length_accepted() {
        let max_compat = "a".repeat(500);
        let content = format!(
            "---\nname: test\ndescription: A skill\ncompatibility: {}\n---\n\nBody",
            max_compat
        );
        let result = parse_skill_md(&content);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compatibility_empty_rejected() {
        let content = "---\nname: test\ndescription: A skill\ncompatibility: \"\"\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must not be empty"));
    }

    // --- Directory name matching ---

    #[test]
    fn test_name_matches_directory_ok() {
        let result = validate_name_matches_directory("terraform", "terraform");
        assert!(result.is_ok());
    }

    #[test]
    fn test_name_mismatches_directory() {
        let result = validate_name_matches_directory("terraform", "tf-skill");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("must match the parent directory name")
        );
    }
}
