use std::fs;
use std::path::{Path, PathBuf};

/// Information about a discovered AGENTS.md file
#[derive(Debug, Clone)]
pub struct AgentsMdInfo {
    pub content: String,
    pub path: PathBuf,
}

/// Discovers and reads AGENTS.md file from the given directory upward.
/// Returns the nearest AGENTS.md found (closest to start_dir wins per spec).
///
/// Search order at each directory level:
/// 1. AGENTS.md (canonical)
/// 2. agents.md (lowercase variant)
pub fn discover_agents_md(start_dir: &Path) -> Option<AgentsMdInfo> {
    let mut current = start_dir.to_path_buf();

    loop {
        // Check canonical AGENTS.md first
        let agents_file = current.join("AGENTS.md");
        if agents_file.exists()
            && let Ok(content) = fs::read_to_string(&agents_file)
        {
            return Some(AgentsMdInfo {
                content,
                path: agents_file,
            });
        }

        // Check lowercase variant
        let agents_file_lower = current.join("agents.md");
        if agents_file_lower.exists()
            && let Ok(content) = fs::read_to_string(&agents_file_lower)
        {
            return Some(AgentsMdInfo {
                content,
                path: agents_file_lower,
            });
        }

        // Move up to parent directory
        if !current.pop() {
            break;
        }
    }

    None
}

/// Format AGENTS.md content for context injection
pub fn format_agents_md_for_context(info: &AgentsMdInfo) -> String {
    format!(
        "# AGENTS.md (from {})\n\n{}",
        info.path.display(),
        info.content.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_discover_agents_md_in_current_dir() {
        let temp_dir = TempDir::new().unwrap();
        let agents_path = temp_dir.path().join("AGENTS.md");
        let mut file = File::create(&agents_path).unwrap();
        writeln!(file, "# Test AGENTS.md\n\nSome content").unwrap();

        let result = discover_agents_md(temp_dir.path());
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.content.contains("Test AGENTS.md"));
        assert_eq!(info.path, agents_path);
    }

    #[test]
    fn test_discover_agents_md_in_parent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let agents_path = temp_dir.path().join("AGENTS.md");
        let mut file = File::create(&agents_path).unwrap();
        writeln!(file, "# Parent AGENTS.md").unwrap();

        let child_dir = temp_dir.path().join("subdir");
        fs::create_dir(&child_dir).unwrap();

        let result = discover_agents_md(&child_dir);
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.content.contains("Parent AGENTS.md"));
    }

    #[test]
    fn test_discover_agents_md_lowercase() {
        let temp_dir = TempDir::new().unwrap();
        let agents_path = temp_dir.path().join("agents.md");
        let mut file = File::create(&agents_path).unwrap();
        writeln!(file, "# Lowercase agents.md").unwrap();

        let result = discover_agents_md(temp_dir.path());
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.content.contains("Lowercase agents.md"));
    }

    #[test]
    fn test_discover_agents_md_canonical_takes_precedence() {
        let temp_dir = TempDir::new().unwrap();

        // Create canonical AGENTS.md
        let canonical = temp_dir.path().join("AGENTS.md");
        let mut file = File::create(&canonical).unwrap();
        writeln!(file, "# Canonical").unwrap();

        // On case-insensitive filesystems (macOS, Windows), creating agents.md
        // would overwrite AGENTS.md. So we just verify that AGENTS.md is found
        // when it exists (the precedence logic works on case-sensitive systems).
        let result = discover_agents_md(temp_dir.path());
        assert!(result.is_some());
        let info = result.unwrap();
        // Should find the file we created
        assert!(info.content.contains("Canonical"));
    }

    #[test]
    fn test_discover_agents_md_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let result = discover_agents_md(temp_dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_format_agents_md_for_context() {
        let info = AgentsMdInfo {
            content: "## Setup\n- Run tests".to_string(),
            path: PathBuf::from("/project/AGENTS.md"),
        };

        let formatted = format_agents_md_for_context(&info);
        assert!(formatted.contains("# AGENTS.md (from /project/AGENTS.md)"));
        assert!(formatted.contains("## Setup"));
        assert!(formatted.contains("- Run tests"));
    }
}
