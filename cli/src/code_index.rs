use stakpak_api::models::{BuildCodeIndexInput, BuildCodeIndexOutput, CodeIndex, SimpleDocument};
use stakpak_api::{AgentProvider, ClientConfig, LocalClient, RemoteClient};
use stakpak_shared::file_watcher::{FileWatchEvent, create_and_start_watcher};
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::models::indexing::IndexingStatus;

use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use chrono::Utc;
use stakpak_shared::utils::{
    self, is_supported_file, read_gitignore_patterns, should_include_entry,
};

use crate::config::{AppConfig, ProviderType};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, interval};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileOperation {
    Created,
    Modified,
    Deleted,
}

impl std::fmt::Display for FileOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileOperation::Created => write!(f, "created"),
            FileOperation::Modified => write!(f, "modified"),
            FileOperation::Deleted => write!(f, "deleted"),
        }
    }
}

const INDEX_FRESHNESS_MINUTES: i64 = 10;
const MAX_AUTO_INDEX_FILES: usize = 200;

const DEBOUNCE_PROCESS_INTERVAL_SECONDS: u64 = 5;
const DEBOUNCE_DURATION_SECONDS: u64 = 15;

#[derive(Debug, Clone)]
struct PendingUpdate {
    operation: FileOperation,
    file_uri: String,
    app_config: AppConfig,
    directory: Option<String>,
    last_update_time: Instant,
}

#[derive(Debug)]
enum DebounceMessage {
    ScheduleUpdate(PendingUpdate),
}

struct DebounceActor {
    receiver: mpsc::Receiver<DebounceMessage>,
    pending_updates: HashMap<String, PendingUpdate>,
}

impl DebounceActor {
    fn new() -> (Self, mpsc::Sender<DebounceMessage>) {
        let (sender, receiver) = mpsc::channel(100);
        let actor = Self {
            receiver,
            pending_updates: HashMap::new(),
        };
        (actor, sender)
    }

    async fn run(mut self) {
        let mut process_interval = interval(Duration::from_secs(DEBOUNCE_PROCESS_INTERVAL_SECONDS)); // Check every 5 seconds

        loop {
            tokio::select! {
                // Handle incoming messages
                message = self.receiver.recv() => {
                    match message {
                        Some(DebounceMessage::ScheduleUpdate(update)) => {
                            self.handle_schedule_update(update).await;
                        }
                        None => {
                            debug!("Debounce actor channel closed, shutting down");
                            break;
                        }
                    }
                }

                // Periodic processing of pending updates
                _ = process_interval.tick() => {
                    self.process_pending_updates().await;
                }
            }
        }
    }

    async fn handle_schedule_update(&mut self, update: PendingUpdate) {
        let key = format!("{}:{}", update.operation, update.file_uri);
        debug!(
            "Actor scheduling debounced update for {} operation on {}",
            update.operation, update.file_uri
        );
        self.pending_updates.insert(key, update);
    }

    async fn process_pending_updates(&mut self) {
        let now = Instant::now();
        let mut to_process = Vec::new();
        let mut to_remove = Vec::new();

        // Find updates that are ready to process
        for (key, update) in &self.pending_updates {
            if now.duration_since(update.last_update_time)
                >= Duration::from_secs(DEBOUNCE_DURATION_SECONDS)
            {
                to_process.push(update.clone());
                to_remove.push(key.clone());
            }
        }

        // Remove processed updates from pending map
        for key in to_remove {
            self.pending_updates.remove(&key);
        }

        // Process updates sequentially
        for update in to_process {
            info!(
                "Actor processing debounced update for {} operation on {}",
                update.operation, update.file_uri
            );

            if let Err(e) = execute_code_index_update(
                &update.app_config,
                &update.directory,
                update.operation,
                &update.file_uri,
            )
            .await
            {
                error!("Failed to process debounced update: {}", e);
            }
        }
    }
}

// Global actor sender
static DEBOUNCE_ACTOR_SENDER: OnceLock<mpsc::Sender<DebounceMessage>> = OnceLock::new();

fn get_debounce_actor_sender() -> &'static mpsc::Sender<DebounceMessage> {
    DEBOUNCE_ACTOR_SENDER.get_or_init(|| {
        let (actor, sender) = DebounceActor::new();

        // Spawn the actor
        tokio::spawn(async move {
            info!("Starting debounce actor");
            actor.run().await;
            info!("Debounce actor shutdown");
        });

        sender
    })
}

pub async fn get_or_build_local_code_index(
    app_config: &AppConfig,
    directory: Option<String>,
    index_big_project: bool,
) -> Result<CodeIndex, String> {
    // Set the directory to use
    let dir = directory.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });

    // First, count supported files to see if we should proceed
    let file_count = count_supported_files(&dir)?;

    if file_count > MAX_AUTO_INDEX_FILES && !index_big_project {
        // Store the indexing status
        let status = IndexingStatus {
            indexed: false,
            reason: format!(
                "Directory contains {} supported files (>{} threshold). Use --index-big-project to enable indexing.",
                file_count, MAX_AUTO_INDEX_FILES
            ),
            file_count,
            timestamp: Utc::now(),
        };
        store_indexing_status(&status)?;

        warn!("Skipping code indexing: {}", status.reason);
        return Err(status.reason);
    }

    // Try to load existing index
    match load_existing_index() {
        Ok(index) if is_index_fresh(&index) => {
            // Index exists and is fresh (less than 10 minutes old)
            let status = IndexingStatus {
                indexed: true,
                reason: "Using existing fresh index".to_string(),
                file_count,
                timestamp: index.last_updated,
            };
            store_indexing_status(&status)?;
            Ok(index)
        }
        Ok(_) => {
            // Index exists but is stale, rebuild it
            warn!("Code index is older than 10 minutes, rebuilding...");
            rebuild_and_load_index(app_config, Some(dir), file_count).await
        }
        Err(_) => {
            // No index exists or failed to load, build a new one
            rebuild_and_load_index(app_config, Some(dir), file_count).await
        }
    }
}

/// Count supported files in directory
fn count_supported_files(base_dir: &str) -> Result<usize, String> {
    let mut count = 0;
    let ignore_patterns = read_gitignore_patterns(base_dir);

    for entry in WalkDir::new(base_dir)
        .into_iter()
        .filter_entry(|e| should_include_entry(e, base_dir, &ignore_patterns))
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && is_supported_file(entry.path()) {
            count += 1;
            // Early exit if we've already exceeded the threshold to avoid counting millions of files
            if count > MAX_AUTO_INDEX_FILES * 2 {
                break;
            }
        }
    }

    Ok(count)
}

/// Store indexing status for use by tools
fn store_indexing_status(status: &IndexingStatus) -> Result<(), String> {
    let status_json = serde_json::to_string_pretty(status)
        .map_err(|e| format!("Failed to serialize indexing status: {}", e))?;
    LocalStore::write_session_data("indexing_status.json", &status_json)
        .map_err(|e| format!("Failed to store indexing status: {}", e))?;
    Ok(())
}

/// Load existing index from local storage
fn load_existing_index() -> Result<CodeIndex, String> {
    let index_str = LocalStore::read_session_data("code_index.json")
        .map_err(|e| format!("Failed to read code index: {}", e))?;

    if index_str.is_empty() {
        return Err("Code index is empty".to_string());
    }

    parse_code_index(&index_str)
}

/// Parse code index from JSON string
fn parse_code_index(index_str: &str) -> Result<CodeIndex, String> {
    serde_json::from_str(index_str).map_err(|e| {
        error!("Failed to parse code index: {}", e);
        format!("Failed to parse code index: {}", e)
    })
}

/// Check if the index is fresh (less than 10 minutes old)
fn is_index_fresh(index: &CodeIndex) -> bool {
    let now = Utc::now();
    let ten_minutes_ago = now - chrono::Duration::minutes(INDEX_FRESHNESS_MINUTES);
    index.last_updated >= ten_minutes_ago
}

/// Rebuild the index and load it from storage
async fn rebuild_and_load_index(
    app_config: &AppConfig,
    directory: Option<String>,
    file_count: usize,
) -> Result<CodeIndex, String> {
    build_local_code_index(app_config, directory).await?;
    let index = load_existing_index()?;

    // Store successful indexing status
    let status = IndexingStatus {
        indexed: true,
        reason: format!("Successfully indexed {} files", file_count),
        file_count,
        timestamp: index.last_updated,
    };
    store_indexing_status(&status)?;

    Ok(index)
}

/// Build local code index
async fn build_local_code_index(
    app_config: &AppConfig,
    directory: Option<String>,
) -> Result<usize, String> {
    let directory = directory.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });

    let client: Box<dyn AgentProvider> = match app_config.provider {
        ProviderType::Remote => {
            let client = RemoteClient::new(&ClientConfig {
                api_key: app_config.api_key.clone(),
                api_endpoint: app_config.api_endpoint.clone(),
            })
            .map_err(|e| e.to_string())?;
            Box::new(client)
        }
        ProviderType::Local => Box::new(LocalClient),
    };

    let documents = process_directory(&directory)?;

    let index = client
        .build_code_index(&BuildCodeIndexInput { documents })
        .await?;

    // Create CodeIndex with timestamp
    let code_index = CodeIndex {
        last_updated: Utc::now(),
        index,
    };

    // Write code_index to .stakpak/code_index.json
    let index_json = serde_json::to_string_pretty(&code_index).map_err(|e| {
        error!("Failed to serialize code index: {}", e);
        format!("Failed to serialize code index: {}", e)
    })?;

    LocalStore::write_session_data("code_index.json", &index_json)?;

    Ok(code_index.index.blocks.len())
}

fn process_directory(base_dir: &str) -> Result<Vec<SimpleDocument>, String> {
    let mut documents = Vec::new();

    // Read .gitignore patterns
    let ignore_patterns = read_gitignore_patterns(base_dir);

    for entry in WalkDir::new(base_dir)
        .into_iter()
        .filter_entry(|e| should_include_entry(e, base_dir, &ignore_patterns))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let content = std::fs::read_to_string(path).map_err(|_| "Failed to read file")?;

        // Get absolute path and create consistent URI
        let absolute_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        documents.push(SimpleDocument {
            uri: format!("file://{}", absolute_path.to_string_lossy()),
            content,
        });
    }

    Ok(documents)
}

pub fn start_code_index_watcher(
    app_config: &AppConfig,
    directory: Option<String>,
) -> Result<JoinHandle<Result<(), String>>, String> {
    let watch_dir = directory.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });

    let watch_path = PathBuf::from(&watch_dir);

    // Read gitignore patterns for filtering
    let ignore_patterns = read_gitignore_patterns(&watch_dir);

    // Create file filter that combines gitignore patterns and supported file types
    let watch_dir_clone = watch_dir.clone();
    let filter = move |path: &Path| -> bool {
        // Get relative path from base directory to match gitignore patterns
        let base_path = PathBuf::from(&watch_dir_clone);
        let relative_path = match path.strip_prefix(&base_path) {
            Ok(rel_path) => rel_path,
            Err(_) => path,
        };
        let path_str = relative_path.to_string_lossy();

        // Check gitignore patterns
        for pattern in &ignore_patterns {
            if utils::matches_gitignore_pattern(pattern, &path_str) {
                return false;
            }
        }

        is_supported_file(path)
    };

    info!(
        "Starting code index file watcher for directory: {}",
        watch_dir
    );

    let app_config = app_config.clone();
    // Spawn background task
    let handle = tokio::spawn(async move {
        // Create the file watcher with channel
        let (_watcher, mut event_receiver) = create_and_start_watcher(watch_path, filter)
            .await
            .map_err(|e| format!("Failed to create file watcher: {}", e))?;

        info!("Code index file watcher started successfully");

        // Main event loop - handle processed file watch events
        while let Some(watch_event) = event_receiver.recv().await {
            if let Err(e) =
                handle_code_index_update_event(&app_config, &directory, watch_event).await
            {
                error!("Error handling code index update: {}", e);
            }
        }

        warn!("File watcher channel closed, stopping watcher");

        Ok(())
    });

    Ok(handle)
}

async fn handle_code_index_update_event(
    app_config: &AppConfig,
    directory: &Option<String>,
    event: FileWatchEvent,
) -> Result<(), String> {
    match event {
        FileWatchEvent::Created { file } => {
            update_code_index(app_config, directory, FileOperation::Created, &file.uri).await
        }
        FileWatchEvent::Modified {
            file,
            old_content: _,
        } => update_code_index(app_config, directory, FileOperation::Modified, &file.uri).await,
        FileWatchEvent::Deleted { file } => {
            update_code_index(app_config, directory, FileOperation::Deleted, &file.uri).await
        }
        FileWatchEvent::Raw { event } => {
            debug!("Raw filesystem event: {:?}", event);
            // Usually we don't need to handle raw events as they're processed into the above variants
            Ok(())
        }
    }
}

/// Find all document URIs that the given document depends on
fn find_document_dependencies(index: &BuildCodeIndexOutput, document_uri: &str) -> HashSet<String> {
    index
        .blocks
        .iter()
        .filter(|block| block.document_uri == document_uri)
        .flat_map(|block| &block.dependencies)
        .filter(|dep| dep.satisfied)
        .filter_map(|dep| dep.key.as_ref())
        .filter_map(|dep_key| {
            index
                .blocks
                .iter()
                .find(|other_block| {
                    other_block.key == *dep_key && other_block.document_uri != document_uri
                })
                .map(|other_block| other_block.document_uri.clone())
        })
        .collect::<HashSet<String>>()
}

/// Find all document URIs that depend on the given document
fn find_document_dependents(index: &BuildCodeIndexOutput, document_uri: &str) -> HashSet<String> {
    // Find all blocks in this document
    let document_block_keys: HashSet<String> = index
        .blocks
        .iter()
        .filter(|block| block.document_uri == document_uri)
        .map(|block| block.key.clone())
        .collect();

    // Find all blocks that depend on any block in this document
    index
        .blocks
        .iter()
        .filter(|block| block.document_uri != document_uri)
        .filter(|block| {
            block.dependencies.iter().any(|dep| {
                dep.satisfied
                    && dep
                        .key
                        .as_ref()
                        .map(|dep_key| document_block_keys.contains(dep_key))
                        .unwrap_or(false)
            })
        })
        .map(|block| block.document_uri.clone())
        .collect()
}

/// Read the content of a file, handling file:// URIs
fn read_file_content(uri: &str) -> Result<String, String> {
    let file_path = if uri.starts_with("file://") {
        uri.strip_prefix("file://").unwrap_or(uri)
    } else {
        uri
    };

    std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read file {}: {}", file_path, e))
}

/// Convert a file path to a file:// URI format
fn path_to_uri(path: &str) -> String {
    if path.starts_with("file://") {
        path.to_string()
    } else {
        // Convert to absolute path for consistency
        let path_buf = std::path::Path::new(path);
        let absolute_path = path_buf.canonicalize().unwrap_or_else(|_| {
            // If canonicalize fails, try to make it absolute relative to current dir
            if path_buf.is_absolute() {
                path_buf.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join(path_buf)
            }
        });
        format!("file://{}", absolute_path.to_string_lossy())
    }
}

/// Merge new index results into existing index, replacing blocks for specified documents
fn merge_index_results(
    existing_index: &mut BuildCodeIndexOutput,
    new_index: BuildCodeIndexOutput,
    updated_document_uris: &HashSet<String>,
) {
    // Remove all blocks from documents that were re-indexed
    existing_index
        .blocks
        .retain(|block| !updated_document_uris.contains(&block.document_uri));

    // Add all new blocks
    existing_index.blocks.extend(new_index.blocks);

    // Merge errors and warnings (keep existing ones for non-updated documents)
    existing_index
        .errors
        .retain(|error| !updated_document_uris.contains(&error.uri));
    existing_index.errors.extend(new_index.errors);

    existing_index
        .warnings
        .retain(|warning| !updated_document_uris.contains(&warning.uri));
    existing_index.warnings.extend(new_index.warnings);
}

async fn update_code_index(
    app_config: &AppConfig,
    directory: &Option<String>,
    operation: FileOperation,
    file_uri: &str,
) -> Result<(), String> {
    // Use the actor-based debouncing mechanism
    let actor_sender = get_debounce_actor_sender();

    let pending_update = PendingUpdate {
        operation,
        file_uri: file_uri.to_string(),
        app_config: app_config.clone(),
        directory: directory.clone(),
        last_update_time: Instant::now(),
    };

    debug!(
        "Sending update to debounce actor for {} operation on {}",
        operation, file_uri
    );

    actor_sender
        .send(DebounceMessage::ScheduleUpdate(pending_update))
        .await
        .map_err(|e| format!("Failed to send message to debounce actor: {}", e))?;

    Ok(())
}

async fn execute_code_index_update(
    app_config: &AppConfig,
    _directory: &Option<String>,
    operation: FileOperation,
    file_uri: &str,
) -> Result<(), String> {
    info!(
        "Executing code index update for {} operation on {}",
        operation, file_uri
    );

    // Load existing index
    let mut existing_index = match load_existing_index() {
        Ok(index) => index,
        Err(e) => {
            warn!(
                "Failed to load existing index for incremental update: {}. Building fresh index.",
                e
            );
            return Ok(()); // Let the next request trigger a full rebuild
        }
    };

    let file_uri_normalized = path_to_uri(file_uri);
    let mut documents_to_reindex = HashSet::new();

    match operation {
        FileOperation::Created | FileOperation::Modified => {
            // Add the changed document
            documents_to_reindex.insert(file_uri_normalized.clone());

            // Find dependencies and dependents to maintain consistency
            let dependencies =
                find_document_dependencies(&existing_index.index, &file_uri_normalized);
            let dependents = find_document_dependents(&existing_index.index, &file_uri_normalized);

            documents_to_reindex.extend(dependencies);
            documents_to_reindex.extend(dependents);

            info!(
                "Re-indexing {} documents due to {} {}",
                documents_to_reindex.len(),
                operation,
                file_uri
            );

            // Read content for all documents that need re-indexing
            let mut documents = Vec::new();
            for doc_uri in &documents_to_reindex {
                match read_file_content(doc_uri) {
                    Ok(content) => {
                        documents.push(SimpleDocument {
                            uri: doc_uri.clone(),
                            content,
                        });
                    }
                    Err(e) => {
                        warn!("Failed to read document {} for re-indexing: {}", doc_uri, e);
                        // Continue with other documents
                    }
                }
            }

            if documents.is_empty() {
                warn!("No documents to re-index");
                return Ok(());
            }

            // Call the indexing API
            let client: Box<dyn AgentProvider> = match app_config.provider {
                ProviderType::Remote => {
                    let client = RemoteClient::new(&ClientConfig {
                        api_key: app_config.api_key.clone(),
                        api_endpoint: app_config.api_endpoint.clone(),
                    })
                    .map_err(|e| e.to_string())?;
                    Box::new(client)
                }
                ProviderType::Local => Box::new(LocalClient),
            };

            let new_index = client
                .build_code_index(&BuildCodeIndexInput { documents })
                .await?;

            // Merge the results
            merge_index_results(&mut existing_index.index, new_index, &documents_to_reindex);
        }
        FileOperation::Deleted => {
            // Find all block keys from the deleted document
            let deleted_block_keys: HashSet<String> = existing_index
                .index
                .blocks
                .iter()
                .filter(|block| block.document_uri == file_uri_normalized)
                .map(|block| block.key.clone())
                .collect();

            info!(
                "Marking dependencies as unsatisfied due to deletion of {} (affected {} blocks)",
                file_uri,
                deleted_block_keys.len()
            );

            // Mark dependencies pointing to deleted blocks as unsatisfied
            let mut unsatisfied_count = 0;
            for block in &mut existing_index.index.blocks {
                if block.document_uri != file_uri_normalized {
                    for dep in &mut block.dependencies {
                        if dep.satisfied
                            && let Some(dep_key) = &dep.key
                            && deleted_block_keys.contains(dep_key)
                        {
                            dep.satisfied = false;
                            unsatisfied_count += 1;
                            debug!(
                                "Marked dependency {} -> {} as unsatisfied",
                                block.key, dep_key
                            );
                        }
                    }
                }
            }

            if unsatisfied_count > 0 {
                info!("Marked {} dependencies as unsatisfied", unsatisfied_count);
            }

            // Remove blocks from the deleted file
            existing_index
                .index
                .blocks
                .retain(|block| block.document_uri != file_uri_normalized);
            existing_index
                .index
                .errors
                .retain(|error| error.uri != file_uri_normalized);
            existing_index
                .index
                .warnings
                .retain(|warning| warning.uri != file_uri_normalized);
        }
    }

    // Update timestamp
    existing_index.last_updated = Utc::now();

    // Save updated index
    let index_json = serde_json::to_string_pretty(&existing_index)
        .map_err(|e| format!("Failed to serialize updated code index: {}", e))?;

    LocalStore::write_session_data("code_index.json", &index_json)?;

    info!(
        "Successfully updated code index for {} operation on {}",
        operation, file_uri
    );
    Ok(())
}
