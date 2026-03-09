use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct PatternCache {
    regex: HashMap<String, regex::Regex>,
    glob: HashMap<String, globset::GlobMatcher>,
}

impl PatternCache {
    fn new() -> Self {
        Self {
            regex: HashMap::new(),
            glob: HashMap::new(),
        }
    }
}

static PATTERN_CACHE: OnceLock<Mutex<PatternCache>> = OnceLock::new();

#[inline]
fn cache() -> &'static Mutex<PatternCache> {
    PATTERN_CACHE.get_or_init(|| Mutex::new(PatternCache::new()))
}

/// Match a scope-key pattern against a single argument.
///
/// - `re:<regex>` — regex match (compiled once and cached)
/// - Contains `*`, `?`, or `[` — glob match (compiled once and cached)
/// - Otherwise — exact string equality
pub fn matches_pattern(pattern: &str, arg: &str) -> bool {
    if let Some(re_pattern) = pattern.strip_prefix("re:") {
        let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        // Return cached compiled regex if available; compile and cache otherwise.
        if let Some(re) = guard.regex.get(re_pattern) {
            return re.is_match(arg);
        }
        match regex::Regex::new(re_pattern) {
            Ok(re) => {
                let result = re.is_match(arg);
                guard.regex.insert(re_pattern.to_string(), re);
                result
            }
            Err(_) => false,
        }
    } else if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(matcher) = guard.glob.get(pattern) {
            return matcher.is_match(arg);
        }
        match globset::Glob::new(pattern) {
            Ok(g) => {
                let matcher = g.compile_matcher();
                let result = matcher.is_match(arg);
                guard.glob.insert(pattern.to_string(), matcher);
                result
            }
            Err(_) => false,
        }
    } else {
        pattern == arg
    }
}
