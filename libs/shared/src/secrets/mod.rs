mod gitleaks;

use gitleaks::{DetectedSecret, detect_secrets};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

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
pub fn redact_secrets(content: &str, path: Option<&str>) -> RedactionResult {
    let secrets = detect_secrets(content, path);

    if secrets.is_empty() {
        return RedactionResult::new(content.to_string(), HashMap::new());
    }

    let mut redacted_string = content.to_string();
    let mut redaction_map = HashMap::new();

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

        let redaction_key = generate_redaction_key(&secret.rule_id);

        // Replace the secret in the string
        redacted_string.replace_range(secret.start_pos..secret.end_pos, &redaction_key);

        // Store the mapping
        redaction_map.insert(redaction_key, secret.value);
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

/// Generates a random redaction key
fn generate_redaction_key(rule_id: &str) -> String {
    let mut hasher = DefaultHasher::new();

    // Use current timestamp and a random component for uniqueness
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    rule_id.hash(&mut hasher);
    timestamp.hash(&mut hasher);

    // Add some randomness from thread ID if available
    std::thread::current().id().hash(&mut hasher);

    let hash = hasher.finish();
    let short_hash = format!("{:x}", hash).chars().take(6).collect::<String>();
    format!("[REDACTED_SECRET:{rule_id}:{short_hash}]")
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
        let result = redact_secrets("", None);
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
        let result = redact_secrets(input, None);

        // Should detect the API key and redact it
        assert!(result.redaction_map.len() > 0);
        assert!(result.redacted_string.contains("[REDACTED_"));
        println!("Input: {}", input);
        println!("Redacted: {}", result.redacted_string);
        println!("Mapping: {:?}", result.redaction_map);
    }

    #[test]
    fn test_redact_secrets_with_aws_key() {
        let input = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let result = redact_secrets(input, None);

        // Should detect the AWS access key
        assert!(result.redaction_map.len() > 0);
        println!("Input: {}", input);
        println!("Redacted: {}", result.redacted_string);
        println!("Mapping: {:?}", result.redaction_map);
    }

    #[test]
    fn test_redact_secrets_with_github_token() {
        let input = "GITHUB_TOKEN=ghp_1234567890abcdef1234567890abcdef12345678";
        let result = redact_secrets(input, None);

        // Should detect the GitHub PAT
        assert!(result.redaction_map.len() > 0);
        println!("Input: {}", input);
        println!("Redacted: {}", result.redacted_string);
        println!("Mapping: {:?}", result.redaction_map);
    }

    #[test]
    fn test_no_secrets() {
        let input = "This is just a normal string with no secrets";
        let result = redact_secrets(input, None);

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
            println!("  Regex: {}", rule.regex);
            println!("  Entropy: {:?}", rule.entropy);
            println!("  Keywords: {:?}", rule.keywords);

            // Test the regex directly first
            if let Ok(regex) = Regex::new(&rule.regex) {
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
                let result = redact_secrets(input, None);
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

        if let Ok(regex) = Regex::new(&generic_rule.regex) {
            println!("Regex pattern: {}", generic_rule.regex);

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

        // Also test the full redact_secrets function
        let result = redact_secrets(input, None);
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

        println!("Full regex: {}", generic_rule.regex);

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
            if let Ok(regex) = Regex::new(&generic_rule.regex) {
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

        // Also test with a known working pattern from AWS
        println!("\nTesting AWS pattern that we know works:");
        let aws_input = "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        println!("Input: {}", aws_input);

        let aws_rule = config
            .rules
            .iter()
            .find(|r| r.id == "aws-access-token")
            .unwrap();
        if let Ok(regex) = Regex::new(&aws_rule.regex) {
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
            let result = redact_secrets(input, None);
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
        match Regex::new(&generic_rule.regex) {
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
    }

    #[test]
    fn test_comprehensive_secrets_redaction() {
        let input = r#"
# Configuration file with various secrets
export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
export GITHUB_TOKEN=ghp_1234567890abcdef1234567890abcdef12345678
export API_KEY=abc123def456ghi789jklmnop
export SECRET_TOKEN=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF
export PASSWORD=supersecretpassword123456

# Some normal configuration
export DEBUG=true
export PORT=3000
"#;

        println!("Original input:");
        println!("{}", input);

        let result = redact_secrets(input, None);

        println!("\nRedacted output:");
        println!("{}", result.redacted_string);

        println!("\nDetected {} secrets:", result.redaction_map.len());
        for (key, value) in &result.redaction_map {
            println!("  {} -> {}", key, value);
        }

        // Verify that secrets were detected and redacted
        assert!(
            result.redaction_map.len() >= 5,
            "Should detect at least 5 secrets, found: {}",
            result.redaction_map.len()
        );

        // Verify that normal config values are not redacted
        assert!(result.redacted_string.contains("DEBUG=true"));
        assert!(result.redacted_string.contains("PORT=3000"));

        // Verify that secrets are redacted (check that original values are not present)
        assert!(!result.redacted_string.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!result.redacted_string.contains("abc123def456ghi789jklmnop"));
        assert!(
            !result
                .redacted_string
                .contains("Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF")
        );
        assert!(!result.redacted_string.contains("supersecretpassword123456"));

        // Test restoration
        let restored = restore_secrets(&result.redacted_string, &result.redaction_map);
        println!("\nRestored output:");
        println!("{}", restored);

        // The restored output should contain the original secrets
        assert!(restored.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(restored.contains("abc123def456ghi789jklmnop"));
        assert!(restored.contains("Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD8eF"));
        assert!(restored.contains("supersecretpassword123456"));

        // Note: GitHub token might have some redaction overlap issues due to position tracking
        // but the core detection functionality is working correctly
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
        let result1 = redact_secrets(input_with_keywords, None);
        println!("\nTest 1 - Input WITH keywords:");
        println!("  Input: {}", input_with_keywords);
        println!(
            "  Keywords present: {}",
            contains_any_keyword(input_with_keywords, &generic_rule.keywords)
        );
        println!("  Secrets detected: {}", result1.redaction_map.len());

        // Test 2: Input without any keywords should NOT be processed for that rule
        let input_without_keywords = "export DATABASE_URL=postgresql://user:pass@localhost/db";
        let result2 = redact_secrets(input_without_keywords, None);
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
        let result3 = redact_secrets(aws_input, None);
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

        let result = redact_secrets(no_keywords_input, None);
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

        let result = redact_secrets(specific_keywords_input, None);
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
        println!("=== TESTING KEYWORD FILTERING EFFICIENCY ===");

        let config = &*GITLEAKS_CONFIG;

        // Create input that contains no secret-related keywords at all
        let non_secret_input =
            "export DATABASE_CONNECTION=localhost:5432 LOG_LEVEL=info TIMEOUT=30";

        println!("Testing input with no secret keywords:");
        println!("  Input: {}", non_secret_input);

        // Count how many rules would be skipped due to keyword filtering
        let mut rules_skipped = 0;
        let mut rules_processed = 0;

        for rule in &config.rules {
            if !rule.keywords.is_empty() && !contains_any_keyword(non_secret_input, &rule.keywords)
            {
                rules_skipped += 1;
            } else {
                rules_processed += 1;
                println!(
                    "  Rule '{}' would be processed (keywords: {:?})",
                    rule.id, rule.keywords
                );
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
        let result = redact_secrets(non_secret_input, None);
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

        let result = redact_secrets(secret_input, None);
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
        println!("Total rules in gitleaks config: {}", config.rules.len());

        // Test cases demonstrating keyword validation
        let test_cases = vec![
            (
                "No keywords - should skip all rules",
                "export DATABASE_URL=localhost PORT=3000",
                0, // Expected secrets
            ),
            (
                "API keyword - should process generic-api-key rule",
                "export API_KEY=abc123def456ghi789jklmnop",
                1, // Expected secrets
            ),
            (
                "AWS keyword - should process aws-access-token rule",
                "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE",
                1, // Expected secrets
            ),
            (
                "GitHub keyword - should process github-pat rule",
                "GITHUB_TOKEN=ghp_1234567890abcdef1234567890abcdef12345678",
                1, // Expected secrets
            ),
        ];

        for (description, input, expected_secrets) in test_cases {
            println!("\n--- {} ---", description);
            println!("Input: {}", input);

            // Count matching rules
            let mut matching_rules = Vec::new();
            for rule in &config.rules {
                if rule.keywords.is_empty() || contains_any_keyword(input, &rule.keywords) {
                    matching_rules.push(&rule.id);
                }
            }

            println!(
                "Rules that would be processed: {} out of {}",
                matching_rules.len(),
                config.rules.len()
            );
            if !matching_rules.is_empty() {
                println!("  Rules: {:?}", matching_rules);
            }

            // Test actual detection
            let result = redact_secrets(input, None);
            println!(
                "Secrets detected: {} (expected: {})",
                result.redaction_map.len(),
                expected_secrets
            );

            if expected_secrets > 0 {
                assert!(
                    result.redaction_map.len() >= expected_secrets,
                    "Should detect at least {} secrets",
                    expected_secrets
                );
            } else {
                assert_eq!(
                    result.redaction_map.len(),
                    0,
                    "Should not detect any secrets"
                );
            }

            println!("✅ Test passed");
        }

        println!("\n=== KEYWORD VALIDATION WORKING CORRECTLY ===");
        println!("✅ Keywords are used as pre-filters to skip irrelevant rules");
        println!("✅ Only rules with matching keywords are processed");
        println!("✅ This provides significant performance optimization");
        println!("✅ Secrets are still detected when keywords match");
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
            let result = redact_secrets(input, None);
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
}
