use std::collections::HashMap;
use std::sync::RwLock;

/// Compiled pattern cache — avoids recompiling regex/glob on every `matches_pattern` call.
static PATTERN_CACHE: RwLock<Option<PatternCache>> = RwLock::new(None);

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

/// Match a scope-key pattern against a single argument.
///
/// - `re:<regex>` — regex match (compiled once and cached)
/// - Contains `*`, `?`, or `[` — glob match (compiled once and cached)
/// - Otherwise — exact string equality
pub fn matches_pattern(pattern: &str, arg: &str) -> bool {
    if let Some(re_pattern) = pattern.strip_prefix("re:") {
        // Fast path: read-only lookup (concurrent readers allowed).
        {
            let guard = PATTERN_CACHE.read().unwrap_or_else(|e| e.into_inner());
            if let Some(cache) = guard.as_ref()
                && let Some(re) = cache.regex.get(re_pattern)
            {
                return re.is_match(arg);
            }
        }
        // Slow path: compile and insert under a write lock.
        let mut guard = PATTERN_CACHE.write().unwrap_or_else(|e| e.into_inner());
        let cache = guard.get_or_insert_with(PatternCache::new);
        // Double-check after acquiring write lock.
        if let Some(re) = cache.regex.get(re_pattern) {
            return re.is_match(arg);
        }
        match regex::Regex::new(re_pattern) {
            Ok(re) => {
                let result = re.is_match(arg);
                cache.regex.insert(re_pattern.to_string(), re);
                result
            }
            Err(_) => false,
        }
    } else if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        // Fast path: read-only lookup.
        {
            let guard = PATTERN_CACHE.read().unwrap_or_else(|e| e.into_inner());
            if let Some(cache) = guard.as_ref()
                && let Some(matcher) = cache.glob.get(pattern)
            {
                return matcher.is_match(arg);
            }
        }
        // Slow path: compile and insert under a write lock.
        let mut guard = PATTERN_CACHE.write().unwrap_or_else(|e| e.into_inner());
        let cache = guard.get_or_insert_with(PatternCache::new);
        // Double-check after acquiring write lock.
        if let Some(matcher) = cache.glob.get(pattern) {
            return matcher.is_match(arg);
        }
        match globset::Glob::new(pattern) {
            Ok(g) => {
                let matcher = g.compile_matcher();
                let result = matcher.is_match(arg);
                cache.glob.insert(pattern.to_string(), matcher);
                result
            }
            Err(_) => false,
        }
    } else {
        pattern == arg
    }
}
