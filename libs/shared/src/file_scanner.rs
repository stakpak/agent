//! Flat directory scanning for markdown files.

use std::path::Path;

/// Scan dir for `.md` files. `name_prefix` strips from stem (e.g. cmd_deploy.md → deploy).
/// `max_bytes`: skip larger files (0 = no limit).
pub fn scan_flat_markdown_dir(
    dir: &Path,
    name_prefix: Option<&str>,
    max_bytes: u64,
) -> Vec<(String, String)> {
    if !dir.is_dir() {
        return vec![];
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };

    let mut result = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        let name = match name_prefix {
            Some(prefix) => match stem.strip_prefix(prefix) {
                Some(n) if !n.is_empty() => n.to_string(),
                _ => continue,
            },
            None => stem.to_string(),
        };

        if max_bytes > 0 {
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            if metadata.len() > max_bytes {
                continue;
            }
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c.trim().to_string(),
            Err(_) => continue,
        };
        if content.is_empty() {
            continue;
        }

        result.push((name, content));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_scan_flat_markdown_dir() {
        let dir = TempDir::new().unwrap();
        assert!(scan_flat_markdown_dir(dir.path(), None, 0).is_empty());

        fs::write(dir.path().join("cmd_deploy.md"), "# Deploy\nDeploy to prod").unwrap();
        fs::write(dir.path().join("cmd_test.md"), "# Test\nRun tests").unwrap();
        fs::write(dir.path().join("other.md"), "# Other\n").unwrap();

        let with_prefix = scan_flat_markdown_dir(dir.path(), Some("cmd_"), 0);
        assert_eq!(with_prefix.len(), 2);
        let names: Vec<_> = with_prefix.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"deploy"));
        assert!(names.contains(&"test"));

        let dir2 = TempDir::new().unwrap();
        fs::write(dir2.path().join("foo.md"), "# Foo\nContent").unwrap();
        let no_prefix = scan_flat_markdown_dir(dir2.path(), None, 0);
        assert_eq!(no_prefix.len(), 1);
        assert_eq!(no_prefix[0].0, "foo");
        assert_eq!(no_prefix[0].1, "# Foo\nContent");
    }
}
