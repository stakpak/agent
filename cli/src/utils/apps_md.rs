use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of parent directories to traverse when searching for APPS.md
const MAX_TRAVERSAL_DEPTH: usize = 5;

/// Information about a discovered APPS.md file
#[derive(Debug, Clone)]
pub struct AppsMdInfo {
    pub content: String,
    pub path: PathBuf,
}

/// Discovers and reads the nearest APPS.md file from the given directory upward
/// (up to 3 parent levels), falling back to the global `~/.stakpak/APPS.md`.
///
/// Returns the first (nearest) APPS.md found. Closest to start_dir wins.
///
/// Search order at each directory level:
/// 1. APPS.md (canonical)
/// 2. apps.md (lowercase variant)
///
/// If nothing found within 3 levels:
/// 3. ~/.stakpak/APPS.md (global fallback)
pub fn discover_apps_md(start_dir: &Path) -> Option<AppsMdInfo> {
    let mut current = start_dir.to_path_buf();

    for _ in 0..=MAX_TRAVERSAL_DEPTH {
        // Check canonical APPS.md first
        let apps_file = current.join("APPS.md");
        if apps_file.exists()
            && let Ok(content) = fs::read_to_string(&apps_file)
        {
            return Some(AppsMdInfo {
                content,
                path: apps_file.canonicalize().unwrap_or(apps_file),
            });
        }

        // Check lowercase variant
        let apps_file_lower = current.join("apps.md");
        if apps_file_lower.exists()
            && let Ok(content) = fs::read_to_string(&apps_file_lower)
        {
            return Some(AppsMdInfo {
                content,
                path: apps_file_lower.canonicalize().unwrap_or(apps_file_lower),
            });
        }

        // Move up to parent directory
        if !current.pop() {
            break;
        }
    }

    // Fall back to global ~/.stakpak/APPS.md
    if let Some(home) = std::env::home_dir() {
        let global_apps = home.join(".stakpak").join("APPS.md");
        if global_apps.exists()
            && let Ok(content) = fs::read_to_string(&global_apps)
        {
            return Some(AppsMdInfo {
                content,
                path: global_apps.canonicalize().unwrap_or(global_apps),
            });
        }
    }

    None
}

/// Format APPS.md content for context injection
pub fn format_apps_md_for_context(info: &AppsMdInfo) -> String {
    format!(
        "# APPS.md (from {})\n\n{}",
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
    fn test_discover_apps_md_in_current_dir() {
        let temp_dir = TempDir::new().unwrap();
        let apps_path = temp_dir.path().join("APPS.md");
        let mut file = File::create(&apps_path).unwrap();
        writeln!(file, "# Test APPS.md\n\nSome content").unwrap();

        let result = discover_apps_md(temp_dir.path());
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.content.contains("Test APPS.md"));
        assert_eq!(info.path, apps_path.canonicalize().unwrap());
    }

    #[test]
    fn test_discover_apps_md_in_parent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let apps_path = temp_dir.path().join("APPS.md");
        let mut file = File::create(&apps_path).unwrap();
        writeln!(file, "# Parent APPS.md").unwrap();

        let child_dir = temp_dir.path().join("subdir");
        fs::create_dir(&child_dir).unwrap();

        let result = discover_apps_md(&child_dir);
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.content.contains("Parent APPS.md"));
    }

    #[test]
    fn test_discover_apps_md_lowercase() {
        let temp_dir = TempDir::new().unwrap();
        let apps_path = temp_dir.path().join("apps.md");
        let mut file = File::create(&apps_path).unwrap();
        writeln!(file, "# Lowercase apps.md").unwrap();

        let result = discover_apps_md(temp_dir.path());
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.content.contains("Lowercase apps.md"));
    }

    #[test]
    fn test_discover_apps_md_respects_max_depth() {
        let temp_dir = TempDir::new().unwrap();

        // Create APPS.md 7 levels up — should NOT be found (max depth is 5)
        let apps_path = temp_dir.path().join("APPS.md");
        let mut file = File::create(&apps_path).unwrap();
        writeln!(file, "# Too far APPS.md").unwrap();

        let deep_dir = temp_dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("f")
            .join("g");
        fs::create_dir_all(&deep_dir).unwrap();

        let result = discover_apps_md(&deep_dir);
        // Should not find the APPS.md that is 7 levels up
        if let Some(info) = result {
            assert!(
                !info.content.contains("Too far"),
                "Should not discover APPS.md beyond max traversal depth"
            );
        }
    }

    #[test]
    fn test_discover_apps_md_within_max_depth() {
        let temp_dir = TempDir::new().unwrap();

        // Create APPS.md 5 levels up — should be found
        let apps_path = temp_dir.path().join("APPS.md");
        let mut file = File::create(&apps_path).unwrap();
        writeln!(file, "# Reachable APPS.md").unwrap();

        let deep_dir = temp_dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e");
        fs::create_dir_all(&deep_dir).unwrap();

        let result = discover_apps_md(&deep_dir);
        assert!(result.is_some());
        assert!(
            result.unwrap().content.contains("Reachable"),
            "Should discover APPS.md within max traversal depth"
        );
    }

    #[test]
    fn test_discover_apps_md_nearest_wins() {
        let temp_dir = TempDir::new().unwrap();

        // Create APPS.md at root
        let root_apps = temp_dir.path().join("APPS.md");
        let mut file = File::create(&root_apps).unwrap();
        writeln!(file, "# Root APPS.md").unwrap();

        // Create APPS.md in child
        let child_dir = temp_dir.path().join("child");
        fs::create_dir(&child_dir).unwrap();
        let child_apps = child_dir.join("APPS.md");
        let mut file = File::create(&child_apps).unwrap();
        writeln!(file, "# Child APPS.md").unwrap();

        let result = discover_apps_md(&child_dir);
        assert!(result.is_some());
        let info = result.unwrap();
        // Nearest (child) should win
        assert!(info.content.contains("Child APPS.md"));
        assert_eq!(info.path, child_apps.canonicalize().unwrap());
    }

    #[test]
    fn test_discover_apps_md_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let result = discover_apps_md(temp_dir.path());
        // May find global ~/.stakpak/APPS.md if it exists, otherwise None
        if let Some(info) = result {
            assert!(
                info.path.to_string_lossy().contains(".stakpak"),
                "Should only find global APPS.md (if any)"
            );
        }
    }

    #[test]
    fn test_discover_apps_md_canonical_takes_precedence() {
        let temp_dir = TempDir::new().unwrap();

        // Create canonical APPS.md
        let canonical = temp_dir.path().join("APPS.md");
        let mut file = File::create(&canonical).unwrap();
        writeln!(file, "# Canonical").unwrap();

        // On case-insensitive filesystems (macOS, Windows), creating apps.md
        // would overwrite APPS.md. So we just verify that APPS.md is found
        // when it exists (the precedence logic works on case-sensitive systems).
        let result = discover_apps_md(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().content.contains("Canonical"));
    }

    #[test]
    fn test_format_apps_md_for_context() {
        let info = AppsMdInfo {
            content: "## My App\n- Port 8080".to_string(),
            path: PathBuf::from("/project/APPS.md"),
        };

        let formatted = format_apps_md_for_context(&info);
        assert!(formatted.contains("# APPS.md (from /project/APPS.md)"));
        assert!(formatted.contains("## My App"));
        assert!(formatted.contains("- Port 8080"));
    }
}
