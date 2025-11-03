use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use walkdir::WalkDir;

/// Represents a file's content and metadata for tracking changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBuffer {
    pub content: String,
    pub uri: String,
    pub hash: u64,
    pub path: PathBuf,
}

/// Events that can occur during file watching
#[derive(Debug, Clone)]
pub enum FileWatchEvent {
    /// File was modified
    Modified {
        file: FileBuffer,
        old_content: String,
    },
    /// File was deleted
    Deleted { file: FileBuffer },
    /// File was created
    Created { file: FileBuffer },
    /// Raw filesystem event for custom handling
    Raw { event: Event },
}

/// Trait for filtering which files should be watched
pub trait FileFilter: Send + Sync {
    /// Returns true if the file should be watched
    fn should_watch(&self, path: &Path) -> bool;
}

/// Simple closure-based file filter
pub struct ClosureFilter<F>
where
    F: Fn(&Path) -> bool + Send + Sync,
{
    filter_fn: F,
}

impl<F> ClosureFilter<F>
where
    F: Fn(&Path) -> bool + Send + Sync,
{
    pub fn new(filter_fn: F) -> Self {
        Self { filter_fn }
    }
}

impl<F> FileFilter for ClosureFilter<F>
where
    F: Fn(&Path) -> bool + Send + Sync,
{
    fn should_watch(&self, path: &Path) -> bool {
        (self.filter_fn)(path)
    }
}

/// Main file watcher that can watch directories for changes
pub struct FileWatcher {
    watch_dir: PathBuf,
    watched_files: HashMap<String, FileBuffer>,
    filter: Arc<dyn FileFilter>,
    watcher: Option<RecommendedWatcher>,
}

impl FileWatcher {
    /// Create a new file watcher
    pub fn new<F>(watch_dir: PathBuf, filter: F) -> Self
    where
        F: FileFilter + 'static,
    {
        Self {
            watch_dir,
            watched_files: HashMap::new(),
            filter: Arc::new(filter),
            watcher: None,
        }
    }

    /// Initialize the watcher and scan for existing files
    pub fn initialize(&mut self) -> Result<(), String> {
        self.watched_files = self.scan_directory()?;
        Ok(())
    }

    /// Start watching the directory and return a receiver for processed events
    pub async fn start_watching(&mut self) -> Result<mpsc::Receiver<FileWatchEvent>, String> {
        let (processed_tx, processed_rx) = mpsc::channel(100);
        let (raw_tx, mut raw_rx) = mpsc::unbounded_channel();

        let watch_dir = self.watch_dir.clone();
        let filter = Arc::clone(&self.filter);
        let raw_tx_clone = raw_tx.clone();

        // Create the filesystem watcher
        let watcher: Result<RecommendedWatcher, notify::Error> = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| {
                if let Ok(event) = result {
                    // Filter events based on paths
                    let should_process = event
                        .paths
                        .iter()
                        .any(|path| path.is_file() && filter.should_watch(path));

                    if should_process {
                        let _ = raw_tx_clone.send(event);
                    }
                }
            },
            Config::default(),
        );
        let mut watcher = watcher.map_err(|e| format!("Failed to create watcher: {}", e))?;

        // Start watching
        watcher
            .watch(&watch_dir, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch directory: {}", e))?;

        self.watcher = Some(watcher);

        // Spawn background task to process raw events
        let watch_dir_clone = self.watch_dir.clone();
        let filter_clone = Arc::clone(&self.filter);
        let watched_files = self.watched_files.clone();

        tokio::spawn(async move {
            let mut internal_watcher = InternalEventProcessor {
                watch_dir: watch_dir_clone,
                watched_files,
                filter: filter_clone,
                processed_tx,
            };

            while let Some(raw_event) = raw_rx.recv().await {
                if let Err(e) = internal_watcher.process_event(raw_event).await {
                    eprintln!("Error processing file watch event: {}", e);
                }
            }
        });

        Ok(processed_rx)
    }

    /// Get current watched files (snapshot at initialization)
    pub fn get_watched_files(&self) -> &HashMap<String, FileBuffer> {
        &self.watched_files
    }

    /// Get the directory being watched
    pub fn watch_dir(&self) -> &Path {
        &self.watch_dir
    }

    /// Scan directory for existing files
    fn scan_directory(&self) -> Result<HashMap<String, FileBuffer>, String> {
        let mut files = HashMap::new();

        for entry in WalkDir::new(&self.watch_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file() && self.filter.should_watch(entry.path()))
        {
            let path = entry.path();
            if let Ok(buffer) = self.create_file_buffer(path) {
                files.insert(buffer.uri.clone(), buffer);
            }
        }

        Ok(files)
    }

    /// Create a file buffer from a path
    fn create_file_buffer(&self, path: &Path) -> Result<FileBuffer, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file {}: {}", path.display(), e))?;

        let hash = self.hash_content(&content);
        let uri = self.path_to_uri(path);

        // Use canonical path for consistency
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        Ok(FileBuffer {
            content,
            uri,
            hash,
            path: canonical_path,
        })
    }

    /// Convert path to URI
    fn path_to_uri(&self, path: &Path) -> String {
        // Use canonical path to ensure consistency across all platforms
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Create absolute URI instead of relative
        format!(
            "file://{}",
            canonical_path.to_string_lossy().replace('\\', "/")
        )
    }

    /// Hash file content
    fn hash_content(&self, content: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }
}

/// Internal event processor that handles raw events and produces processed events
struct InternalEventProcessor {
    #[allow(dead_code)]
    watch_dir: PathBuf,
    watched_files: HashMap<String, FileBuffer>,
    filter: Arc<dyn FileFilter>,
    processed_tx: mpsc::Sender<FileWatchEvent>,
}

impl InternalEventProcessor {
    /// Process a raw filesystem event and send processed events
    async fn process_event(&mut self, event: Event) -> Result<(), String> {
        let mut events_to_send = Vec::new();

        // Handle deletions first
        self.process_deletions(&mut events_to_send);

        // Handle modifications and creations
        self.process_modifications(&event, &mut events_to_send)?;

        // Send all processed events
        for event in events_to_send {
            if self.processed_tx.send(event).await.is_err() {
                // Channel was closed, stop processing
                return Err("Event channel closed".to_string());
            }
        }

        Ok(())
    }

    /// Process file deletions
    fn process_deletions(&mut self, events: &mut Vec<FileWatchEvent>) {
        let mut to_remove = Vec::new();

        for (uri, buffer) in &self.watched_files {
            if !buffer.path.exists() {
                events.push(FileWatchEvent::Deleted {
                    file: buffer.clone(),
                });
                to_remove.push(uri.clone());
            }
        }

        for uri in to_remove {
            self.watched_files.remove(&uri);
        }
    }

    /// Process file modifications and creations
    fn process_modifications(
        &mut self,
        event: &Event,
        events: &mut Vec<FileWatchEvent>,
    ) -> Result<(), String> {
        for path in &event.paths {
            if !path.is_file() || !self.filter.should_watch(path) {
                continue;
            }

            let uri = self.path_to_uri(path);

            match self.create_file_buffer(path) {
                Ok(new_buffer) => {
                    if let Some(old_buffer) = self.watched_files.get(&uri) {
                        // File exists and was modified
                        if old_buffer.hash != new_buffer.hash {
                            events.push(FileWatchEvent::Modified {
                                file: new_buffer.clone(),
                                old_content: old_buffer.content.clone(),
                            });
                            self.watched_files.insert(uri, new_buffer);
                        }
                    } else {
                        // New file created
                        events.push(FileWatchEvent::Created {
                            file: new_buffer.clone(),
                        });
                        self.watched_files.insert(uri, new_buffer);
                    }
                }
                Err(_) => {
                    // File might have been deleted
                    if let Some(old_buffer) = self.watched_files.remove(&uri) {
                        events.push(FileWatchEvent::Deleted { file: old_buffer });
                    }
                }
            }
        }

        Ok(())
    }

    /// Create a file buffer from a path
    fn create_file_buffer(&self, path: &Path) -> Result<FileBuffer, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file {}: {}", path.display(), e))?;

        let hash = self.hash_content(&content);
        let uri = self.path_to_uri(path);

        // Use canonical path for consistency
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        Ok(FileBuffer {
            content,
            uri,
            hash,
            path: canonical_path,
        })
    }

    /// Convert path to URI
    fn path_to_uri(&self, path: &Path) -> String {
        // Use canonical path to ensure consistency across all platforms
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Create absolute URI instead of relative
        format!(
            "file://{}",
            canonical_path.to_string_lossy().replace('\\', "/")
        )
    }

    /// Hash file content
    fn hash_content(&self, content: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }
}

/// Convenience function to create a file watcher with closure-based filter
pub fn create_file_watcher<F>(watch_dir: PathBuf, filter: F) -> Result<FileWatcher, String>
where
    F: Fn(&Path) -> bool + Send + Sync + 'static,
{
    let filter = ClosureFilter::new(filter);
    let watcher = FileWatcher::new(watch_dir, filter);
    Ok(watcher)
}

/// Convenience function to create and start a file watcher, returning the event receiver
pub async fn create_and_start_watcher<F>(
    watch_dir: PathBuf,
    filter: F,
) -> Result<(FileWatcher, mpsc::Receiver<FileWatchEvent>), String>
where
    F: Fn(&Path) -> bool + Send + Sync + 'static,
{
    let mut watcher = create_file_watcher(watch_dir, filter)?;
    watcher.initialize()?;
    let receiver = watcher.start_watching().await?;
    Ok((watcher, receiver))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::time::Duration;

    // Helper function to create a test directory with some files
    fn create_test_directory() -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let temp_path = temp_dir.path();

        // Create some test files
        fs::write(temp_path.join("test1.txt"), "content1").expect("Failed to write test1.txt");
        fs::write(temp_path.join("test2.rs"), "fn main() {}").expect("Failed to write test2.rs");
        fs::write(temp_path.join("ignore.log"), "log content").expect("Failed to write ignore.log");

        // Create subdirectory with files
        let sub_dir = temp_path.join("subdir");
        fs::create_dir(&sub_dir).expect("Failed to create subdirectory");
        fs::write(sub_dir.join("nested.txt"), "nested content")
            .expect("Failed to write nested.txt");

        temp_dir
    }

    // Simple test filter that only watches .txt and .rs files
    fn test_filter(path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            matches!(ext.to_str(), Some("txt") | Some("rs"))
        } else {
            false
        }
    }

    #[test]
    fn test_file_filter_trait() {
        let filter = ClosureFilter::new(test_filter);

        assert!(filter.should_watch(Path::new("test.txt")));
        assert!(filter.should_watch(Path::new("test.rs")));
        assert!(!filter.should_watch(Path::new("test.log")));
        assert!(!filter.should_watch(Path::new("test")));
    }

    #[test]
    fn test_file_watcher_creation() {
        let temp_dir = create_test_directory();
        let filter = ClosureFilter::new(test_filter);

        let watcher = FileWatcher::new(temp_dir.path().to_path_buf(), filter);

        assert_eq!(watcher.watch_dir(), temp_dir.path());
        assert_eq!(watcher.get_watched_files().len(), 0); // Not initialized yet
    }

    #[test]
    fn test_file_watcher_initialization() {
        let temp_dir = create_test_directory();
        let filter = ClosureFilter::new(test_filter);

        let mut watcher = FileWatcher::new(temp_dir.path().to_path_buf(), filter);
        watcher.initialize().expect("Failed to initialize watcher");

        let watched_files = watcher.get_watched_files();

        // Should have 3 files: test1.txt, test2.rs, and nested.txt (filtered by extension)
        assert_eq!(watched_files.len(), 3);

        // Check that files are properly tracked
        let file_names: Vec<_> = watched_files
            .values()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();

        assert!(file_names.contains(&"test1.txt"));
        assert!(file_names.contains(&"test2.rs"));
        assert!(file_names.contains(&"nested.txt"));
    }

    #[tokio::test]
    async fn test_create_and_start_watcher() {
        let temp_dir = create_test_directory();

        let (watcher, _rx) = create_and_start_watcher(temp_dir.path().to_path_buf(), test_filter)
            .await
            .expect("Failed to create and start watcher");

        // Should have the same files as the basic test
        assert_eq!(watcher.get_watched_files().len(), 3);
        assert_eq!(watcher.watch_dir(), temp_dir.path());
    }

    #[tokio::test]
    async fn test_real_file_creation_detection() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        let (_watcher, mut event_rx) =
            create_and_start_watcher(temp_dir.path().to_path_buf(), test_filter)
                .await
                .expect("Failed to create and start watcher");

        // Give the watcher a moment to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Create a new file
        let new_file = temp_dir.path().join("new_test.txt");
        fs::write(&new_file, "new file content").expect("Failed to create new file");
        let new_file_canonical = new_file
            .canonicalize()
            .expect("Failed to canonicalize path");

        // Wait for processed events
        let mut creation_detected = false;
        let timeout = tokio::time::Instant::now() + Duration::from_secs(2);

        while tokio::time::Instant::now() < timeout && !creation_detected {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    if let FileWatchEvent::Created { file } = event
                        && file.path == new_file_canonical {
                            assert_eq!(file.content, "new file content");
                            creation_detected = true;
                            break;
                        }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    // Continue waiting
                }
            }
        }

        assert!(creation_detected, "File creation was not detected");
    }

    #[tokio::test]
    async fn test_real_file_modification_detection() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        // Create initial file
        let test_file = temp_dir.path().join("modify_test.txt");
        fs::write(&test_file, "initial content").expect("Failed to create initial file");
        let test_file_canonical = test_file
            .canonicalize()
            .expect("Failed to canonicalize path");

        let (_watcher, mut event_rx) =
            create_and_start_watcher(temp_dir.path().to_path_buf(), test_filter)
                .await
                .expect("Failed to create and start watcher");

        // Give the watcher a moment to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Modify the file
        fs::write(&test_file, "modified content").expect("Failed to modify file");

        // Wait for processed events
        let mut modification_detected = false;
        let timeout = tokio::time::Instant::now() + Duration::from_secs(2);

        while tokio::time::Instant::now() < timeout && !modification_detected {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    if let FileWatchEvent::Modified { file, old_content } = event
                        && file.path == test_file_canonical {
                            assert_eq!(file.content, "modified content");
                            assert_eq!(old_content, "initial content");
                            modification_detected = true;
                            break;
                        }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    // Continue waiting
                }
            }
        }

        assert!(modification_detected, "File modification was not detected");
    }

    #[tokio::test]
    async fn test_file_filter_in_real_watching() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        let (_watcher, mut event_rx) = create_and_start_watcher(
            temp_dir.path().to_path_buf(),
            test_filter, // Only watches .txt and .rs files
        )
        .await
        .expect("Failed to create and start watcher");

        // Give the watcher a moment to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Create files with different extensions
        let txt_file = temp_dir.path().join("watched.txt");
        let log_file = temp_dir.path().join("ignored.log");

        fs::write(&txt_file, "should be watched").expect("Failed to create txt file");
        fs::write(&log_file, "should be ignored").expect("Failed to create log file");

        let txt_file_canonical = txt_file
            .canonicalize()
            .expect("Failed to canonicalize txt file path");
        let log_file_canonical = log_file
            .canonicalize()
            .expect("Failed to canonicalize log file path");

        // Wait for processed events
        let mut txt_detected = false;
        let mut log_detected = false;
        let timeout = tokio::time::Instant::now() + Duration::from_secs(2);

        while tokio::time::Instant::now() < timeout && !txt_detected {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    if let FileWatchEvent::Created { file } = event {
                        if file.path == txt_file_canonical {
                            txt_detected = true;
                        } else if file.path == log_file_canonical {
                            log_detected = true;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    // Continue waiting
                }
            }
        }

        assert!(txt_detected, "TXT file creation should be detected");
        assert!(!log_detected, "LOG file creation should be filtered out");
    }
}
