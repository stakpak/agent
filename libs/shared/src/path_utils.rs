//! Path utilities.

use std::path::PathBuf;

/// Expand `~/` to home directory.
pub fn expand_path(path: &str) -> PathBuf {
    path.strip_prefix("~/")
        .and_then(|s| dirs::home_dir().map(|h| h.join(s)))
        .unwrap_or_else(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_path() {
        assert_eq!(expand_path("/foo/bar"), PathBuf::from("/foo/bar"));
        assert_eq!(expand_path("foo/bar"), PathBuf::from("foo/bar"));
        let with_tilde = expand_path("~/test");
        assert!(with_tilde.to_string_lossy().contains("test"));
    }
}
