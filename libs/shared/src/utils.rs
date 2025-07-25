use rand::Rng;
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
pub fn matches_gitignore_pattern(pattern: &str, path: &str) -> bool {
    // Basic gitignore pattern matching
    let pattern = pattern.trim_end_matches('/'); // Remove trailing slash

    if pattern.contains('*') {
        // Simple wildcard matching
        if pattern.starts_with('*') && pattern.ends_with('*') {
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
