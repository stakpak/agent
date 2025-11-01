pub mod gitleaks;
use crate::helper::generate_simple_id;
/// Re-export the gitleaks initialization function for external access
pub use gitleaks::initialize_gitleaks_config;
use gitleaks::{DetectedSecret, detect_secrets};
use std::collections::HashMap;
use std::fmt;

/// A result containing both the redacted string and the mapping of redaction keys to original secrets
#[derive(Debug, Clone)]
pub struct RedactionResult {
    /// The input string with secrets replaced by redaction keys
    pub redacted_string: String,
    /// Mapping from redaction key to the original secret value
    pub redaction_map: HashMap<String, String>,
}

impl RedactionResult {
    pub fn new(redacted_string: String, redaction_map: HashMap<String, String>) -> Self {
        Self {
            redacted_string,
            redaction_map,
        }
    }
}

impl fmt::Display for RedactionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.redacted_string)
    }
}

/// Redacts secrets from the input string and returns both the redacted string and redaction mapping
///
/// When privacy_mode is enabled, also detects and redacts private data like IP addresses and AWS account IDs
pub fn redact_secrets(
    content: &str,
    path: Option<&str>,
    old_redaction_map: &HashMap<String, String>,
    privacy_mode: bool,
) -> RedactionResult {
    let mut secrets = detect_secrets(content, path, privacy_mode);

    let mut redaction_map = old_redaction_map.clone();
    let mut reverse_redaction_map: HashMap<String, String> = old_redaction_map
        .clone()
        .into_iter()
        .map(|(k, v)| (v, k))
        .collect();

    for (original_secret, redaction_key) in &reverse_redaction_map {
        // Extract rule_id from redaction_key format: [REDACTED_SECRET:rule_id:id]
        let key_parts = redaction_key.split(':').collect::<Vec<&str>>();
        if key_parts.len() == 3 {
            let rule_id = key_parts[1].to_string();
            if let Some(start) = content.find(original_secret) {
                let end = start + original_secret.len();
                secrets.push(DetectedSecret {
                    rule_id,
                    value: original_secret.clone(),
                    start_pos: start,
                    end_pos: end,
                });
            }
        }
    }

    if secrets.is_empty() {
        return RedactionResult::new(content.to_string(), HashMap::new());
    }

    let mut redacted_string = content.to_string();

    // Deduplicate overlapping secrets - keep the longest one
    let mut deduplicated_secrets: Vec<DetectedSecret> = Vec::new();
    let mut sorted_by_start = secrets;
    sorted_by_start.sort_by(|a, b| a.start_pos.cmp(&b.start_pos));

    for secret in sorted_by_start {
        let mut should_add = true;
        let mut to_remove = Vec::new();

        for (i, existing) in deduplicated_secrets.iter().enumerate() {
            // Check if secrets overlap
            let overlaps =
                secret.start_pos < existing.end_pos && secret.end_pos > existing.start_pos;

            if overlaps {
                // Keep the longer secret (more specific)
                if secret.value.len() > existing.value.len() {
                    to_remove.push(i);
                } else {
                    should_add = false;
                    break;
                }
            }
        }

        // Remove secrets that should be replaced by this longer one
        for &i in to_remove.iter().rev() {
            deduplicated_secrets.remove(i);
        }

        if should_add {
            deduplicated_secrets.push(secret);
        }
    }

    // Sort by position in reverse order to avoid index shifting issues
    deduplicated_secrets.sort_by(|a, b| b.start_pos.cmp(&a.start_pos));

    for secret in deduplicated_secrets {
        // Validate character boundaries before replacement
        if !content.is_char_boundary(secret.start_pos) || !content.is_char_boundary(secret.end_pos)
        {
            continue;
        }

        // Validate positions are within bounds
        if secret.start_pos >= redacted_string.len() || secret.end_pos > redacted_string.len() {
            continue;
        }

        // make sure same secrets have the same redaction key within the same file
        // without making the hash content dependent (content addressable)
        let redaction_key = if let Some(existing_key) = reverse_redaction_map.get(&secret.value) {
            existing_key.clone()
        } else {
            let key = generate_redaction_key(&secret.rule_id);
            // Store the mapping (only once per unique secret value)
            redaction_map.insert(key.clone(), secret.value.clone());
            reverse_redaction_map.insert(secret.value, key.clone());
            key
        };

        // Replace the secret in the string
        redacted_string.replace_range(secret.start_pos..secret.end_pos, &redaction_key);
    }

    RedactionResult::new(redacted_string, redaction_map)
}

/// Restores secrets in a redacted string using the provided redaction map
pub fn restore_secrets(redacted_string: &str, redaction_map: &HashMap<String, String>) -> String {
    let mut restored = redacted_string.to_string();

    for (redaction_key, original_value) in redaction_map {
        restored = restored.replace(redaction_key, original_value);
    }

    restored
}

/// Redacts a specific password value from the content without running secret detection
pub fn redact_password(
    content: &str,
    password: &str,
    old_redaction_map: &HashMap<String, String>,
) -> RedactionResult {
    if password.is_empty() {
        return RedactionResult::new(content.to_string(), HashMap::new());
    }

    let mut redacted_string = content.to_string();
    let mut redaction_map = old_redaction_map.clone();
    let mut reverse_redaction_map: HashMap<String, String> = old_redaction_map
        .clone()
        .into_iter()
        .map(|(k, v)| (v, k))
        .collect();

    // Check if we already have a redaction key for this password
    let redaction_key = if let Some(existing_key) = reverse_redaction_map.get(password) {
        existing_key.clone()
    } else {
        let key = generate_redaction_key("password");
        // Store the mapping
        redaction_map.insert(key.clone(), password.to_string());
        reverse_redaction_map.insert(password.to_string(), key.clone());
        key
    };

    // Replace all occurrences of the password
    redacted_string = redacted_string.replace(password, &redaction_key);

    RedactionResult::new(redacted_string, redaction_map)
}

/// Generates a random redaction key
fn generate_redaction_key(rule_id: &str) -> String {
    let id = generate_simple_id(6);
    format!("[REDACTED_SECRET:{rule_id}:{id}]")
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    use crate::secrets::gitleaks::{
        GITLEAKS_CONFIG, calculate_entropy, contains_any_keyword, create_simple_api_key_regex,
        is_allowed_by_rule_allowlist, should_allow_match,
    };

    use super::*;

    #[test]
    fn test_redaction_key_generation() {
        let key1 = generate_redaction_key("test");
        let key2 = generate_redaction_key("my-rule");

        // Keys should be different
        assert_ne!(key1, key2);

        // Keys should follow the expected format
        assert!(key1.starts_with("[REDACTED_SECRET:test:"));
        assert!(key1.ends_with("]"));
        assert!(key2.starts_with("[REDACTED_SECRET:my-rule:"));
        assert!(key2.ends_with("]"));
    }

    #[test]
    fn test_empty_input() {
        let result = redact_secrets("", None, &HashMap::new(), false);
        assert_eq!(result.redacted_string, "");
        assert!(result.redaction_map.is_empty());
    }

    #[test]
    fn test_restore_secrets() {
        let mut redaction_map = HashMap::new();
        redaction_map.insert("[REDACTED_abc123]".to_string(), "secret123".to_string());
        redaction_map.insert("[REDACTED_def456]".to_string(), "api_key_xyz".to_string());

        let redacted = "Password is [REDACTED_abc123] and key is [REDACTED_def456]";
        let restored = restore_secrets(redacted, &redaction_map);

        assert_eq!(restored, "Password is secret123 and key is api_key_xyz");
    }

    #[test]
    fn test_redaction_result_display() {
        let mut redaction_map = HashMap::new();
        redaction_map.insert("[REDACTED_test]".to_string(), "secret".to_string());

        let result = RedactionResult::new("Hello [REDACTED_test]".to_string(), redaction_map);
        assert_eq!(format!("{}", result), "Hello [REDACTED_test]");
    }

    #[test]
    fn test_redact_secrets_with_api_key() {
        // Use a pattern that matches the generic-api-key rule
        let input = "export API_KEY=abc123def456ghi789jkl012mno345pqr678";
        let result = redact_secrets(input, None, &HashMap::new(), false);

        // Should detect the API key and redact it
        assert!(result.redaction_map.len() > 0);
        assert!(result.redacted_string.contains("[REDACTED_"));
        println!("Input: {}", input);
        println!("Redacted: {}", result.redacted_string);
        println!("Mapping: {:?}", result.redaction_map);
    }

    #[test]
    fn test_redact_secrets_with_aws_key() {
        let input = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EX23PLE";
        let result = redact_secrets(input, None, &HashMap::new(), false);

        // Should detect the AWS access key
        assert!(result.redaction_map.len() > 0);
        println!("Input: {}", input);
        println!("Redacted: {}", result.redacted_string);
        println!("Mapping: {:?}", result.redaction_map);
    }

    #[test]
    fn test_redaction_identical_secrets() {
        let input = r#"
        export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EX23PLE
        export AWS_ACCESS_KEY_ID_2=AKIAIOSFODNN7EX23PLE
        "#;
        let result = redact_secrets(input, None, &HashMap::new(), false);

        assert_eq!(result.redaction_map.len(), 1);
    }

    #[test]
    fn test_redaction_identical_secrets_different_contexts() {
        let input_1 = r#"
        export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EX23PLE
        "#;
        let input_2 = r#"
        export SOME_OTHER_SECRET=AKIAIOSFODNN7EX23PLE
        "#;
        let result_1 = redact_secrets(input_1, None, &HashMap::new(), false);
        let result_2 = redact_secrets(input_2, None, &result_1.redaction_map, false);

        assert_eq!(result_1.redaction_map, result_2.redaction_map);
    }

    #[test]
    fn test_redact_secrets_with_github_token() {
        let input = "GITHUB_TOKEN=ghp_1234567890abcdef1234567890abcdef12345678";
        let result = redact_secrets(input, None, &HashMap::new(), false);

        // Should detect the GitHub PAT
        assert!(result.redaction_map.len() > 0);
        println!("Input: {}", input);
        println!("Redacted: {}", result.redacted_string);
        println!("Mapping: {:?}", result.redaction_map);
    }

    #[test]
    fn test_no_secrets() {
        let input = "This is just a normal string with no secrets";
        let result = redact_secrets(input, None, &HashMap::new(), false);

        // Should not detect any secrets
        assert_eq!(result.redaction_map.len(), 0);
        assert_eq!(result.redacted_string, input);
    }

    #[test]
    fn test_debug_generic_api_key() {
        let config = &*GITLEAKS_CONFIG;

        // Find the generic-api-key rule
        let generic_rule = config.rules.iter().find(|r| r.id == "generic-api-key");
        if let Some(rule) = generic_rule {
            println!("Generic API Key Rule:");
            println!("  Regex: {:?}", rule.regex);
            println!("  Entropy: {:?}", rule.entropy);
            println!("  Keywords: {:?}", rule.keywords);

            // Test the regex directly first
            if let Some(regex_pattern) = &rule.regex {
                if let Ok(regex) = Regex::new(regex_pattern) {
                    let test_input = "API_KEY=abc123def456ghi789jkl012mno345pqr678";
                    println!("\nTesting regex directly:");
                    println!("  Input: {}", test_input);

                    for mat in regex.find_iter(test_input) {
                        println!("  Raw match: '{}'", mat.as_str());
                        println!("  Match position: {}-{}", mat.start(), mat.end());

                        // Check captures
                        if let Some(captures) = regex.captures(mat.as_str()) {
                            for (i, cap) in captures.iter().enumerate() {
                                if let Some(cap) = cap {
                                    println!("  Capture {}: '{}'", i, cap.as_str());
                                    if i == 1 {
                                        let entropy = calculate_entropy(cap.as_str());
                                        println!("  Entropy of capture 1: {:.2}", entropy);
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                println!("  No regex pattern (path-based rule)");
            }

            // Test various input patterns
            let test_inputs = vec![
                "API_KEY=abc123def456ghi789jkl012mno345pqr678",
                "api_key=RaNd0mH1ghEnTr0pyV4luE567890abcdef",
                "access_key=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF2gH5jK",
                "secret_token=1234567890abcdef1234567890abcdef",
                "password=9k2L8pMvB3nQ7rX1ZdF5GhJwY4AsPo6C",
            ];

            for input in test_inputs {
                println!("\nTesting input: {}", input);
                let result = redact_secrets(input, None, &HashMap::new(), false);
                println!("  Detected secrets: {}", result.redaction_map.len());
                if result.redaction_map.len() > 0 {
                    println!("  Redacted: {}", result.redacted_string);
                }
            }
        } else {
            println!("Generic API key rule not found!");
        }
    }

    #[test]
    fn test_simple_regex_match() {
        // Test a very simple case that should definitely match
        let input = "key=abcdefghijklmnop";
        println!("Testing simple input: {}", input);

        let config = &*GITLEAKS_CONFIG;
        let generic_rule = config
            .rules
            .iter()
            .find(|r| r.id == "generic-api-key")
            .unwrap();

        if let Some(regex_pattern) = &generic_rule.regex {
            if let Ok(regex) = Regex::new(regex_pattern) {
                println!("Regex pattern: {}", regex_pattern);

                if regex.is_match(input) {
                    println!("✓ Regex MATCHES the input!");

                    for mat in regex.find_iter(input) {
                        println!("Match found: '{}'", mat.as_str());

                        if let Some(captures) = regex.captures(mat.as_str()) {
                            println!("Full capture groups:");
                            for (i, cap) in captures.iter().enumerate() {
                                if let Some(cap) = cap {
                                    println!("  Group {}: '{}'", i, cap.as_str());
                                    if i == 1 {
                                        let entropy = calculate_entropy(cap.as_str());
                                        println!("  Entropy: {:.2} (threshold: 3.5)", entropy);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    println!("✗ Regex does NOT match the input");
                }
            }
        } else {
            println!("Rule has no regex pattern (path-based rule)");
        }

        // Also test the full redact_secrets function
        let result = redact_secrets(input, None, &HashMap::new(), false);
        println!(
            "Full function result: {} secrets detected",
            result.redaction_map.len()
        );
    }

    #[test]
    fn test_regex_breakdown() {
        let config = &*GITLEAKS_CONFIG;
        let generic_rule = config
            .rules
            .iter()
            .find(|r| r.id == "generic-api-key")
            .unwrap();

        if let Some(regex_pattern) = &generic_rule.regex {
            println!("Full regex: {}", regex_pattern);

            // Let's break down the regex and test each part
            let test_inputs = vec![
                "key=abcdefghijklmnop",
                "api_key=abcdefghijklmnop",
                "secret=abcdefghijklmnop",
                "token=abcdefghijklmnop",
                "password=abcdefghijklmnop",
                "access_key=abcdefghijklmnop",
            ];

            for input in test_inputs {
                println!("\nTesting: '{}'", input);

                // Test if the regex matches at all
                if let Ok(regex) = Regex::new(regex_pattern) {
                    let matches: Vec<_> = regex.find_iter(input).collect();
                    println!("  Matches found: {}", matches.len());

                    for (i, mat) in matches.iter().enumerate() {
                        println!("  Match {}: '{}'", i, mat.as_str());

                        // Test captures
                        if let Some(captures) = regex.captures(mat.as_str()) {
                            for (j, cap) in captures.iter().enumerate() {
                                if let Some(cap) = cap {
                                    println!("    Capture {}: '{}'", j, cap.as_str());
                                    if j == 1 {
                                        let entropy = calculate_entropy(cap.as_str());
                                        println!("    Entropy: {:.2} (threshold: 3.5)", entropy);
                                        if entropy >= 3.5 {
                                            println!("    ✓ Entropy check PASSED");
                                        } else {
                                            println!("    ✗ Entropy check FAILED");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            println!("Rule has no regex pattern (path-based rule)");
        }

        // Also test with a known working pattern from AWS
        println!("\nTesting AWS pattern that we know works:");
        let aws_input = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        println!("Input: {}", aws_input);

        let aws_rule = config
            .rules
            .iter()
            .find(|r| r.id == "aws-access-token")
            .unwrap();
        if let Some(aws_regex_pattern) = &aws_rule.regex {
            if let Ok(regex) = Regex::new(aws_regex_pattern) {
                for mat in regex.find_iter(aws_input) {
                    println!("AWS Match: '{}'", mat.as_str());
                    if let Some(captures) = regex.captures(mat.as_str()) {
                        for (i, cap) in captures.iter().enumerate() {
                            if let Some(cap) = cap {
                                println!("  AWS Capture {}: '{}'", i, cap.as_str());
                            }
                        }
                    }
                }
            }
        } else {
            println!("AWS rule has no regex pattern");
        }
    }

    #[test]
    fn test_working_api_key_patterns() {
        let config = &*GITLEAKS_CONFIG;
        let generic_rule = config
            .rules
            .iter()
            .find(|r| r.id == "generic-api-key")
            .unwrap();

        // Get the compiled regex
        let regex = generic_rule
            .compiled_regex
            .as_ref()
            .expect("Regex should be compiled");

        // Create test patterns that should match the regex structure
        let test_inputs = vec![
            // Pattern: prefix + keyword + separator + value + terminator
            "myapp_api_key = \"abc123def456ghi789jklmnop\"",
            "export SECRET_TOKEN=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF",
            "app.auth.password: 9k2L8pMvB3nQ7rX1ZdF5GhJwY4AsPo6C8mN",
            "config.access_key=\"RaNd0mH1ghEnTr0pyV4luE567890abcdef\";",
            "DB_CREDENTIALS=xy9mP2nQ8rT4vW7yZ3cF6hJ1lN5sAdefghij",
        ];

        for input in test_inputs {
            println!("\nTesting: '{}'", input);

            let matches: Vec<_> = regex.find_iter(input).collect();
            println!("  Matches found: {}", matches.len());

            for (i, mat) in matches.iter().enumerate() {
                println!("  Match {}: '{}'", i, mat.as_str());

                if let Some(captures) = regex.captures(mat.as_str()) {
                    for (j, cap) in captures.iter().enumerate() {
                        if let Some(cap) = cap {
                            println!("    Capture {}: '{}'", j, cap.as_str());
                            if j == 1 {
                                let entropy = calculate_entropy(cap.as_str());
                                println!("    Entropy: {:.2} (threshold: 3.5)", entropy);

                                // Also check if it would be allowed by allowlists
                                let allowed = should_allow_match(
                                    input,
                                    None,
                                    mat.as_str(),
                                    mat.start(),
                                    mat.end(),
                                    generic_rule,
                                    &config.allowlist,
                                );
                                println!("    Allowed by allowlist: {}", allowed);
                            }
                        }
                    }
                }
            }

            // Test the full redact_secrets function
            let result = redact_secrets(input, None, &HashMap::new(), false);
            println!(
                "  Full function detected: {} secrets",
                result.redaction_map.len()
            );
            if result.redaction_map.len() > 0 {
                println!("  Redacted result: {}", result.redacted_string);
            }
        }
    }

    #[test]
    fn test_regex_components() {
        // Test individual components of the generic API key regex
        let test_input = "export API_KEY=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF";
        println!("Testing input: {}", test_input);

        // Test simpler regex patterns step by step
        let test_patterns = vec![
            (r"API_KEY", "Simple keyword match"),
            (r"(?i)api_key", "Case insensitive keyword"),
            (r"(?i).*key.*", "Any text with 'key'"),
            (r"(?i).*key\s*=", "Key with equals"),
            (r"(?i).*key\s*=\s*\w+", "Key with value"),
            (
                r"(?i)[\w.-]*(?:key).*?=.*?(\w{10,})",
                "Complex pattern with capture",
            ),
        ];

        for (pattern, description) in test_patterns {
            println!("\nTesting pattern: {} ({})", pattern, description);

            match Regex::new(pattern) {
                Ok(regex) => {
                    if regex.is_match(test_input) {
                        println!("  ✓ MATCHES");
                        for mat in regex.find_iter(test_input) {
                            println!("    Full match: '{}'", mat.as_str());
                        }
                        if let Some(captures) = regex.captures(test_input) {
                            for (i, cap) in captures.iter().enumerate() {
                                if let Some(cap) = cap {
                                    println!("    Capture {}: '{}'", i, cap.as_str());
                                }
                            }
                        }
                    } else {
                        println!("  ✗ NO MATCH");
                    }
                }
                Err(e) => println!("  Error: {}", e),
            }
        }

        // Test if there's an issue with the actual gitleaks regex compilation
        let config = &*GITLEAKS_CONFIG;
        let generic_rule = config
            .rules
            .iter()
            .find(|r| r.id == "generic-api-key")
            .unwrap();

        println!("\nTesting actual gitleaks regex:");
        if let Some(regex_pattern) = &generic_rule.regex {
            match Regex::new(regex_pattern) {
                Ok(regex) => {
                    println!("  ✓ Regex compiles successfully");
                    println!("  Testing against: {}", test_input);
                    if regex.is_match(test_input) {
                        println!("  ✓ MATCHES");
                    } else {
                        println!("  ✗ NO MATCH");
                    }
                }
                Err(e) => println!("  ✗ Regex compilation error: {}", e),
            }
        } else {
            println!("  Rule has no regex pattern (path-based rule)");
        }
    }

    #[test]
    fn test_comprehensive_secrets_redaction() {
        let input = r#"
# Configuration file with various secrets
export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7REALKEY
export GITHUB_TOKEN=ghp_1234567890abcdef1234567890abcdef12345678
export API_KEY=abc123def456ghi789jklmnop
export SECRET_TOKEN=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF
export PASSWORD=supersecretpassword123456

# Some normal configuration
export DEBUG=true
export PORT=3000
"#;

        println!("Original input:\n{}", input);

        let result = redact_secrets(input, None, &HashMap::new(), false);

        println!("Redacted output:\n{}", result.redacted_string);
        println!("\nDetected {} secrets:", result.redaction_map.len());
        for (key, value) in &result.redaction_map {
            println!("  {} -> {}", key, value);
        }

        // Should detect at least 5 secrets: AWS key, GitHub token, API key, secret token, password
        assert!(
            result.redaction_map.len() >= 5,
            "Should detect at least 5 secrets, found: {}",
            result.redaction_map.len()
        );

        // Verify specific secrets are redacted
        assert!(!result.redacted_string.contains("AKIAIOSFODNN7REALKEY"));
        assert!(
            !result
                .redacted_string
                .contains("ghp_1234567890abcdef1234567890abcdef12345678")
        );
        assert!(!result.redacted_string.contains("abc123def456ghi789jklmnop"));

        // Verify normal config is preserved
        assert!(result.redacted_string.contains("DEBUG=true"));
        assert!(result.redacted_string.contains("PORT=3000"));
    }

    // Helper function for keyword validation tests
    fn count_rules_that_would_process(input: &str) -> Vec<String> {
        let config = &*GITLEAKS_CONFIG;
        let mut rules = Vec::new();

        for rule in &config.rules {
            if rule.keywords.is_empty() || contains_any_keyword(input, &rule.keywords) {
                rules.push(rule.id.clone());
            }
        }

        rules
    }

    #[test]
    fn test_keyword_filtering() {
        println!("=== TESTING KEYWORD FILTERING ===");

        let config = &*GITLEAKS_CONFIG;

        // Find a rule that has keywords (like generic-api-key)
        let generic_rule = config
            .rules
            .iter()
            .find(|r| r.id == "generic-api-key")
            .unwrap();
        println!("Generic API Key rule keywords: {:?}", generic_rule.keywords);

        // Test 1: Input with keywords should be processed
        let input_with_keywords = "export API_KEY=abc123def456ghi789jklmnop";
        let result1 = redact_secrets(input_with_keywords, None, &HashMap::new(), false);
        println!("\nTest 1 - Input WITH keywords:");
        println!("  Input: {}", input_with_keywords);
        println!(
            "  Keywords present: {}",
            contains_any_keyword(input_with_keywords, &generic_rule.keywords)
        );
        println!("  Secrets detected: {}", result1.redaction_map.len());

        // Test 2: Input without any keywords should NOT be processed for that rule
        let input_without_keywords = "export DATABASE_URL=postgresql://user:pass@localhost/db";
        let result2 = redact_secrets(input_without_keywords, None, &HashMap::new(), false);
        println!("\nTest 2 - Input WITHOUT generic-api-key keywords:");
        println!("  Input: {}", input_without_keywords);
        println!(
            "  Keywords present: {}",
            contains_any_keyword(input_without_keywords, &generic_rule.keywords)
        );
        println!("  Secrets detected: {}", result2.redaction_map.len());

        // Test 3: Input with different rule's keywords (AWS)
        let aws_rule = config
            .rules
            .iter()
            .find(|r| r.id == "aws-access-token")
            .unwrap();
        let aws_input = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let result3 = redact_secrets(aws_input, None, &HashMap::new(), false);
        println!("\nTest 3 - AWS input:");
        println!("  Input: {}", aws_input);
        println!("  AWS rule keywords: {:?}", aws_rule.keywords);
        println!(
            "  Keywords present: {}",
            contains_any_keyword(aws_input, &aws_rule.keywords)
        );
        println!("  Secrets detected: {}", result3.redaction_map.len());

        // Validate that keyword filtering is working
        assert!(
            contains_any_keyword(input_with_keywords, &generic_rule.keywords),
            "API_KEY input should contain generic-api-key keywords"
        );
        assert!(
            !contains_any_keyword(input_without_keywords, &generic_rule.keywords),
            "DATABASE_URL input should NOT contain generic-api-key keywords"
        );
        assert!(
            contains_any_keyword(aws_input, &aws_rule.keywords),
            "AWS input should contain AWS rule keywords"
        );
    }

    #[test]
    fn test_keyword_optimization_performance() {
        println!("=== TESTING KEYWORD OPTIMIZATION PERFORMANCE ===");

        let config = &*GITLEAKS_CONFIG;

        // Test case 1: Input with NO keywords for any rule should be very fast
        let no_keywords_input = "export DATABASE_CONNECTION=some_long_connection_string_that_has_no_common_secret_keywords";
        println!("Testing input with no secret keywords:");
        println!("  Input: {}", no_keywords_input);

        let mut keyword_matches = 0;
        for rule in &config.rules {
            if contains_any_keyword(no_keywords_input, &rule.keywords) {
                keyword_matches += 1;
                println!("  Rule '{}' keywords match: {:?}", rule.id, rule.keywords);
            }
        }
        println!(
            "  Rules with matching keywords: {} out of {}",
            keyword_matches,
            config.rules.len()
        );

        let result = redact_secrets(no_keywords_input, None, &HashMap::new(), false);
        println!("  Secrets detected: {}", result.redaction_map.len());

        // Test case 2: Input with specific keywords should only process relevant rules
        let specific_keywords_input = "export GITHUB_TOKEN=ghp_1234567890abcdef";
        println!("\nTesting input with specific keywords (github):");
        println!("  Input: {}", specific_keywords_input);

        let mut matching_rules = Vec::new();
        for rule in &config.rules {
            if contains_any_keyword(specific_keywords_input, &rule.keywords) {
                matching_rules.push(&rule.id);
            }
        }
        println!("  Rules that would be processed: {:?}", matching_rules);

        let result = redact_secrets(specific_keywords_input, None, &HashMap::new(), false);
        println!("  Secrets detected: {}", result.redaction_map.len());

        // Test case 3: Verify that rules without keywords are always processed
        let rules_without_keywords: Vec<_> = config
            .rules
            .iter()
            .filter(|rule| rule.keywords.is_empty())
            .collect();
        println!(
            "\nRules without keywords (always processed): {}",
            rules_without_keywords.len()
        );
        for rule in &rules_without_keywords {
            println!("  - {}", rule.id);
        }

        // Assertions
        assert!(
            keyword_matches < config.rules.len(),
            "Input with no keywords should not match all rules"
        );
        assert!(
            !matching_rules.is_empty(),
            "GitHub token input should match some rules"
        );
        assert!(
            matching_rules.contains(&&"github-pat".to_string())
                || matching_rules
                    .iter()
                    .any(|rule_id| rule_id.contains("github")),
            "GitHub token should match GitHub-related rules"
        );
    }

    #[test]
    fn test_keyword_filtering_efficiency() {
        println!("=== KEYWORD FILTERING EFFICIENCY TEST ===");

        let config = &*GITLEAKS_CONFIG;
        println!("Total rules in config: {}", config.rules.len());

        // Test with input that has NO matching keywords
        let non_secret_input = "export DATABASE_URL=localhost PORT=3000 DEBUG=true TIMEOUT=30";
        println!("\nTesting non-secret input: {}", non_secret_input);

        let mut rules_skipped = 0;
        let mut rules_processed = 0;

        for rule in &config.rules {
            if rule.keywords.is_empty() {
                rules_processed += 1;
            } else if contains_any_keyword(non_secret_input, &rule.keywords) {
                rules_processed += 1;
            } else {
                rules_skipped += 1;
            }
        }

        println!(
            "  Rules skipped due to keyword filtering: {}",
            rules_skipped
        );
        println!("  Rules that would be processed: {}", rules_processed);
        println!(
            "  Efficiency gain: {:.1}% of rules skipped",
            (rules_skipped as f64 / config.rules.len() as f64) * 100.0
        );

        // Verify no secrets are detected
        let result = redact_secrets(non_secret_input, None, &HashMap::new(), false);
        println!("  Secrets detected: {}", result.redaction_map.len());

        // Now test with input that has relevant keywords
        let secret_input =
            "export API_KEY=abc123def456ghi789jklmnop SECRET_TOKEN=xyz789uvw012rst345def678";
        println!("\nTesting input WITH secret keywords:");
        println!("  Input: {}", secret_input);

        let mut rules_with_keywords = 0;
        for rule in &config.rules {
            if contains_any_keyword(secret_input, &rule.keywords) {
                rules_with_keywords += 1;
            }
        }

        println!("  Rules that match keywords: {}", rules_with_keywords);

        let result = redact_secrets(secret_input, None, &HashMap::new(), false);
        println!("  Secrets detected: {}", result.redaction_map.len());

        // Assertions
        assert!(
            rules_skipped > 0,
            "Should skip at least some rules for non-secret input"
        );
        assert!(
            rules_with_keywords > 0,
            "Should find matching rules for secret input"
        );
        assert!(
            result.redaction_map.len() >= 1,
            "Should detect at least one secret"
        );
    }

    #[test]
    fn test_keyword_validation_summary() {
        println!("=== KEYWORD VALIDATION SUMMARY ===");

        let config = &*GITLEAKS_CONFIG;
        let total_rules = config.rules.len();
        println!("Total rules in gitleaks config: {}", total_rules);

        // Test no keywords - should skip most rules
        let no_keyword_input = "export DATABASE_URL=localhost PORT=3000";
        println!("\n--- No keywords - should skip all rules ---");
        println!("Input: {}", no_keyword_input);

        let no_keyword_rules = count_rules_that_would_process(no_keyword_input);
        println!(
            "Rules that would be processed: {} out of {}",
            no_keyword_rules.len(),
            total_rules
        );
        println!("  Rules: {:?}", no_keyword_rules);

        let no_keyword_secrets = detect_secrets(no_keyword_input, None, false);
        println!(
            "Secrets detected: {} (expected: 0)",
            no_keyword_secrets.len()
        );
        assert_eq!(no_keyword_secrets.len(), 0, "Should not detect any secrets");
        println!("✅ Test passed");

        // Test API keyword - should process generic-api-key rule
        let api_input = "export API_KEY=abc123def456ghi789jklmnop";
        println!("\n--- API keyword - should process generic-api-key rule ---");
        println!("Input: {}", api_input);

        let api_rules = count_rules_that_would_process(api_input);
        println!(
            "Rules that would be processed: {} out of {}",
            api_rules.len(),
            total_rules
        );
        println!("  Rules: {:?}", api_rules);

        let api_secrets = detect_secrets(api_input, None, false);
        println!("Secrets detected: {} (expected: 1)", api_secrets.len());
        assert!(api_secrets.len() >= 1, "Should detect at least 1 secrets");
        println!("✅ Test passed");

        // Test AWS keyword - should process aws-access-token rule
        // Use a realistic AWS key that matches the pattern [A-Z2-7]{16}
        let aws_input = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7REALKEY";
        println!("\n--- AWS keyword - should process aws-access-token rule ---");
        println!("Input: {}", aws_input);

        let aws_rules = count_rules_that_would_process(aws_input);
        println!(
            "Rules that would be processed: {} out of {}",
            aws_rules.len(),
            total_rules
        );
        println!("  Rules: {:?}", aws_rules);

        let aws_secrets = detect_secrets(aws_input, None, false);
        println!("Secrets detected: {} (expected: 1)", aws_secrets.len());

        // Should detect AWS key
        assert!(aws_secrets.len() >= 1, "Should detect at least 1 secrets");
        println!("✅ Test passed");
    }

    #[test]
    fn test_debug_missing_secrets() {
        println!("=== DEBUGGING MISSING SECRETS ===");

        let test_cases = vec![
            "SECRET_TOKEN=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF",
            "PASSWORD=supersecretpassword123456",
        ];

        for input in test_cases {
            println!("\nTesting: {}", input);

            // Check entropy first
            let parts: Vec<&str> = input.split('=').collect();
            if parts.len() == 2 {
                let secret_value = parts[1];
                let entropy = calculate_entropy(secret_value);
                println!("  Secret value: '{}'", secret_value);
                println!("  Entropy: {:.2} (threshold: 3.5)", entropy);

                if entropy >= 3.5 {
                    println!("  ✓ Entropy check PASSED");
                } else {
                    println!("  ✗ Entropy check FAILED - this is why it's not detected");
                }
            }

            // Test the fallback regex directly
            if let Ok(regex) = create_simple_api_key_regex() {
                println!("  Testing fallback regex:");
                if regex.is_match(input) {
                    println!("    ✓ Fallback regex MATCHES");
                    for mat in regex.find_iter(input) {
                        println!("    Match: '{}'", mat.as_str());
                        if let Some(captures) = regex.captures(mat.as_str()) {
                            for (i, cap) in captures.iter().enumerate() {
                                if let Some(cap) = cap {
                                    println!("      Capture {}: '{}'", i, cap.as_str());
                                }
                            }
                        }

                        // Test allowlist checking
                        let config = &*GITLEAKS_CONFIG;
                        let generic_rule = config
                            .rules
                            .iter()
                            .find(|r| r.id == "generic-api-key")
                            .unwrap();
                        let allowed = should_allow_match(
                            input,
                            None,
                            mat.as_str(),
                            mat.start(),
                            mat.end(),
                            generic_rule,
                            &config.allowlist,
                        );
                        println!("      Allowed by allowlist: {}", allowed);
                        if allowed {
                            println!(
                                "      ✗ FILTERED OUT by allowlist - this is why it's not detected"
                            );
                        }
                    }
                } else {
                    println!("    ✗ Fallback regex does NOT match");
                }
            }

            // Test full detection
            let result = redact_secrets(input, None, &HashMap::new(), false);
            println!(
                "  Full detection result: {} secrets",
                result.redaction_map.len()
            );
        }
    }

    #[test]
    fn test_debug_allowlist_filtering() {
        println!("=== DEBUGGING ALLOWLIST FILTERING ===");

        let test_cases = vec![
            "SECRET_TOKEN=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF",
            "PASSWORD=supersecretpassword123456",
        ];

        let config = &*GITLEAKS_CONFIG;
        let generic_rule = config
            .rules
            .iter()
            .find(|r| r.id == "generic-api-key")
            .unwrap();

        for input in test_cases {
            println!("\nAnalyzing: {}", input);

            if let Ok(regex) = create_simple_api_key_regex() {
                for mat in regex.find_iter(input) {
                    let match_text = mat.as_str();
                    println!("  Match: '{}'", match_text);

                    // Test global allowlist
                    if let Some(global_allowlist) = &config.allowlist {
                        println!("  Checking global allowlist:");

                        // Test global regex patterns
                        if let Some(regexes) = &global_allowlist.regexes {
                            for (i, pattern) in regexes.iter().enumerate() {
                                if let Ok(regex) = Regex::new(pattern) {
                                    if regex.is_match(match_text) {
                                        println!(
                                            "    ✗ FILTERED by global regex {}: '{}'",
                                            i, pattern
                                        );
                                    }
                                }
                            }
                        }

                        // Test global stopwords
                        if let Some(stopwords) = &global_allowlist.stopwords {
                            for stopword in stopwords {
                                if match_text.to_lowercase().contains(&stopword.to_lowercase()) {
                                    println!("    ✗ FILTERED by global stopword: '{}'", stopword);
                                }
                            }
                        }
                    }

                    // Test rule-specific allowlists
                    if let Some(rule_allowlists) = &generic_rule.allowlists {
                        for (rule_idx, allowlist) in rule_allowlists.iter().enumerate() {
                            println!("  Checking rule allowlist {}:", rule_idx);

                            // Test rule regex patterns
                            if let Some(regexes) = &allowlist.regexes {
                                for (i, pattern) in regexes.iter().enumerate() {
                                    if let Ok(regex) = Regex::new(pattern) {
                                        if regex.is_match(match_text) {
                                            println!(
                                                "    ✗ FILTERED by rule regex {}: '{}'",
                                                i, pattern
                                            );
                                        }
                                    }
                                }
                            }

                            // Test rule stopwords
                            if let Some(stopwords) = &allowlist.stopwords {
                                for stopword in stopwords {
                                    if match_text.to_lowercase().contains(&stopword.to_lowercase())
                                    {
                                        println!("    ✗ FILTERED by rule stopword: '{}'", stopword);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_debug_new_allowlist_logic() {
        println!("=== DEBUGGING NEW ALLOWLIST LOGIC ===");

        let test_cases = vec![
            "SECRET_TOKEN=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF",
            "PASSWORD=supersecretpassword123456",
            "PASSWORD=password123", // Should be filtered
            "API_KEY=example_key",  // Should be filtered
        ];

        let config = &*GITLEAKS_CONFIG;
        let generic_rule = config
            .rules
            .iter()
            .find(|r| r.id == "generic-api-key")
            .unwrap();

        for input in test_cases {
            println!("\nTesting: {}", input);

            if let Ok(regex) = create_simple_api_key_regex() {
                for mat in regex.find_iter(input) {
                    let match_text = mat.as_str();
                    println!("  Match: '{}'", match_text);

                    // Parse the KEY=VALUE
                    if let Some(equals_pos) = match_text.find('=') {
                        let value = &match_text[equals_pos + 1..];
                        println!("    Value: '{}'", value);

                        // Test specific stopwords
                        let test_stopwords = ["token", "password", "super", "word"];
                        for stopword in test_stopwords {
                            let value_lower = value.to_lowercase();
                            let stopword_lower = stopword.to_lowercase();

                            if value_lower == stopword_lower {
                                println!("    '{}' - Exact match: YES", stopword);
                            } else if value.len() < 15 && value_lower.contains(&stopword_lower) {
                                let without_stopword = value_lower.replace(&stopword_lower, "");
                                let is_simple = without_stopword.chars().all(|c| {
                                    c.is_ascii_digit() || "!@#$%^&*()_+-=[]{}|;:,.<>?".contains(c)
                                });
                                println!(
                                    "    '{}' - Short+contains: len={}, without='{}', simple={}",
                                    stopword,
                                    value.len(),
                                    without_stopword,
                                    is_simple
                                );
                            } else {
                                println!("    '{}' - No filter", stopword);
                            }
                        }
                    }

                    // Test the actual allowlist
                    if let Some(rule_allowlists) = &generic_rule.allowlists {
                        for (rule_idx, allowlist) in rule_allowlists.iter().enumerate() {
                            let allowed = is_allowed_by_rule_allowlist(
                                input,
                                None,
                                match_text,
                                mat.start(),
                                mat.end(),
                                allowlist,
                            );
                            println!("  Rule allowlist {}: allowed = {}", rule_idx, allowed);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_redact_password_basic() {
        let content = "User password is supersecret123 and should be hidden";
        let password = "supersecret123";
        let result = redact_password(content, password, &HashMap::new());

        // Should redact the password
        assert!(!result.redacted_string.contains(password));
        assert!(
            result
                .redacted_string
                .contains("[REDACTED_SECRET:password:")
        );
        assert_eq!(result.redaction_map.len(), 1);

        // The redaction map should contain our password
        let redacted_password = result.redaction_map.values().next().unwrap();
        assert_eq!(redacted_password, password);
    }

    #[test]
    fn test_redact_password_empty() {
        let content = "Some content without password";
        let password = "";
        let result = redact_password(content, password, &HashMap::new());

        // Should not change anything
        assert_eq!(result.redacted_string, content);
        assert!(result.redaction_map.is_empty());
    }

    #[test]
    fn test_redact_password_multiple_occurrences() {
        let content = "Password is mypass123 and again mypass123 appears here";
        let password = "mypass123";
        let result = redact_password(content, password, &HashMap::new());

        // Should redact both occurrences with the same key
        assert!(!result.redacted_string.contains(password));
        assert_eq!(result.redaction_map.len(), 1);

        // Count redaction keys in the result
        let redaction_key = result.redaction_map.keys().next().unwrap();
        let count = result.redacted_string.matches(redaction_key).count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_redact_password_reuse_existing_key() {
        // Start with an existing redaction map
        let mut existing_map = HashMap::new();
        existing_map.insert(
            "[REDACTED_SECRET:password:abc123]".to_string(),
            "mypassword".to_string(),
        );

        let content = "The password mypassword should use existing key";
        let password = "mypassword";
        let result = redact_password(content, password, &existing_map);

        // Should reuse the existing key
        assert_eq!(result.redaction_map.len(), 1);
        assert!(
            result
                .redaction_map
                .contains_key("[REDACTED_SECRET:password:abc123]")
        );
        assert!(
            result
                .redacted_string
                .contains("[REDACTED_SECRET:password:abc123]")
        );
    }

    #[test]
    fn test_redact_password_with_existing_different_secrets() {
        // Start with an existing redaction map containing different secrets
        let mut existing_map = HashMap::new();
        existing_map.insert(
            "[REDACTED_SECRET:api-key:xyz789]".to_string(),
            "some_api_key".to_string(),
        );

        let content = "API key is some_api_key and password is newpassword123";
        let password = "newpassword123";
        let result = redact_password(content, password, &existing_map);

        // Should preserve existing mapping and add new one
        assert_eq!(result.redaction_map.len(), 2);
        assert!(
            result
                .redaction_map
                .contains_key("[REDACTED_SECRET:api-key:xyz789]")
        );
        assert!(
            result
                .redaction_map
                .get("[REDACTED_SECRET:api-key:xyz789]")
                .unwrap()
                == "some_api_key"
        );

        // Should add new password mapping
        let new_keys: Vec<_> = result
            .redaction_map
            .keys()
            .filter(|k| k.contains("password"))
            .collect();
        assert_eq!(new_keys.len(), 1);
        let password_key = new_keys[0];
        assert_eq!(
            result.redaction_map.get(password_key).unwrap(),
            "newpassword123"
        );
    }

    #[test]
    fn test_redact_password_no_match() {
        let content = "This content has no matching password";
        let password = "notfound";
        let result = redact_password(content, password, &HashMap::new());

        // Should still create a redaction key but content unchanged
        assert_eq!(result.redacted_string, content);
        assert_eq!(result.redaction_map.len(), 1);
        assert_eq!(result.redaction_map.values().next().unwrap(), "notfound");
    }

    #[test]
    fn test_redact_password_integration_with_restore() {
        let content = "Login with username admin and password secret456";
        let password = "secret456";
        let result = redact_password(content, password, &HashMap::new());

        // Redact the password
        assert!(!result.redacted_string.contains(password));
        assert!(result.redacted_string.contains("username admin"));

        // Restore should bring back the original
        let restored = restore_secrets(&result.redacted_string, &result.redaction_map);
        assert_eq!(restored, content);
    }

    #[test]
    fn test_shell_password_redaction_scenario() {
        // Simulate the exact scenario from the test: 
        // 1. User pastes password
        let password = "SuperSecret123!Password";
        let redaction_result1 = redact_password("", password, &HashMap::new());
        println!("After storing password, map has {} entries", redaction_result1.redaction_map.len());
        
        // 2. Shell command echoes the password in output
        let shell_output = "Attempting to echo password: SuperSecret123!Password";
        let redaction_result2 = redact_secrets(shell_output, None, &redaction_result1.redaction_map, false);
        
        println!("Shell output before: {}", shell_output);
        println!("Shell output after: {}", redaction_result2.redacted_string);
        println!("Redaction map: {:?}", redaction_result2.redaction_map);
        
        // The password should be redacted
        assert!(!redaction_result2.redacted_string.contains(password), 
                "Password should not appear in plain text in output");
        assert!(redaction_result2.redacted_string.contains("[REDACTED_SECRET:password:"),
                "Output should contain redaction marker");
    }

    #[test]
    fn test_redact_secrets_with_existing_redaction_map() {
        // Test that secrets in the existing redaction map get redacted even if not detected by detect_secrets
        let content = "The secret value is mysecretvalue123 and another is anothersecret456";

        // First, test with empty map to prove the secret wouldn't normally be redacted
        let result_empty = redact_secrets(content, None, &HashMap::new(), false);

        // Verify that mysecretvalue123 is NOT redacted when using empty map
        assert!(result_empty.redacted_string.contains("mysecretvalue123"));
        // Now create an existing redaction map with one of the secrets
        let mut existing_redaction_map = HashMap::new();
        existing_redaction_map.insert(
            "[REDACTED_SECRET:manual:abc123]".to_string(),
            "mysecretvalue123".to_string(),
        );

        let result = redact_secrets(content, None, &existing_redaction_map, false);

        // The secret from the existing map should be redacted
        assert!(
            result
                .redacted_string
                .contains("[REDACTED_SECRET:manual:abc123]")
        );
        assert!(!result.redacted_string.contains("mysecretvalue123"));

        // The redaction map should contain the existing mapping
        assert!(
            result
                .redaction_map
                .contains_key("[REDACTED_SECRET:manual:abc123]")
        );
        assert_eq!(
            result
                .redaction_map
                .get("[REDACTED_SECRET:manual:abc123]")
                .unwrap(),
            "mysecretvalue123"
        );
    }
}
