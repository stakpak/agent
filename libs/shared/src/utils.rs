use crate::local_store::LocalStore;
use async_trait::async_trait;
use rand::Rng;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::DirEntry;

/// Read .gitignore patterns from the specified base directory
pub fn read_gitignore_patterns(base_dir: &str) -> Vec<String> {
    let mut patterns = vec![".git".to_string()]; // Always ignore .git directory

    let gitignore_path = PathBuf::from(base_dir).join(".gitignore");
    if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
        for line in content.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if !line.is_empty() && !line.starts_with('#') {
                patterns.push(line.to_string());
            }
        }
    }

    patterns
}

/// Check if a directory entry should be included based on gitignore patterns and file type support
pub fn should_include_entry(entry: &DirEntry, base_dir: &str, ignore_patterns: &[String]) -> bool {
    let path = entry.path();
    let is_file = entry.file_type().is_file();

    // Get relative path from base directory
    let base_path = PathBuf::from(base_dir);
    let relative_path = match path.strip_prefix(&base_path) {
        Ok(rel_path) => rel_path,
        Err(_) => path,
    };

    let path_str = relative_path.to_string_lossy();

    // Check if path matches any ignore pattern
    for pattern in ignore_patterns {
        if matches_gitignore_pattern(pattern, &path_str) {
            return false;
        }
    }

    // For files, also check if they are supported file types
    if is_file {
        is_supported_file(entry.path())
    } else {
        true // Allow directories to be traversed
    }
}

/// Check if a path matches a gitignore pattern
#[allow(clippy::string_slice)] // pattern[1..len-1] guarded by starts_with('*')/ends_with('*'), '*' is ASCII
pub fn matches_gitignore_pattern(pattern: &str, path: &str) -> bool {
    // Basic gitignore pattern matching
    let pattern = pattern.trim_end_matches('/'); // Remove trailing slash

    if pattern.contains('*') {
        if pattern == "*" {
            true
        } else if pattern.starts_with('*') && pattern.ends_with('*') {
            let middle = &pattern[1..pattern.len() - 1];
            path.contains(middle)
        } else if let Some(suffix) = pattern.strip_prefix('*') {
            path.ends_with(suffix)
        } else if let Some(prefix) = pattern.strip_suffix('*') {
            path.starts_with(prefix)
        } else {
            // Pattern contains * but not at start/end, do basic glob matching
            pattern_matches_glob(pattern, path)
        }
    } else {
        // Exact match or directory match
        path == pattern || path.starts_with(&format!("{}/", pattern))
    }
}

/// Simple glob pattern matching for basic cases
#[allow(clippy::string_slice)] // text_pos accumulated from starts_with/find on same string, always valid boundaries
pub fn pattern_matches_glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return text == pattern;
    }

    let mut text_pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            // First part must match at the beginning
            if !text[text_pos..].starts_with(part) {
                return false;
            }
            text_pos += part.len();
        } else if i == parts.len() - 1 {
            // Last part must match at the end
            return text[text_pos..].ends_with(part);
        } else {
            // Middle parts must be found in order
            if let Some(pos) = text[text_pos..].find(part) {
                text_pos += pos + part.len();
            } else {
                return false;
            }
        }
    }
    true
}

/// Check if a directory entry represents a supported file type
pub fn is_supported_file(file_path: &Path) -> bool {
    match file_path.file_name().and_then(|name| name.to_str()) {
        Some(name) => {
            // Only allow supported files
            if file_path.is_file() {
                name.ends_with(".tf")
                    || name.ends_with(".tfvars")
                    || name.ends_with(".yaml")
                    || name.ends_with(".yml")
                    || name.to_lowercase().contains("dockerfile")
            } else {
                true // Allow directories to be traversed
            }
        }
        None => false,
    }
}

/// Generate a secure password with alphanumeric characters and optional symbols
pub fn generate_password(length: usize, no_symbols: bool) -> String {
    let mut rng = rand::rng();

    // Define character sets
    let lowercase = "abcdefghijklmnopqrstuvwxyz";
    let uppercase = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let digits = "0123456789";
    let symbols = "!@#$%^&*()_+-=[]{}|;:,.<>?";

    // Build the character set based on options
    let mut charset = String::new();
    charset.push_str(lowercase);
    charset.push_str(uppercase);
    charset.push_str(digits);

    if !no_symbols {
        charset.push_str(symbols);
    }

    let charset_chars: Vec<char> = charset.chars().collect();

    // Generate password ensuring at least one character from each required category
    let mut password = String::new();

    // Ensure at least one character from each category
    password.push(
        lowercase
            .chars()
            .nth(rng.random_range(0..lowercase.len()))
            .unwrap(),
    );
    password.push(
        uppercase
            .chars()
            .nth(rng.random_range(0..uppercase.len()))
            .unwrap(),
    );
    password.push(
        digits
            .chars()
            .nth(rng.random_range(0..digits.len()))
            .unwrap(),
    );

    if !no_symbols {
        password.push(
            symbols
                .chars()
                .nth(rng.random_range(0..symbols.len()))
                .unwrap(),
        );
    }

    // Fill the rest with random characters from the full charset
    let remaining_length = if length > password.len() {
        length - password.len()
    } else {
        0
    };

    for _ in 0..remaining_length {
        let random_char = charset_chars[rng.random_range(0..charset_chars.len())];
        password.push(random_char);
    }

    // Shuffle the password to randomize the order
    let mut password_chars: Vec<char> = password.chars().collect();
    for i in 0..password_chars.len() {
        let j = rng.random_range(0..password_chars.len());
        password_chars.swap(i, j);
    }

    // Take only the requested length
    password_chars.into_iter().take(length).collect()
}

/// Sanitize text output by removing control characters while preserving essential whitespace
pub fn sanitize_text_output(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            // Drop replacement char
            if c == '\u{FFFD}' {
                return false;
            }
            // Allow essential whitespace even though they're "control"
            if matches!(c, '\n' | '\t' | '\r' | ' ') {
                return true;
            }
            // Keep everything else that's not a control character
            !c.is_control()
        })
        .collect()
}

/// Truncate a string by character count and append `...` when truncated.
///
/// Uses char iteration (not byte slicing) so it is UTF-8 safe.
pub fn truncate_chars_with_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut truncated: String = text.chars().take(max_chars).collect();
    truncated.push_str("...");
    truncated
}

/// Handle large output: if the output has >= `max_lines`, save the full content to session
/// storage and return a string showing only the first or last `max_lines` lines with a pointer
/// to the saved file. Returns `Ok(final_string)` or `Err(error_string)` on failure.
pub fn handle_large_output(
    output: &str,
    file_prefix: &str,
    max_lines: usize,
    show_head: bool,
) -> Result<String, String> {
    let output_lines = output.lines().collect::<Vec<_>>();
    if output_lines.len() >= max_lines {
        let mut __rng__ = rand::rng();
        let output_file = format!(
            "{}.{:06x}.txt",
            file_prefix,
            __rng__.random_range(0..=0xFFFFFF)
        );
        let output_file_path = match LocalStore::write_session_data(&output_file, output) {
            Ok(path) => path,
            Err(e) => {
                return Err(format!("Failed to write session data: {}", e));
            }
        };

        let excerpt = if show_head {
            let head_lines: Vec<&str> = output_lines.iter().take(max_lines).copied().collect();
            head_lines.join("\n")
        } else {
            let mut tail_lines: Vec<&str> =
                output_lines.iter().rev().take(max_lines).copied().collect();
            tail_lines.reverse();
            tail_lines.join("\n")
        };

        let position = if show_head { "first" } else { "last" };
        Ok(format!(
            "Showing the {} {} / {} output lines. Full output saved to {}\n{}\n{}",
            position,
            max_lines,
            output_lines.len(),
            output_file_path,
            if show_head { "" } else { "...\n" },
            excerpt
        ))
    } else {
        Ok(output.to_string())
    }
}

#[cfg(test)]
mod password_tests {
    use super::*;

    #[test]
    fn test_generate_password_length() {
        let password = generate_password(10, false);
        assert_eq!(password.len(), 10);

        let password = generate_password(20, true);
        assert_eq!(password.len(), 20);
    }

    #[test]
    fn test_generate_password_no_symbols() {
        let password = generate_password(50, true);
        let symbols = "!@#$%^&*()_+-=[]{}|;:,.<>?";

        for symbol in symbols.chars() {
            assert!(
                !password.contains(symbol),
                "Password should not contain symbol: {}",
                symbol
            );
        }
    }

    #[test]
    fn test_generate_password_with_symbols() {
        let password = generate_password(50, false);
        let symbols = "!@#$%^&*()_+-=[]{}|;:,.<>?";

        // At least one symbol should be present (due to our algorithm)
        let has_symbol = password.chars().any(|c| symbols.contains(c));
        assert!(has_symbol, "Password should contain at least one symbol");
    }

    #[test]
    fn test_generate_password_contains_required_chars() {
        let password = generate_password(50, false);

        let has_lowercase = password.chars().any(|c| c.is_ascii_lowercase());
        let has_uppercase = password.chars().any(|c| c.is_ascii_uppercase());
        let has_digit = password.chars().any(|c| c.is_ascii_digit());

        assert!(has_lowercase, "Password should contain lowercase letters");
        assert!(has_uppercase, "Password should contain uppercase letters");
        assert!(has_digit, "Password should contain digits");
    }

    #[test]
    fn test_generate_password_uniqueness() {
        let password1 = generate_password(20, false);
        let password2 = generate_password(20, false);

        // Very unlikely to generate the same password twice
        assert_ne!(password1, password2);
    }
}

#[cfg(test)]
mod truncate_tests {
    use super::*;

    #[test]
    fn truncate_chars_with_ellipsis_exact_boundary_keeps_value() {
        let value = "a".repeat(20);
        let truncated = truncate_chars_with_ellipsis(&value, 20);
        assert_eq!(truncated, value);
    }

    #[test]
    fn truncate_chars_with_ellipsis_appends_suffix_when_truncated() {
        let value = "é".repeat(10);
        let truncated = truncate_chars_with_ellipsis(&value, 5);
        assert_eq!(truncated, "ééééé...");
    }
}

/// Directory entry information for tree generation
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
}

/// Trait for abstracting file system operations for tree generation
#[async_trait]
pub trait FileSystemProvider {
    type Error: std::fmt::Display;

    /// List directory contents
    async fn list_directory(&self, path: &str) -> Result<Vec<DirectoryEntry>, Self::Error>;
}

/// Generate a tree view of a directory structure using a generic file system provider
pub async fn generate_directory_tree<P: FileSystemProvider>(
    provider: &P,
    path: &str,
    prefix: &str,
    max_depth: usize,
    current_depth: usize,
) -> Result<String, P::Error> {
    let mut result = String::new();

    if current_depth >= max_depth || current_depth >= 10 {
        return Ok(result);
    }

    let entries = provider.list_directory(path).await?;
    let mut file_entries = Vec::new();
    let mut dir_entries = Vec::new();
    for entry in entries.iter() {
        if entry.is_directory {
            if entry.name == "."
                || entry.name == ".."
                || entry.name == ".git"
                || entry.name == "node_modules"
            {
                continue;
            }
            dir_entries.push(entry.clone());
        } else {
            file_entries.push(entry.clone());
        }
    }

    dir_entries.sort_by(|a, b| a.name.cmp(&b.name));
    file_entries.sort_by(|a, b| a.name.cmp(&b.name));

    const MAX_ITEMS: usize = 5;
    let total_items = dir_entries.len() + file_entries.len();
    let should_limit = current_depth > 0 && total_items > MAX_ITEMS;

    if should_limit {
        if dir_entries.len() > MAX_ITEMS {
            dir_entries.truncate(MAX_ITEMS);
            file_entries.clear();
        } else {
            let remaining_items = MAX_ITEMS - dir_entries.len();
            file_entries.truncate(remaining_items);
        }
    }

    let mut dir_headers = Vec::new();
    let mut dir_futures = Vec::new();
    for (i, entry) in dir_entries.iter().enumerate() {
        let is_last_dir = i == dir_entries.len() - 1;
        let is_last_overall = is_last_dir && file_entries.is_empty() && !should_limit;
        let current_prefix = if is_last_overall {
            "└── "
        } else {
            "├── "
        };
        let next_prefix = format!(
            "{}{}",
            prefix,
            if is_last_overall { "    " } else { "│   " }
        );

        let header = format!("{}{}{}/\n", prefix, current_prefix, entry.name);
        dir_headers.push(header);

        let entry_path = entry.path.clone();
        let next_prefix_clone = next_prefix.clone();
        let future = async move {
            generate_directory_tree(
                provider,
                &entry_path,
                &next_prefix_clone,
                max_depth,
                current_depth + 1,
            )
            .await
        };
        dir_futures.push(future);
    }
    if !dir_futures.is_empty() {
        let subtree_results = futures::future::join_all(dir_futures).await;

        for (i, header) in dir_headers.iter().enumerate() {
            result.push_str(header);
            if let Some(Ok(subtree)) = subtree_results.get(i) {
                result.push_str(subtree);
            }
        }
    }

    for (i, entry) in file_entries.iter().enumerate() {
        let is_last_file = i == file_entries.len() - 1;
        let is_last_overall = is_last_file && !should_limit;
        let current_prefix = if is_last_overall {
            "└── "
        } else {
            "├── "
        };
        result.push_str(&format!("{}{}{}\n", prefix, current_prefix, entry.name));
    }

    if should_limit {
        let remaining_count = total_items - MAX_ITEMS;
        result.push_str(&format!(
            "{}└── ... {} more item{}\n",
            prefix,
            remaining_count,
            if remaining_count == 1 { "" } else { "s" }
        ));
    }

    Ok(result)
}

/// Strip the MCP server prefix and any trailing "()" from a tool name.
/// Example: "stakpak__run_command" -> "run_command"
/// Example: "run_command" -> "run_command"
/// Example: "str_replace()" -> "str_replace"
pub fn strip_tool_name(name: &str) -> &str {
    let mut result = name;

    // Strip the MCP server prefix (e.g., "stakpak__")
    if let Some((_, suffix)) = result.split_once("__") {
        result = suffix;
    }

    // Strip trailing "()" if present
    if let Some(stripped) = result.strip_suffix("()") {
        result = stripped;
    }

    backward_compatibility_mapping(result)
}

/// Map legacy tool names to their current counterparts.
/// Currently handles mapping "read_rulebook" to "load_skill".
pub fn backward_compatibility_mapping(name: &str) -> &str {
    match name {
        "read_rulebook" | "read_rulebooks" => "load_skill",
        _ => name,
    }
}

/// Local file system provider implementation
pub struct LocalFileSystemProvider;

#[async_trait]
impl FileSystemProvider for LocalFileSystemProvider {
    type Error = std::io::Error;

    async fn list_directory(&self, path: &str) -> Result<Vec<DirectoryEntry>, Self::Error> {
        let entries = fs::read_dir(path)?;
        let mut result = Vec::new();

        for entry in entries {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            let file_path = entry.path().to_string_lossy().to_string();
            let is_directory = entry.file_type()?.is_dir();

            result.push(DirectoryEntry {
                name: file_name,
                path: file_path,
                is_directory,
            });
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_matches_gitignore_pattern_exact() {
        assert!(matches_gitignore_pattern("node_modules", "node_modules"));
        assert!(matches_gitignore_pattern(
            "node_modules",
            "node_modules/package.json"
        ));
        assert!(!matches_gitignore_pattern(
            "node_modules",
            "src/node_modules"
        ));
    }

    #[test]
    fn test_matches_gitignore_pattern_wildcard_prefix() {
        assert!(matches_gitignore_pattern("*.log", "debug.log"));
        assert!(matches_gitignore_pattern("*.log", "error.log"));
        assert!(!matches_gitignore_pattern("*.log", "log.txt"));
    }

    #[test]
    fn test_matches_gitignore_pattern_wildcard_suffix() {
        assert!(matches_gitignore_pattern("temp*", "temp"));
        assert!(matches_gitignore_pattern("temp*", "temp.txt"));
        assert!(matches_gitignore_pattern("temp*", "temporary"));
        assert!(!matches_gitignore_pattern("temp*", "mytemp"));
    }

    #[test]
    fn test_matches_gitignore_pattern_wildcard_middle() {
        assert!(matches_gitignore_pattern("*temp*", "temp"));
        assert!(matches_gitignore_pattern("*temp*", "mytemp"));
        assert!(matches_gitignore_pattern("*temp*", "temporary"));
        assert!(matches_gitignore_pattern("*temp*", "mytemporary"));
        assert!(!matches_gitignore_pattern("*temp*", "example"));
    }

    #[test]
    fn test_pattern_matches_glob() {
        assert!(pattern_matches_glob("test*.txt", "test.txt"));
        assert!(pattern_matches_glob("test*.txt", "test123.txt"));
        assert!(pattern_matches_glob("*test*.txt", "mytest.txt"));
        assert!(pattern_matches_glob("*test*.txt", "mytestfile.txt"));
        assert!(!pattern_matches_glob("test*.txt", "test.log"));
        assert!(!pattern_matches_glob("*test*.txt", "example.txt"));
    }

    #[test]
    fn test_read_gitignore_patterns() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();

        // Create a .gitignore file
        let gitignore_content = r#"
# This is a comment
node_modules
*.log
dist/
.env

# Another comment
temp*
"#;

        let gitignore_path = temp_path.join(".gitignore");
        let mut file = fs::File::create(&gitignore_path)?;
        file.write_all(gitignore_content.as_bytes())?;

        let patterns = read_gitignore_patterns(temp_path.to_str().unwrap());

        // Should include .git by default
        assert!(patterns.contains(&".git".to_string()));
        assert!(patterns.contains(&"node_modules".to_string()));
        assert!(patterns.contains(&"*.log".to_string()));
        assert!(patterns.contains(&"dist/".to_string()));
        assert!(patterns.contains(&".env".to_string()));
        assert!(patterns.contains(&"temp*".to_string()));

        // Should not include comments or empty lines
        assert!(!patterns.iter().any(|p| p.starts_with('#')));
        assert!(!patterns.contains(&"".to_string()));

        Ok(())
    }

    #[test]
    fn test_read_gitignore_patterns_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let patterns = read_gitignore_patterns(temp_path.to_str().unwrap());

        // Should only contain .git when no .gitignore exists
        assert_eq!(patterns, vec![".git".to_string()]);
    }

    #[test]
    fn test_strip_tool_name() {
        assert_eq!(strip_tool_name("stakpak__run_command"), "run_command");
        assert_eq!(strip_tool_name("run_command"), "run_command");
        assert_eq!(strip_tool_name("str_replace()"), "str_replace");
        assert_eq!(strip_tool_name("stakpak__read_rulebook"), "load_skill");
        assert_eq!(strip_tool_name("read_rulebook()"), "load_skill");
        assert_eq!(strip_tool_name("read_rulebooks"), "load_skill");
        // Additional edge cases
        assert_eq!(strip_tool_name("just_name"), "just_name");
        assert_eq!(strip_tool_name("prefix__name()"), "name");
        assert_eq!(strip_tool_name("nested__prefix__tool"), "prefix__tool");
        assert_eq!(strip_tool_name("empty_suffix()"), "empty_suffix");
    }

    #[test]
    fn test_backward_compatibility_mapping() {
        assert_eq!(
            backward_compatibility_mapping("read_rulebook"),
            "load_skill"
        );
        assert_eq!(
            backward_compatibility_mapping("read_rulebooks"),
            "load_skill"
        );
        assert_eq!(backward_compatibility_mapping("run_command"), "run_command");
    }

    #[test]
    fn test_gitignore_integration() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();

        // Create a .gitignore file
        let gitignore_content = "node_modules\n*.log\ndist/\n";
        let gitignore_path = temp_path.join(".gitignore");
        let mut file = fs::File::create(&gitignore_path)?;
        file.write_all(gitignore_content.as_bytes())?;

        let patterns = read_gitignore_patterns(temp_path.to_str().unwrap());

        // Test various paths
        assert!(
            patterns
                .iter()
                .any(|p| matches_gitignore_pattern(p, "node_modules"))
        );
        assert!(
            patterns
                .iter()
                .any(|p| matches_gitignore_pattern(p, "node_modules/package.json"))
        );
        assert!(
            patterns
                .iter()
                .any(|p| matches_gitignore_pattern(p, "debug.log"))
        );
        assert!(
            patterns
                .iter()
                .any(|p| matches_gitignore_pattern(p, "dist/bundle.js"))
        );
        assert!(
            patterns
                .iter()
                .any(|p| matches_gitignore_pattern(p, ".git"))
        );

        // These should not match
        assert!(
            !patterns
                .iter()
                .any(|p| matches_gitignore_pattern(p, "src/main.js"))
        );
        assert!(
            !patterns
                .iter()
                .any(|p| matches_gitignore_pattern(p, "README.md"))
        );

        Ok(())
    }
}
