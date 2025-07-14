use stakpak_shared::utils::{matches_gitignore_pattern, read_gitignore_patterns};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::AppState;

#[derive(Debug, Clone)]
pub struct AutoComplete {
    pub file_suggestions: Vec<String>,
    pub filtered_files: Vec<String>,
    pub is_file_mode: bool,
    pub trigger_char: Option<char>, // '@' or None for Tab
    // Cache for filtered results to avoid recomputation
    filter_cache: HashMap<String, Vec<String>>,
    // Pre-computed lowercase versions for faster matching
    lowercase_files: Vec<String>,
    pub debounced_filter: DebouncedFilter,
}

impl Default for AutoComplete {
    fn default() -> Self {
        Self {
            file_suggestions: Vec::new(),
            filtered_files: Vec::new(),
            is_file_mode: false,
            trigger_char: None,
            filter_cache: HashMap::new(),
            lowercase_files: Vec::new(),
            debounced_filter: DebouncedFilter::new(120), // 120ms debounce
        }
    }
}

impl AutoComplete {
    /// Load all files from current directory recursively
    pub fn load_files_from_directory(&mut self, dir: &Path) {
        // Only scan if not already cached for this session
        if !self.file_suggestions.is_empty() {
            return;
        }
        self.file_suggestions.clear();
        self.lowercase_files.clear();
        self.filter_cache.clear();

        // Read gitignore patterns from the directory
        let base_dir = dir.to_string_lossy();
        let ignore_patterns = read_gitignore_patterns(&base_dir);
        self.collect_files_recursive(dir, dir, &ignore_patterns);

        // Pre-compute lowercase versions for faster filtering
        self.lowercase_files = self
            .file_suggestions
            .iter()
            .map(|f| f.to_lowercase())
            .collect();
    }

    fn collect_files_recursive(
        &mut self,
        current_dir: &Path,
        base_dir: &Path,
        ignore_patterns: &[String],
    ) {
        if let Ok(entries) = fs::read_dir(current_dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Get relative path from base directory for gitignore matching
                let relative_path = match path.strip_prefix(base_dir) {
                    Ok(rel_path) => rel_path,
                    Err(_) => &path,
                };
                let path_str = relative_path.to_string_lossy();

                // Check if path matches any gitignore pattern
                let should_ignore = ignore_patterns
                    .iter()
                    .any(|pattern| matches_gitignore_pattern(pattern, &path_str));

                if should_ignore {
                    continue;
                }

                if path.is_file() {
                    // Add relative path from base directory
                    if let Some(path_str) = relative_path.to_str() {
                        self.file_suggestions.push(path_str.to_string());
                    }
                } else if path.is_dir() {
                    // Recursively collect from subdirectories
                    self.collect_files_recursive(&path, base_dir, ignore_patterns);
                }
            }
        }
    }

    /// Filter files based on current input - optimized version, debounced
    pub fn filter_files(&mut self, current_word: &str) {
        if !self.debounced_filter.should_filter(current_word) {
            return;
        }
        // Fast path: if input is empty, just show the first 50 files
        if current_word.is_empty() {
            self.filtered_files = self.file_suggestions.iter().take(50).cloned().collect();
            return;
        }

        // Check cache first
        if let Some(cached_result) = self.filter_cache.get(current_word) {
            self.filtered_files = cached_result.clone();
            return;
        }

        let word_lower = current_word.to_lowercase();
        let mut results = Vec::new();

        // Use pre-computed lowercase versions for faster matching
        for (i, file_lower) in self.lowercase_files.iter().enumerate() {
            if file_lower.contains(&word_lower) || fuzzy_match_optimized(file_lower, &word_lower) {
                results.push(self.file_suggestions[i].clone());

                // Early exit if we have enough results
                if results.len() >= 100 {
                    break;
                }
            }
        }

        // Sort by relevance (exact matches first, then prefix matches, then contains)
        results.sort_by(|a, b| {
            let a_lower = a.to_lowercase();
            let b_lower = b.to_lowercase();

            // Exact match check
            if a_lower == word_lower {
                return std::cmp::Ordering::Less;
            }
            if b_lower == word_lower {
                return std::cmp::Ordering::Greater;
            }

            // Prefix match check
            let a_starts = a_lower.starts_with(&word_lower);
            let b_starts = b_lower.starts_with(&word_lower);
            match (a_starts, b_starts) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.len().cmp(&b.len()), // Shorter files first for same category
            }
        });

        // Limit results to prevent overwhelming UI
        results.truncate(50);

        // Cache the result for future use
        self.filter_cache
            .insert(current_word.to_string(), results.clone());

        // Limit cache size to prevent memory issues
        if self.filter_cache.len() > 100 {
            self.filter_cache.clear();
        }

        self.filtered_files = results;
    }

    /// Get the current filtered files for display
    pub fn get_filtered_files(&self) -> &[String] {
        &self.filtered_files
    }

    /// Get a specific file by index for selection
    pub fn get_file_at_index(&self, index: usize) -> Option<&str> {
        self.filtered_files.get(index).map(|s| s.as_str())
    }

    /// Get the number of filtered files
    pub fn filtered_count(&self) -> usize {
        self.filtered_files.len()
    }

    /// Reset autocomplete state
    pub fn reset(&mut self) {
        self.filtered_files.clear();
        self.is_file_mode = false;
        self.trigger_char = None;
        // Don't clear cache and lowercase_files - keep them for performance
    }

    /// Check if currently in file autocomplete mode
    pub fn is_active(&self) -> bool {
        self.is_file_mode
    }

    /// Clear all caches (call this when directory changes)
    pub fn clear_caches(&mut self) {
        self.file_suggestions.clear();
        self.lowercase_files.clear();
        self.filter_cache.clear();
    }
}

/// Optimized fuzzy matching with early exit
fn fuzzy_match_optimized(text: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }

    if text.len() < pattern.len() {
        return false;
    }

    let text_chars: Vec<char> = text.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();

    let mut text_idx = 0;
    let mut pattern_idx = 0;

    while text_idx < text_chars.len() && pattern_idx < pattern_chars.len() {
        if text_chars[text_idx] == pattern_chars[pattern_idx] {
            pattern_idx += 1;
        }
        text_idx += 1;
    }

    pattern_idx == pattern_chars.len()
}

/// Find @ trigger before cursor position - optimized
pub fn find_at_trigger(state: &AppState) -> Option<usize> {
    let input = &state.input;
    let cursor_pos = state.cursor_position;
    let safe_pos = cursor_pos.min(input.len());
    let before_cursor = &input[..safe_pos];

    // Find the last @ that's either at start or preceded by whitespace
    // Use bytes for faster iteration when possible
    let bytes = before_cursor.as_bytes();
    for i in (0..bytes.len()).rev() {
        if bytes[i] == b'@' {
            // Check if it's at start or preceded by whitespace
            if i == 0 || bytes[i - 1].is_ascii_whitespace() {
                return Some(i);
            }
        }
    }
    None
}

/// Get the current word being typed for filtering - optimized
pub fn get_current_word(state: &AppState, trigger_char: Option<char>) -> String {
    let input = &state.input;
    let cursor_pos = state.cursor_position;
    let safe_pos = cursor_pos.min(input.len());

    match trigger_char {
        Some('@') => {
            // Find @ and get text after it
            if let Some(at_pos) = find_at_trigger(state) {
                let after_at = &input[at_pos + 1..safe_pos];
                after_at.to_string()
            } else {
                String::new()
            }
        }
        None => {
            // Tab mode - get current word
            let before_cursor = &input[..safe_pos];
            if let Some(word_start) = before_cursor.rfind(char::is_whitespace) {
                input[word_start + 1..safe_pos].to_string()
            } else {
                before_cursor.to_string()
            }
        }
        _ => String::new(),
    }
}

/// Handle Tab trigger for file autocomplete - with debouncing
pub fn handle_tab_trigger(state: &mut AppState) -> bool {
    if state.input.trim().is_empty() {
        return false;
    }

    // Load files if not already loaded
    if state.autocomplete.file_suggestions.is_empty() {
        if let Ok(current_dir) = std::env::current_dir() {
            state.autocomplete.load_files_from_directory(&current_dir);
        }
    }

    let current_word = get_current_word(state, None);
    state.autocomplete.filter_files(&current_word);

    if !state.autocomplete.filtered_files.is_empty() {
        state.autocomplete.is_file_mode = true;
        state.autocomplete.trigger_char = None;
        state.show_helper_dropdown = true;
        state.helper_selected = 0;
        return true;
    }
    false
}

/// Handle @ trigger for file autocomplete - with debouncing
pub fn handle_at_trigger(state: &mut AppState) -> bool {
    if state.autocomplete.file_suggestions.is_empty() {
        if let Ok(current_dir) = std::env::current_dir() {
            state.autocomplete.load_files_from_directory(&current_dir);
        }
    }

    let current_word = get_current_word(state, Some('@'));
    state.autocomplete.filter_files(&current_word);

    if !state.autocomplete.filtered_files.is_empty() {
        state.autocomplete.is_file_mode = true;
        state.autocomplete.trigger_char = Some('@');
        state.show_helper_dropdown = true;
        state.helper_selected = 0;
        return true;
    }
    false
}

/// Handle file selection and update input string
pub fn handle_file_selection(state: &mut AppState, selected_file: &str) {
    match state.autocomplete.trigger_char {
        Some('@') => {
            // Replace from @ to cursor with selected file
            if let Some(at_pos) = find_at_trigger(state) {
                let before_at = state.input[..at_pos].to_string();
                let after_cursor = state.input[state.cursor_position..].to_string();
                state.input = format!("{}{}{}", before_at, selected_file, after_cursor);
                state.cursor_position = before_at.len() + selected_file.len();
            }
        }
        None => {
            // Tab mode - replace current word
            let safe_pos = state.cursor_position.min(state.input.len());
            let before_cursor = &state.input[..safe_pos];
            if let Some(word_start) = before_cursor.rfind(char::is_whitespace) {
                let before_word = &state.input[..word_start + 1];
                let after_cursor = &state.input[state.cursor_position..];
                state.input = format!("{}{}{}", before_word, selected_file, after_cursor);
                state.cursor_position = word_start + 1 + selected_file.len();
            } else {
                // Replace from beginning
                let after_cursor = &state.input[state.cursor_position..];
                state.input = format!("{}{}", selected_file, after_cursor);
                state.cursor_position = selected_file.len();
            }
        }
        _ => {}
    }

    // Reset autocomplete state
    state.autocomplete.reset();
    state.show_helper_dropdown = false;
    state.filtered_helpers.clear();
    state.helper_selected = 0;
}

#[derive(Debug, Clone)]
pub struct DebouncedFilter {
    last_query: String,
    last_update: std::time::Instant,
    debounce_ms: u64,
}

impl DebouncedFilter {
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            last_query: String::new(),
            last_update: std::time::Instant::now(),
            debounce_ms,
        }
    }

    pub fn should_filter(&mut self, query: &str) -> bool {
        let now = std::time::Instant::now();
        let should_update = query != self.last_query
            || now.duration_since(self.last_update).as_millis() > self.debounce_ms as u128;

        if should_update {
            self.last_query = query.to_string();
            self.last_update = now;
        }

        should_update
    }
}
