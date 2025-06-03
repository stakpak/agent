// Secret redaction implementation based on gitleaks (https://github.com/gitleaks/gitleaks)
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct GitleaksConfig {
    #[allow(dead_code)]
    pub title: Option<String>,
    pub allowlist: Option<Allowlist>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Allowlist {
    #[allow(dead_code)]
    pub description: Option<String>,
    #[allow(dead_code)]
    pub paths: Option<Vec<String>>,
    pub regexes: Option<Vec<String>>,
    pub stopwords: Option<Vec<String>>,
    /// Pre-compiled regexes (not serialized)
    #[serde(skip)]
    pub compiled_regexes: Vec<Regex>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub id: String,
    #[allow(dead_code)]
    pub description: String,
    pub regex: Option<String>,
    pub entropy: Option<f64>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[allow(dead_code)]
    pub path: Option<String>,
    pub allowlists: Option<Vec<RuleAllowlist>>,
    /// Pre-compiled regex (not serialized)
    #[serde(skip)]
    pub compiled_regex: Option<Regex>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RuleAllowlist {
    #[allow(dead_code)]
    pub description: Option<String>,
    pub condition: Option<String>, // "AND" or default "OR"
    pub paths: Option<Vec<String>>,
    pub regexes: Option<Vec<String>>,
    pub stopwords: Option<Vec<String>>,
    #[serde(rename = "regexTarget")]
    pub regex_target: Option<String>, // "match", "line", etc.
    /// Pre-compiled regexes (not serialized)
    #[serde(skip)]
    pub compiled_regexes: Vec<Regex>,
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

#[derive(Debug, Default)]
pub struct CompilationErrors {
    pub regex_errors: Vec<(String, String)>, // (rule_id, error_message)
    pub warnings: Vec<String>,
}

impl CompilationErrors {
    pub fn add_regex_error(&mut self, rule_id: String, error: String) {
        self.regex_errors.push((rule_id, error));
    }

    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.regex_errors.is_empty() && self.warnings.is_empty()
    }
}

/// Trait for compiling regex patterns in configuration structures
pub trait RegexCompilable {
    fn compile_regexes(&mut self) -> CompilationErrors;
}

impl RegexCompilable for Allowlist {
    fn compile_regexes(&mut self) -> CompilationErrors {
        let mut errors = CompilationErrors::default();
        self.compiled_regexes.clear();

        if let Some(regexes) = &self.regexes {
            for pattern in regexes {
                match Regex::new(pattern) {
                    Ok(regex) => self.compiled_regexes.push(regex),
                    Err(e) => errors.add_warning(format!(
                        "Failed to compile allowlist regex '{}': {}",
                        pattern, e
                    )),
                }
            }
        }

        errors
    }
}

impl RegexCompilable for RuleAllowlist {
    fn compile_regexes(&mut self) -> CompilationErrors {
        let mut errors = CompilationErrors::default();
        self.compiled_regexes.clear();

        if let Some(regexes) = &self.regexes {
            for pattern in regexes {
                match Regex::new(pattern) {
                    Ok(regex) => self.compiled_regexes.push(regex),
                    Err(e) => errors.add_warning(format!(
                        "Failed to compile rule allowlist regex '{}': {}",
                        pattern, e
                    )),
                }
            }
        }

        errors
    }
}

impl RegexCompilable for Rule {
    fn compile_regexes(&mut self) -> CompilationErrors {
        let mut errors = CompilationErrors::default();

        // Compile main regex with fallback handling
        if let Some(regex_pattern) = &self.regex {
            match Regex::new(regex_pattern) {
                Ok(regex) => self.compiled_regex = Some(regex),
                Err(e) => {
                    // Handle regex compilation errors with specific fallbacks
                    if self.id == "generic-api-key" {
                        match create_simple_api_key_regex() {
                            Ok(simple_regex) => {
                                self.compiled_regex = Some(simple_regex);
                                errors.add_warning(format!(
                                    "Used fallback regex for rule '{}' due to: {}",
                                    self.id, e
                                ));
                            }
                            Err(fallback_err) => {
                                errors.add_regex_error(
                                    self.id.clone(),
                                    format!(
                                        "Failed to compile regex and fallback: {} / {}",
                                        e, fallback_err
                                    ),
                                );
                            }
                        }
                    } else {
                        errors.add_regex_error(self.id.clone(), e.to_string());
                    }
                }
            }
        } else {
            // Rule has no regex pattern (e.g., path-only rules like pkcs12-file)
            // This is valid for certain types of rules, so no error
            self.compiled_regex = None;
        }

        // Compile allowlist regexes
        if let Some(allowlists) = &mut self.allowlists {
            for allowlist in allowlists {
                let allowlist_errors = allowlist.compile_regexes();
                errors.warnings.extend(allowlist_errors.warnings);
                errors.regex_errors.extend(allowlist_errors.regex_errors);
            }
        }

        errors
    }
}

impl RegexCompilable for GitleaksConfig {
    fn compile_regexes(&mut self) -> CompilationErrors {
        let mut errors = CompilationErrors::default();

        // Compile global allowlist
        if let Some(allowlist) = &mut self.allowlist {
            let allowlist_errors = allowlist.compile_regexes();
            errors.warnings.extend(allowlist_errors.warnings);
            errors.regex_errors.extend(allowlist_errors.regex_errors);
        }

        // Compile rules (keeping only successfully compiled ones)
        let mut compiled_rules = Vec::new();
        for mut rule in self.rules.drain(..) {
            let rule_errors = rule.compile_regexes();
            errors.warnings.extend(rule_errors.warnings);
            errors.regex_errors.extend(rule_errors.regex_errors);

            // Keep rules that either compiled successfully or don't have regex patterns (e.g., path-only rules)
            if rule.compiled_regex.is_some() || rule.regex.is_none() {
                compiled_rules.push(rule);
            }
        }
        self.rules = compiled_rules;

        errors
    }
}

/// Lazy-loaded gitleaks configuration
pub static GITLEAKS_CONFIG: Lazy<GitleaksConfig> = Lazy::new(|| {
    // Load main gitleaks configuration
    let config_str = include_str!("gitleaks.toml");
    let mut config: GitleaksConfig =
        toml::from_str(config_str).expect("Failed to parse gitleaks.toml");

    // Load additional rules configuration
    let additional_config_str = include_str!("additional_rules.toml");
    let additional_config: GitleaksConfig =
        toml::from_str(additional_config_str).expect("Failed to parse additional_rules.toml");

    // Merge additional rules into the main configuration
    config.rules.extend(additional_config.rules);

    // Merge additional allowlist if present
    if let Some(additional_allowlist) = additional_config.allowlist {
        match &mut config.allowlist {
            Some(existing_allowlist) => {
                // Merge regexes
                if let Some(additional_regexes) = additional_allowlist.regexes {
                    match &mut existing_allowlist.regexes {
                        Some(existing_regexes) => existing_regexes.extend(additional_regexes),
                        None => existing_allowlist.regexes = Some(additional_regexes),
                    }
                }

                // Merge stopwords
                if let Some(additional_stopwords) = additional_allowlist.stopwords {
                    match &mut existing_allowlist.stopwords {
                        Some(existing_stopwords) => existing_stopwords.extend(additional_stopwords),
                        None => existing_allowlist.stopwords = Some(additional_stopwords),
                    }
                }
            }
            None => config.allowlist = Some(additional_allowlist),
        }
    }

    let compilation_errors = config.compile_regexes();

    // Log compilation results
    if !compilation_errors.warnings.is_empty() {
        eprintln!("Gitleaks compilation warnings:");
        for warning in &compilation_errors.warnings {
            eprintln!("  Warning: {}", warning);
        }
    }

    if !compilation_errors.regex_errors.is_empty() {
        eprintln!("Gitleaks compilation errors:");
        for (rule_id, error) in &compilation_errors.regex_errors {
            eprintln!("  Error in rule '{}': {}", rule_id, error);
        }
    }

    eprintln!(
        "Gitleaks config loaded: {} rules compiled successfully (includes {} content-based and {} path-based rules)",
        config.rules.len(),
        config
            .rules
            .iter()
            .filter(|r| r.compiled_regex.is_some())
            .count(),
        config
            .rules
            .iter()
            .filter(|r| r.compiled_regex.is_none())
            .count(),
    );
    config
});

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

/// Detects secrets in the input string using gitleaks configuration
///
/// This implementation follows the gitleaks methodology:
/// 1. Apply regex rules to find potential secrets
/// 2. Check entropy thresholds to filter out low-entropy matches
/// 3. Apply allowlists to exclude known false positives
/// 4. Check keywords to ensure relevance
pub fn detect_secrets(input: &str, path: Option<&str>) -> Vec<DetectedSecret> {
    let mut detected_secrets = Vec::new();
    let config = &*GITLEAKS_CONFIG;

    // Apply each compiled rule from the configuration
    for rule in &config.rules {
        // Skip rules that don't have regex patterns (e.g., path-only rules)
        let regex = match &rule.compiled_regex {
            Some(regex) => regex,
            None => continue,
        };

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
                rule,
                &config.allowlist,
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

/// Check if a match should be allowed (filtered out) based on allowlists
pub fn should_allow_match(
    input: &str,
    path: Option<&str>,
    match_text: &str,
    start_pos: usize,
    end_pos: usize,
    rule: &Rule,
    global_allowlist: &Option<Allowlist>,
) -> bool {
    // Check global allowlist first
    if let Some(global) = global_allowlist {
        if is_allowed_by_allowlist(input, match_text, start_pos, end_pos, global) {
            return true;
        }
    }

    // Check rule-specific allowlists
    if let Some(rule_allowlists) = &rule.allowlists {
        for allowlist in rule_allowlists {
            if is_allowed_by_rule_allowlist(input, path, match_text, start_pos, end_pos, allowlist)
            {
                return true;
            }
        }
    }

    false
}

fn is_allowed_by_allowlist(
    _input: &str,
    match_text: &str,
    _start_pos: usize,
    _end_pos: usize,
    allowlist: &Allowlist,
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

pub fn is_allowed_by_rule_allowlist(
    input: &str,
    path: Option<&str>,
    match_text: &str,
    start_pos: usize,
    end_pos: usize,
    allowlist: &RuleAllowlist,
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

    #[test]
    fn test_additional_rules_loaded() {
        let config = &*GITLEAKS_CONFIG;

        // Check that the Anthropic API key rule from additional_rules.toml is loaded
        let anthropic_rule = config.rules.iter().find(|r| r.id == "anthropic-api-key");
        assert!(
            anthropic_rule.is_some(),
            "Anthropic API key rule should be loaded from additional_rules.toml"
        );

        if let Some(rule) = anthropic_rule {
            assert!(rule.keywords.contains(&"anthropic".to_string()));
            assert!(
                rule.compiled_regex.is_some(),
                "Anthropic rule regex should be compiled"
            );
        }

        println!("Total rules loaded: {}", config.rules.len());
    }

    #[test]
    fn test_anthropic_api_key_detection() {
        // Use a more realistic API key that doesn't contain alphabet sequences
        let test_input =
            "ANTHROPIC_API_KEY=sk-ant-api03-Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA9bD2eG5kM8pR1tX4zB7";
        let secrets = detect_secrets(test_input, None);

        // Should detect the Anthropic API key
        let anthropic_secret = secrets.iter().find(|s| s.rule_id == "anthropic-api-key");
        assert!(
            anthropic_secret.is_some(),
            "Should detect Anthropic API key"
        );

        if let Some(secret) = anthropic_secret {
            assert!(secret.value.starts_with("sk-ant-api03-"));
        }
    }
}
