pub mod parser;

use crate::models::{Skill, SkillSource};
use parser::parse_skill_md;
use std::path::{Path, PathBuf};

pub fn discover_skills(directories: &[PathBuf]) -> Vec<Skill> {
    let mut skills = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for dir in directories {
        if !dir.is_dir() {
            continue;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }

            let content = match std::fs::read_to_string(&skill_md) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let (frontmatter, _body) = match parse_skill_md(&content) {
                Ok(parsed) => parsed,
                Err(_) => continue, // skip malformed skills silently
            };

            if seen_names.contains(&frontmatter.name) {
                continue; // first one wins
            }

            seen_names.insert(frontmatter.name.clone());
            skills.push(Skill {
                name: frontmatter.name,
                uri: skill_md.to_string_lossy().to_string(),
                description: frontmatter.description,
                source: SkillSource::Local,
                content: None, // metadata only
                tags: frontmatter.tags,
            });
        }
    }

    skills
}


pub fn load_skill_content(
    name: &str,
    directories: &[PathBuf],
) -> Result<(PathBuf, String), String> {
    let name_lower = name.to_lowercase();

    for dir in directories {
        if !dir.is_dir() {
            continue;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }

            let content = match std::fs::read_to_string(&skill_md) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let (frontmatter, body) = match parse_skill_md(&content) {
                Ok(parsed) => parsed,
                Err(_) => continue,
            };

            if frontmatter.name.to_lowercase() == name_lower {
                return Ok((path, body));
            }
        }
    }

    Err(format!("Skill '{}' not found in any skill directory", name))
}


pub fn load_skill_from_path(path: &Path) -> Result<(PathBuf, String), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    let (_frontmatter, body) = parse_skill_md(&content)?;

    let skill_dir = path
        .parent()
        .ok_or_else(|| "Cannot determine skill directory".to_string())?
        .to_path_buf();

    Ok((skill_dir, body))
}


pub fn default_skill_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Project-level
    dirs.push(PathBuf::from(".stakpak/skills"));

    // User-level
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".stakpak/skills"));
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_skill_dir(base: &Path, name: &str, description: &str, tags: &[&str]) -> PathBuf {
        let skill_dir = base.join(name);
        fs::create_dir_all(&skill_dir).unwrap();

        let tags_str = if tags.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "[{}]",
                tags.iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        let content = format!(
            "---\nname: {}\ndescription: {}\ntags: {}\n---\n\n# {} Instructions\n\nDetailed content here.\n",
            name, description, tags_str, name
        );
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
        skill_dir
    }

    #[test]
    fn test_discover_skills_basic() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(
            tmp.path(),
            "terraform",
            "Terraform best practices",
            &["iac"],
        );
        create_skill_dir(tmp.path(), "docker", "Docker guidelines", &["containers"]);

        let skills = discover_skills(&[tmp.path().to_path_buf()]);
        assert_eq!(skills.len(), 2);
        assert!(skills.iter().all(|s| s.content.is_none())); // progressive disclosure
        assert!(skills.iter().all(|s| s.is_local()));
    }

    #[test]
    fn test_discover_skills_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = discover_skills(&[tmp.path().to_path_buf()]);
        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_skills_nonexistent_dir() {
        let skills = discover_skills(&[PathBuf::from("/nonexistent/path")]);
        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_skills_priority() {
        let high = tempfile::tempdir().unwrap();
        let low = tempfile::tempdir().unwrap();

        create_skill_dir(high.path(), "terraform", "High priority", &[]);
        create_skill_dir(low.path(), "terraform", "Low priority", &[]);

        let skills = discover_skills(&[high.path().to_path_buf(), low.path().to_path_buf()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "High priority");
    }

    #[test]
    fn test_discover_skills_skips_malformed() {
        let tmp = tempfile::tempdir().unwrap();

        // Valid skill
        create_skill_dir(tmp.path(), "good", "A good skill", &[]);

        // Malformed skill (no frontmatter)
        let bad_dir = tmp.path().join("bad");
        fs::create_dir_all(&bad_dir).unwrap();
        fs::write(bad_dir.join("SKILL.md"), "no frontmatter here").unwrap();

        let skills = discover_skills(&[tmp.path().to_path_buf()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "good");
    }

    #[test]
    fn test_load_skill_content() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "terraform", "Terraform practices", &[]);

        let (dir, body) = load_skill_content("terraform", &[tmp.path().to_path_buf()]).unwrap();
        assert_eq!(dir, tmp.path().join("terraform"));
        assert!(body.contains("terraform Instructions"));
    }

    #[test]
    fn test_load_skill_content_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "terraform", "Terraform practices", &[]);

        let result = load_skill_content("Terraform", &[tmp.path().to_path_buf()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_skill_content_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_skill_content("nonexistent", &[tmp.path().to_path_buf()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_skill_from_path() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = create_skill_dir(tmp.path(), "docker", "Docker guidelines", &[]);
        let skill_path = skill_dir.join("SKILL.md");

        let (dir, body) = load_skill_from_path(&skill_path).unwrap();
        assert_eq!(dir, skill_dir);
        assert!(body.contains("docker Instructions"));
    }
}
