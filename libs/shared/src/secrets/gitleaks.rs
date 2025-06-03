// Secret redaction implementation based on gitleaks (https://github.com/gitleaks/gitleaks)
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

#[allow(dead_code)]
/// Gitleaks configuration structures
#[derive(Debug, Deserialize, Clone)]
pub struct GitleaksConfig {
    pub title: Option<String>,
    pub allowlist: Option<GlobalAllowlist>,
    pub rules: Vec<Rule>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct GlobalAllowlist {
    pub description: Option<String>,
    pub paths: Option<Vec<String>>,
    pub regexes: Option<Vec<String>>,
    pub stopwords: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub id: String,
    pub description: String,
    pub regex: String,
    pub entropy: Option<f64>,
    pub keywords: Vec<String>,
    pub path: Option<String>,
    pub allowlists: Option<Vec<RuleAllowlist>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct RuleAllowlist {
    pub description: Option<String>,
    pub condition: Option<String>, // "AND" or default "OR"
    pub paths: Option<Vec<String>>,
    pub regexes: Option<Vec<String>>,
    pub stopwords: Option<Vec<String>>,
    #[serde(rename = "regexTarget")]
    pub regex_target: Option<String>, // "match", "line", etc.
}

/// Represents a detected secret with its position and value
#[derive(Debug, Clone)]
pub struct DetectedSecret {
    /// Detection rule id
    pub rule_id: String,
    /// The secret value
    pub value: String,
    /// Start position in the original string
    pub start_pos: usize,
    /// End position in the original string
    pub end_pos: usize,
}

/// A compiled rule with its pre-compiled regex
#[derive(Debug)]
pub struct CompiledRule {
    pub rule: Rule,
    pub regex: Regex,
    pub compiled_allowlists: Option<Vec<CompiledRuleAllowlist>>,
}

#[allow(dead_code)]
/// Pre-compiled allowlist with compiled regexes
#[derive(Debug)]
pub struct CompiledGlobalAllowlist {
    pub description: Option<String>,
    pub paths: Option<Vec<String>>,
    pub compiled_regexes: Vec<Regex>,
    pub stopwords: Option<Vec<String>>,
}

#[allow(dead_code)]
/// Pre-compiled rule allowlist with compiled regexes
#[derive(Debug)]
pub struct CompiledRuleAllowlist {
    pub description: Option<String>,
    pub condition: Option<String>, // "AND" or default "OR"
    pub paths: Option<Vec<String>>,
    pub compiled_regexes: Vec<Regex>,
    pub stopwords: Option<Vec<String>>,
    pub regex_target: Option<String>, // "match", "line", etc.
}

#[allow(dead_code)]
/// Pre-compiled gitleaks configuration with all regexes compiled
#[derive(Debug)]
pub struct CompiledGitleaksConfig {
    pub title: Option<String>,
    pub compiled_allowlist: Option<CompiledGlobalAllowlist>,
    pub compiled_rules: Vec<CompiledRule>,
}

/// Lazy-loaded gitleaks configuration
pub static GITLEAKS_CONFIG: Lazy<GitleaksConfig> = Lazy::new(|| {
    let config_str = include_str!("gitleaks.toml");
    toml::from_str(config_str).expect("Failed to parse gitleaks.toml")
});

/// Lazy-loaded compiled gitleaks configuration with pre-compiled regexes
pub static COMPILED_GITLEAKS_CONFIG: Lazy<CompiledGitleaksConfig> = Lazy::new(|| {
    let config = &*GITLEAKS_CONFIG;
    let mut compiled_rules = Vec::new();

    // Compile global allowlist regexes
    let compiled_allowlist = config.allowlist.as_ref().map(|allowlist| {
        let mut compiled_regexes = Vec::new();
        if let Some(regexes) = &allowlist.regexes {
            for pattern in regexes {
                if let Ok(regex) = Regex::new(pattern) {
                    compiled_regexes.push(regex);
                } else {
                    eprintln!("Warning: Failed to compile allowlist regex: {}", pattern);
                }
            }
        }

        CompiledGlobalAllowlist {
            description: allowlist.description.clone(),
            paths: allowlist.paths.clone(),
            compiled_regexes,
            stopwords: allowlist.stopwords.clone(),
        }
    });

    for rule in &config.rules {
        // Try to compile the regex, skip if it's too complex
        let regex = match Regex::new(&rule.regex) {
            Ok(regex) => regex,
            Err(e) => {
                // Handle regex compilation errors (e.g., size limit exceeded)
                eprintln!(
                    "Warning: Failed to compile regex for rule '{}': {}",
                    rule.id, e
                );

                // For generic-api-key, use a simpler fallback pattern
                if rule.id == "generic-api-key" {
                    if let Ok(simple_regex) = create_simple_api_key_regex() {
                        simple_regex
                    } else {
                        continue; // Skip this rule entirely
                    }
                } else {
                    continue; // Skip this rule entirely
                }
            }
        };

        // Compile rule allowlist regexes
        let compiled_allowlists = rule.allowlists.as_ref().map(|allowlists| {
            allowlists
                .iter()
                .map(|allowlist| {
                    let mut compiled_regexes = Vec::new();
                    if let Some(regexes) = &allowlist.regexes {
                        for pattern in regexes {
                            if let Ok(regex) = Regex::new(pattern) {
                                compiled_regexes.push(regex);
                            } else {
                                eprintln!(
                                    "Warning: Failed to compile rule allowlist regex: {}",
                                    pattern
                                );
                            }
                        }
                    }

                    CompiledRuleAllowlist {
                        description: allowlist.description.clone(),
                        condition: allowlist.condition.clone(),
                        paths: allowlist.paths.clone(),
                        compiled_regexes,
                        stopwords: allowlist.stopwords.clone(),
                        regex_target: allowlist.regex_target.clone(),
                    }
                })
                .collect()
        });

        compiled_rules.push(CompiledRule {
            rule: rule.clone(),
            regex,
            compiled_allowlists,
        });
    }

    CompiledGitleaksConfig {
        title: config.title.clone(),
        compiled_allowlist,
        compiled_rules,
    }
});

/// Detects secrets in the input string using gitleaks configuration
///
/// This implementation follows the gitleaks methodology:
/// 1. Apply regex rules to find potential secrets
/// 2. Check entropy thresholds to filter out low-entropy matches
/// 3. Apply allowlists to exclude known false positives
/// 4. Check keywords to ensure relevance
pub fn detect_secrets(input: &str, path: Option<&str>) -> Vec<DetectedSecret> {
    let mut detected_secrets = Vec::new();
    let config = &*COMPILED_GITLEAKS_CONFIG;

    // Apply each compiled rule from the configuration
    for compiled_rule in &config.compiled_rules {
        let rule = &compiled_rule.rule;
        let regex = &compiled_rule.regex;

        // Pre-filter: Skip rule if none of its keywords are present in the input
        if !rule.keywords.is_empty() && !contains_any_keyword(input, &rule.keywords) {
            continue;
        }

        // Find all matches for this rule using the pre-compiled regex
        for mat in regex.find_iter(input) {
            let match_text = mat.as_str();
            let start_pos = mat.start();
            let end_pos = mat.end();

            // Check if this match should be filtered out
            if should_allow_match(
                input,
                path,
                match_text,
                start_pos,
                end_pos,
                compiled_rule,
                &config.compiled_allowlist,
            ) {
                continue;
            }

            // Extract the captured secret value and its position
            let (secret_value, secret_start, secret_end) =
                if let Some(captures) = regex.captures_at(input, start_pos) {
                    // Try to get the first capture group, fallback to full match
                    if let Some(capture) = captures.get(1) {
                        // Capture positions are already relative to the full input
                        (capture.as_str().to_string(), capture.start(), capture.end())
                    } else {
                        (match_text.to_string(), start_pos, end_pos)
                    }
                } else {
                    (match_text.to_string(), start_pos, end_pos)
                };

            // Check entropy if specified - apply to the captured secret value, not the full match
            if let Some(entropy_threshold) = rule.entropy {
                let calculated_entropy = calculate_entropy(&secret_value);
                if calculated_entropy < entropy_threshold {
                    continue;
                }
            }

            detected_secrets.push(DetectedSecret {
                rule_id: rule.id.clone(),
                value: secret_value,
                start_pos: secret_start,
                end_pos: secret_end,
            });
        }
    }

    detected_secrets
}

/// Creates a simplified API key regex that works within Rust's regex engine limits
pub fn create_simple_api_key_regex() -> Result<Regex, regex::Error> {
    // The original Gitleaks generic pattern is too complex for Rust's regex engine.
    // We'll use a simpler but still effective pattern that captures the essence:
    // 1. Optional prefix (identifier)
    // 2. Keywords (access, auth, api, etc.)
    // 3. Optional suffix
    // 4. Assignment operators
    // 5. Optional quotes/spaces
    // 6. The actual secret value (captured)
    // 7. Terminator

    let pattern = r#"(?i)[\w.-]{0,30}?(?:access|auth|api|credential|creds|key|password|passwd|secret|token)[\w.-]{0,15}[\s'"]{0,3}(?:=|>|:{1,2}=|\|\||:|=>|\?=|,)[\s'"=]{0,3}([\w.=-]{10,80}|[a-z0-9][a-z0-9+/]{11,}={0,2})(?:[\s'";]|$)"#;
    Regex::new(pattern)
}

/// Calculate Shannon entropy for a string
///
/// Entropy measures the randomness/unpredictability of characters in a string.
/// Higher entropy suggests more randomness, which is characteristic of secrets.
pub fn calculate_entropy(text: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }

    let mut char_counts = std::collections::HashMap::new();
    let total_chars = text.len() as f64;

    // Count character frequencies
    for ch in text.chars() {
        *char_counts.entry(ch).or_insert(0u32) += 1;
    }

    // Calculate Shannon entropy: H = -Σ(p(x) * log2(p(x)))
    let mut entropy = 0.0;
    for &count in char_counts.values() {
        let probability = count as f64 / total_chars;
        if probability > 0.0 {
            entropy -= probability * probability.log2();
        }
    }

    entropy
}

/// Check if a match should be allowed (filtered out) based on allowlists
pub fn should_allow_match(
    input: &str,
    path: Option<&str>,
    match_text: &str,
    start_pos: usize,
    end_pos: usize,
    compiled_rule: &CompiledRule,
    global_allowlist: &Option<CompiledGlobalAllowlist>,
) -> bool {
    // Check global allowlist first
    if let Some(global) = global_allowlist {
        if is_allowed_by_allowlist(input, match_text, start_pos, end_pos, global) {
            return true;
        }
    }

    // Check rule-specific allowlists
    if let Some(rule_allowlists) = &compiled_rule.compiled_allowlists {
        for allowlist in rule_allowlists {
            if is_allowed_by_rule_allowlist(input, path, match_text, start_pos, end_pos, allowlist)
            {
                return true;
            }
        }
    }

    false
}

/// Check if a match is allowed by a global allowlist
fn is_allowed_by_allowlist(
    _input: &str,
    match_text: &str,
    _start_pos: usize,
    _end_pos: usize,
    allowlist: &CompiledGlobalAllowlist,
) -> bool {
    // Check regex patterns
    for regex in &allowlist.compiled_regexes {
        if regex.is_match(match_text) {
            return true;
        }
    }

    // Check stopwords
    if let Some(stopwords) = &allowlist.stopwords {
        for stopword in stopwords {
            if match_text.to_lowercase().contains(&stopword.to_lowercase()) {
                return true;
            }
        }
    }

    false
}

/// Check if a match is allowed by a rule-specific allowlist
pub fn is_allowed_by_rule_allowlist(
    input: &str,
    path: Option<&str>,
    match_text: &str,
    start_pos: usize,
    end_pos: usize,
    allowlist: &CompiledRuleAllowlist,
) -> bool {
    let mut checks = Vec::new();

    // Determine the target text based on regex_target
    let target_text = match allowlist.regex_target.as_deref() {
        Some("match") => match_text,
        Some("line") => {
            // Extract the line containing the match
            let line_start = input[..start_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_end = input[end_pos..]
                .find('\n')
                .map(|i| end_pos + i)
                .unwrap_or(input.len());
            &input[line_start..line_end]
        }
        _ => match_text, // Default to match
    };

    // Check regex patterns using pre-compiled regexes
    if !allowlist.compiled_regexes.is_empty() {
        let regex_matches = allowlist
            .compiled_regexes
            .iter()
            .any(|regex| regex.is_match(target_text));
        checks.push(regex_matches);
    }

    // Check stopwords with configuration-aware logic
    if let Some(stopwords) = &allowlist.stopwords {
        let stopword_matches = stopwords.iter().any(|stopword| {
            // For configuration-style patterns (KEY=VALUE), be more permissive
            if let Some(equals_pos) = target_text.find('=') {
                let value = &target_text[equals_pos + 1..];

                // Only filter if the value itself is obviously a placeholder/test value
                // Check if the entire value is just the stopword or a simple variation
                let value_lower = value.to_lowercase();
                let stopword_lower = stopword.to_lowercase();

                // Filter only if:
                // 1. The value is exactly the stopword (e.g., "password")
                // 2. The value is a simple variation like "password123" or "secretkey"
                // 3. The value contains the stopword and is very short/simple

                if value_lower == stopword_lower {
                    true // Exact match: PASSWORD=password
                } else if value.len() < 15 && value_lower.contains(&stopword_lower) {
                    // Short values containing stopwords: PASSWORD=password123
                    let without_stopword = value_lower.replace(&stopword_lower, "");
                    // If removing the stopword leaves only numbers/simple chars, it's likely a test value
                    without_stopword
                        .chars()
                        .all(|c| c.is_ascii_digit() || "!@#$%^&*()_+-=[]{}|;:,.<>?".contains(c))
                } else {
                    false // Don't filter longer/complex values
                }
            } else {
                // For non-KEY=VALUE patterns, use original logic but be more restrictive
                // Only filter on very obvious stopwords
                let obvious_false_positives = ["example", "test", "demo", "sample", "placeholder"];
                if obvious_false_positives.contains(&stopword.as_str()) {
                    target_text
                        .to_lowercase()
                        .contains(&stopword.to_lowercase())
                } else {
                    false
                }
            }
        });
        checks.push(stopword_matches);
    }

    // Check paths
    if let Some(paths) = &allowlist.paths {
        if let Some(path) = path {
            checks.push(paths.iter().any(|p| path.contains(p)));
        }
    }

    // If no checks were added, this allowlist doesn't apply
    if checks.is_empty() {
        return false;
    }

    // Apply condition logic (AND vs OR)
    match allowlist.condition.as_deref() {
        Some("AND") => checks.iter().all(|&check| check),
        _ => checks.iter().any(|&check| check), // Default to OR
    }
}

/// Helper function to check if input contains any of the rule keywords
pub fn contains_any_keyword(input: &str, keywords: &[String]) -> bool {
    let input_lower = input.to_lowercase();
    keywords
        .iter()
        .any(|keyword| input_lower.contains(&keyword.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entropy_calculation() {
        // Test high entropy (random-like) string
        let high_entropy = calculate_entropy("Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA");

        // Test low entropy (repetitive) string
        let low_entropy = calculate_entropy("aaaaaaaaaa");

        // Test empty string
        let zero_entropy = calculate_entropy("");

        assert!(high_entropy > low_entropy);
        assert_eq!(zero_entropy, 0.0);

        println!("High entropy: {:.2}", high_entropy);
        println!("Low entropy: {:.2}", low_entropy);
        println!("Zero entropy: {:.2}", zero_entropy);
    }
}
