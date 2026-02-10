use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub fn parse_skill_md(content: &str) -> Result<(SkillFrontmatter, String), String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err("SKILL.md must start with YAML frontmatter (---)".to_string());
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    let end_idx = after_first
        .find("\n---")
        .ok_or_else(|| "SKILL.md frontmatter missing closing ---".to_string())?;

    let yaml_str = &after_first[..end_idx];
    let body_start = end_idx + 4; // skip "\n---"
    let body = after_first[body_start..]
        .trim_start_matches('\n')
        .to_string();

    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_str)
        .map_err(|e| format!("Failed to parse frontmatter: {}", e))?;

    if frontmatter.name.is_empty() {
        return Err("SKILL.md frontmatter 'name' is required".to_string());
    }
    if frontmatter.description.is_empty() {
        return Err("SKILL.md frontmatter 'description' is required".to_string());
    }

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
    fn test_parse_no_tags() {
        let content = "---\nname: simple\ndescription: A simple skill\n---\n\nBody here.\n";
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name, "simple");
        assert!(fm.tags.is_empty());
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
    }

    #[test]
    fn test_parse_empty_description() {
        let content = "---\nname: test\ndescription: \"\"\n---\n\nBody";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }
}
