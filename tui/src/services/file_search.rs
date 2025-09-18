use stakpak_shared::utils::{matches_gitignore_pattern, read_gitignore_patterns};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs;
use std::path::Path;

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use tokio::sync::mpsc;

use crate::AppState;
use crate::app::{FileSearchResult, HelperCommand};

/// Fuzzy file matcher that maintains only the best N matches for performance
#[derive(Debug)]
struct FuzzyFileMatcher {
    max_matches: usize,
    pattern: Pattern,
    matcher: Matcher,
    matches: BinaryHeap<Reverse<(u32, String)>>, // (score, path) - Reverse for max-heap behavior
    utf32buf: Vec<char>,
}

impl FuzzyFileMatcher {
    fn new(max_matches: usize, pattern_text: &str) -> Self {
        let pattern = Pattern::new(
            pattern_text,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );

        Self {
            max_matches,
            pattern,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            matches: BinaryHeap::new(),
            utf32buf: Vec::new(),
        }
    }

    fn add_file(&mut self, file_path: &str) {
        let haystack: Utf32Str<'_> = Utf32Str::new(file_path, &mut self.utf32buf);

        if let Some(score) = self.pattern.score(haystack, &mut self.matcher) {
            if self.matches.len() < self.max_matches {
                self.matches.push(Reverse((score, file_path.to_string())));
            } else if let Some(&Reverse((min_score, _))) = self.matches.peek() {
                if score > min_score {
                    self.matches.pop();
                    self.matches.push(Reverse((score, file_path.to_string())));
                }
            }
        }
    }

    fn get_sorted_matches(&mut self) -> Vec<String> {
        let mut sorted_matches: Vec<(u32, String)> = self
            .matches
            .drain()
            .map(|Reverse((score, path))| (score, path))
            .collect();

        // Sort by descending score, then ascending path for consistent ordering
        sorted_matches.sort_by(|a, b| match b.0.cmp(&a.0) {
            std::cmp::Ordering::Equal => a.1.cmp(&b.1),
            other => other,
        });

        sorted_matches.into_iter().map(|(_, path)| path).collect()
    }
}

#[derive(Debug)]
pub struct FileSearch {
    pub file_suggestions: Vec<String>,
    pub filtered_files: Vec<String>,
    pub is_file_mode: bool,
    pub trigger_char: Option<char>, // '@' or None for Tab
    // Fuzzy matcher for efficient file matching
    fuzzy_matcher: Option<FuzzyFileMatcher>,
    pub debounced_filter: DebouncedFilter,
    // Maximum number of matches to return
    max_matches: usize,
}

impl Default for FileSearch {
    fn default() -> Self {
        Self {
            file_suggestions: Vec::new(),
            filtered_files: Vec::new(),
            is_file_mode: false,
            trigger_char: None,
            fuzzy_matcher: None,
            debounced_filter: DebouncedFilter::new(120), // 120ms debounce
            max_matches: 50,                             // Default to 50 matches for performance
        }
    }
}

impl FileSearch {
    /// Load all files from current directory recursively (no caching)
    pub fn load_files_from_directory(&mut self, dir: &Path) {
        // Always clear and reload - no caching
        self.file_suggestions.clear();

        // Read gitignore patterns from the directory
        let base_dir = dir.to_string_lossy();
        let ignore_patterns = read_gitignore_patterns(&base_dir);
        self.collect_files_recursive(dir, dir, &ignore_patterns);
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
                let should_ignore = ignore_patterns.iter().any(|pattern| {
                    // First try the standard pattern matching
                    if matches_gitignore_pattern(pattern, &path_str) {
                        return true;
                    }

                    // Extra step: if pattern starts with "/", also check if path starts with pattern without the leading slash
                    if let Some(pattern_without_slash) = pattern.strip_prefix('/') {
                        path_str == pattern_without_slash
                            || path_str.starts_with(&format!("{}/", pattern_without_slash))
                    } else {
                        false
                    }
                });

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

    /// Filter files based on current input using fuzzy matching - optimized version, debounced
    pub fn filter_files(&mut self, current_word: &str) {
        if !self.debounced_filter.should_filter(current_word) {
            return;
        }

        // Fast path: if input is empty, just show the first N files
        if current_word.is_empty() {
            self.filtered_files = self
                .file_suggestions
                .iter()
                .take(self.max_matches)
                .cloned()
                .collect();
            return;
        }

        // Create or update fuzzy matcher for the current pattern
        self.fuzzy_matcher = Some(FuzzyFileMatcher::new(self.max_matches, current_word));

        // Add all files to the fuzzy matcher
        for file_path in &self.file_suggestions {
            if let Some(ref mut matcher) = self.fuzzy_matcher {
                matcher.add_file(file_path);
            }
        }

        // Get the best matches
        if let Some(ref mut matcher) = self.fuzzy_matcher {
            self.filtered_files = matcher.get_sorted_matches();
        } else {
            self.filtered_files.clear();
        }
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

    /// Reset file_search state
    pub fn reset(&mut self) {
        self.filtered_files.clear();
        self.is_file_mode = false;
        self.trigger_char = None;
        self.fuzzy_matcher = None;
        // Keep file_suggestions for performance
    }

    /// Check if currently in file file_search mode
    pub fn is_active(&self) -> bool {
        self.is_file_mode
    }

    /// Clear all caches (call this when directory changes)
    pub fn clear_caches(&mut self) {
        self.file_suggestions.clear();
        self.fuzzy_matcher = None;
    }

    /// Force reload files from directory (useful when files are created/deleted)
    pub fn force_reload_files(&mut self, dir: &Path) {
        self.clear_caches();
        self.load_files_from_directory(dir);
    }
}

// Refactored: Find @ trigger before cursor position - optimized
pub fn find_at_trigger(input: &str, cursor_pos: usize) -> Option<usize> {
    let safe_pos = cursor_pos.min(input.len());
    let before_cursor = &input[..safe_pos];
    // Find the last @ that's either at start or preceded by whitespace
    for (i, c) in before_cursor.char_indices().rev() {
        if c == '@' {
            // Check if it's at start or preceded by whitespace
            if i == 0
                || before_cursor
                    .chars()
                    .nth(i.saturating_sub(1))
                    .is_some_and(|ch| ch.is_whitespace())
            {
                // Check if @ is followed by whitespace - if so, don't consider it a valid trigger
                let after_at = &input[i + 1..safe_pos];
                if after_at.starts_with(char::is_whitespace) {
                    continue; // Skip this @ and look for the next one
                }
                return Some(i);
            }
        }
    }
    None
}

// Refactored: Get the current word being typed for filtering - optimized
pub fn get_current_word(input: &str, cursor_pos: usize, trigger_char: Option<char>) -> String {
    let safe_pos = cursor_pos.min(input.len());
    match trigger_char {
        Some('@') => {
            if let Some(at_pos) = find_at_trigger(input, cursor_pos) {
                let after_at = &input[at_pos + 1..safe_pos];
                after_at.to_string()
            } else {
                String::new()
            }
        }
        None => {
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

/// Handle Tab trigger for file file_search - with debouncing
pub fn handle_tab_trigger(state: &mut AppState) -> bool {
    if state.input().trim().is_empty() {
        return false;
    }

    // Load files if not already loaded
    if state.file_search.file_suggestions.is_empty() {
        if let Ok(current_dir) = std::env::current_dir() {
            state.file_search.load_files_from_directory(&current_dir);
        }
    }

    let current_word = get_current_word(state.input(), state.cursor_position(), None);
    state.file_search.filter_files(&current_word);

    if !state.file_search.filtered_files.is_empty() {
        state.file_search.is_file_mode = true;
        state.file_search.trigger_char = None;
        state.show_helper_dropdown = true;
        state.helper_selected = 0;
        return true;
    }
    false
}

// Refactored: Handle @ trigger for file file_search - with debouncing
pub fn handle_at_trigger(input: &str, cursor_pos: usize, file_search: &mut FileSearch) -> bool {
    if file_search.file_suggestions.is_empty() {
        if let Ok(current_dir) = std::env::current_dir() {
            file_search.load_files_from_directory(&current_dir);
        }
    }
    let current_word = get_current_word(input, cursor_pos, Some('@'));
    file_search.filter_files(&current_word);
    !file_search.filtered_files.is_empty()
}

/// Handle file selection and update input string
pub fn handle_file_selection(state: &mut AppState, selected_file: &str) {
    match state.file_search.trigger_char {
        Some('@') => {
            // Replace from @ to cursor with selected file
            if let Some(at_pos) = find_at_trigger(state.input(), state.cursor_position()) {
                let before_at = state.input()[..at_pos].to_string();
                let after_cursor = state.input()[state.cursor_position()..].to_string();
                let new_text = format!("{}{}{}", before_at, selected_file, after_cursor);
                state.text_area.set_text(&new_text);
                state
                    .text_area
                    .set_cursor(before_at.len() + selected_file.len());
            }
        }
        None => {
            // Tab mode - replace current word
            let safe_pos = state.cursor_position().min(state.input().len());
            let before_cursor = &state.input()[..safe_pos];
            if let Some(word_start) = before_cursor.rfind(char::is_whitespace) {
                let before_word = &state.input()[..word_start + 1];
                let after_cursor = &state.input()[state.cursor_position()..];
                let new_text = format!("{}{}{}", before_word, selected_file, after_cursor);
                state.text_area.set_text(&new_text);
                state
                    .text_area
                    .set_cursor(word_start + 1 + selected_file.len());
            } else {
                // Replace from beginning
                let after_cursor = &state.input()[state.cursor_position()..];
                let new_text = format!("{}{}", selected_file, after_cursor);
                state.text_area.set_text(&new_text);
                state.text_area.set_cursor(selected_file.len());
            }
        }
        _ => {}
    }

    // Reset file_search state
    state.file_search.reset();
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

/// Async file_search worker for background filtering
pub async fn file_search_worker(
    mut rx: mpsc::Receiver<(String, usize)>, // (input, cursor_position)
    tx: mpsc::Sender<FileSearchResult>,
    helpers: Vec<HelperCommand>,
    mut file_search: FileSearch,
) {
    while let Some((input, cursor_position)) = rx.recv().await {
        // Always load files fresh on each request (no caching)
        if let Ok(current_dir) = std::env::current_dir() {
            file_search.load_files_from_directory(&current_dir);
        }
        // Filter helpers - only when input starts with '/' and is not empty
        let filtered_helpers: Vec<HelperCommand> = if input.starts_with('/') && !input.is_empty() {
            helpers
                .iter()
                .filter(|h| {
                    h.command
                        .to_lowercase()
                        .contains(&input[1..].to_lowercase())
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

        let mut filtered_files = Vec::new();
        // Detect @ trigger using new signature

        if let Some(at_pos) = find_at_trigger(&input, cursor_position) {
            let is_valid_at = at_pos == 0
                || input
                    .chars()
                    .nth(at_pos.saturating_sub(1))
                    .is_some_and(|ch| ch.is_whitespace());
            if is_valid_at && handle_at_trigger(&input, cursor_position, &mut file_search) {
                file_search.is_file_mode = true;
                file_search.trigger_char = Some('@');
                filtered_files = file_search.filtered_files.clone();
            }
        }

        // TODO: Add / and other triggers as needed

        let _ = tx
            .send(FileSearchResult {
                filtered_helpers,
                filtered_files,
                cursor_position,
                input,
            })
            .await;
    }
}