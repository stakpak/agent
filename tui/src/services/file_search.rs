use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;
use std::sync::Arc;

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use tokio::sync::mpsc;

use crate::AppState;
use crate::app::{CustomCommand, FileSearchResult, HelperEntry};
use crate::event_loop::CommandsConfig;

/// Maintains the best N matches for a given pattern using parallel processing
#[derive(Debug)]
struct BestMatchesList {
    max_count: usize,
    num_matches: usize,
    pattern: Pattern,
    matcher: Matcher,
    binary_heap: BinaryHeap<Reverse<(u32, String)>>,
    utf32buf: Vec<char>,
}

impl BestMatchesList {
    fn new(max_count: usize, pattern: Pattern, matcher: Matcher) -> Self {
        Self {
            max_count,
            num_matches: 0,
            pattern,
            matcher,
            binary_heap: BinaryHeap::new(),
            utf32buf: Vec::new(),
        }
    }

    fn insert(&mut self, file_path: &str) {
        let haystack: Utf32Str<'_> = Utf32Str::new(file_path, &mut self.utf32buf);
        if let Some(score) = self.pattern.score(haystack, &mut self.matcher) {
            self.num_matches += 1;

            if self.binary_heap.len() < self.max_count {
                self.binary_heap
                    .push(Reverse((score, file_path.to_string())));
            } else if let Some(min_element) = self.binary_heap.peek()
                && score > min_element.0.0
            {
                self.binary_heap.pop();
                self.binary_heap
                    .push(Reverse((score, file_path.to_string())));
            }
        }
    }

    fn get_sorted_matches(&mut self) -> Vec<String> {
        let mut sorted_matches: Vec<(u32, String)> = self
            .binary_heap
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
    pub debounced_filter: DebouncedFilter,
    // Maximum number of matches to return
    max_matches: usize,
    // Cache for loaded files to avoid reloading
    last_directory: Option<String>,
}

impl Default for FileSearch {
    fn default() -> Self {
        Self {
            file_suggestions: Vec::new(),
            filtered_files: Vec::new(),
            is_file_mode: false,
            trigger_char: None,
            debounced_filter: DebouncedFilter::new(120), // 120ms debounce
            max_matches: 50,                             // Default to 50 matches for performance
            last_directory: None,
        }
    }
}

impl FileSearch {
    /// Load all files and directories from current directory using parallel walking with ignore crate
    pub fn scan_directory(&mut self, dir: &Path) {
        let dir_str = dir.to_string_lossy().to_string();

        // Only reload if directory changed or files are empty
        if self.last_directory.as_ref() == Some(&dir_str) && !self.file_suggestions.is_empty() {
            return;
        }

        self.file_suggestions.clear();
        self.last_directory = Some(dir_str);

        // Build overrides to exclude .git directory
        let mut overrides_builder = OverrideBuilder::new(dir);
        overrides_builder.add("!.git/").ok();
        let overrides = match overrides_builder.build() {
            Ok(o) => o,
            Err(_) => match OverrideBuilder::new(dir).build() {
                Ok(o) => o,
                Err(_) => return,
            },
        };

        // Use ignore crate for fast parallel directory walking
        let walker = WalkBuilder::new(dir)
            .threads(2) // Use 2 threads for optimal performance
            .hidden(false) // Don't skip hidden files
            .git_ignore(true) // Respect .gitignore files
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .overrides(overrides) // Exclude .git directory
            .require_git(false) // Don't require git to be present
            .build_parallel();

        let file_suggestions = Arc::new(std::sync::Mutex::new(Vec::new()));
        let file_suggestions_clone = file_suggestions.clone();

        walker.run(|| {
            let file_suggestions = file_suggestions_clone.clone();
            Box::new(move |entry_result| {
                if let Ok(entry) = entry_result
                    && let Some(ft) = entry.file_type()
                    && (ft.is_file() || ft.is_dir())
                    && let Ok(rel_path) = entry.path().strip_prefix(dir)
                    && let Some(path_str) = rel_path.to_str()
                    && !path_str.is_empty() // Skip the root directory itself
                    && let Ok(mut files) = file_suggestions.lock()
                {
                    // Append "/" to directories to distinguish them
                    let entry_str = if ft.is_dir() {
                        format!("{}/", path_str)
                    } else {
                        path_str.to_string()
                    };
                    files.push(entry_str);
                }
                ignore::WalkState::Continue
            })
        });

        if let Ok(files) = file_suggestions.lock() {
            self.file_suggestions = files.clone();
        }
    }

    /// Filter files based on current input using fuzzy matching with parallel processing
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

        // Create pattern and matcher for fuzzy matching
        let pattern = Pattern::new(
            current_word,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );

        let mut best_matches = BestMatchesList::new(
            self.max_matches,
            pattern,
            Matcher::new(nucleo_matcher::Config::DEFAULT),
        );

        // Process files with early termination
        let mut processed = 0;
        for file_path in &self.file_suggestions {
            best_matches.insert(file_path);
            processed += 1;

            // Early termination: if we have enough high-quality matches, stop
            if processed > self.max_matches * 10
                && best_matches.binary_heap.len() >= self.max_matches
            {
                break;
            }
        }

        self.filtered_files = best_matches.get_sorted_matches();
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
        // Keep file_suggestions for performance
    }

    /// Check if currently in file file_search mode
    pub fn is_active(&self) -> bool {
        self.is_file_mode
    }

    /// Clear all caches (call this when directory changes)
    pub fn clear_caches(&mut self) {
        self.file_suggestions.clear();
        self.last_directory = None;
    }

    /// Force reload files from directory (useful when files are created/deleted)
    pub fn force_reload_files(&mut self, dir: &Path) {
        self.clear_caches();
        self.scan_directory(dir);
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
    if state.file_search.file_suggestions.is_empty()
        && let Ok(current_dir) = std::env::current_dir()
    {
        state.file_search.scan_directory(&current_dir);
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
    if file_search.file_suggestions.is_empty()
        && let Ok(current_dir) = std::env::current_dir()
    {
        file_search.scan_directory(&current_dir);
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
    _helpers: Vec<HelperEntry>, // Unused: we re-scan dynamically when input starts with '/'
    mut file_search: FileSearch,
    commands_config: Option<CommandsConfig>,
) {
    while let Some((input, cursor_position)) = rx.recv().await {
        // Load files if not already loaded (uses cache if same directory)
        if let Ok(current_dir) = std::env::current_dir() {
            file_search.scan_directory(&current_dir);
        }

        // Filter helpers - only when input starts with '/' and is not empty.
        // Re-scan custom commands each time for dynamic reload (no restart needed).
        let (filtered_helpers, custom_commands): (Vec<HelperEntry>, Option<Vec<CustomCommand>>) =
            if input.starts_with('/') && !input.is_empty() {
                let custom = crate::services::commands::scan_custom_commands(
                    commands_config.as_ref(),
                );
                let fresh_helpers = crate::services::commands::get_helper_commands(&custom);
                let search_term = input[1..].to_lowercase();
                let filtered: Vec<HelperEntry> = fresh_helpers
                    .iter()
                    .filter(|h| {
                        // Match against both command ID and display string
                        h.command().to_lowercase().contains(&search_term)
                            || h.display().to_lowercase().contains(&search_term)
                    })
                    .cloned()
                    .collect();
                (filtered, Some(custom))
            } else {
                (Vec::new(), None)
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

        let _ = tx
            .send(FileSearchResult {
                filtered_helpers,
                filtered_files,
                cursor_position,
                input,
                custom_commands,
            })
            .await;
    }
}
