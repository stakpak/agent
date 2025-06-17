use rmcp::model::RawTextContent;
use serde::{Deserialize, Serialize};
use stakpak_api::models::{BuildCodeIndexToolArgs, BuildIndexOutput, SimpleDocument};
use stakpak_api::{Client, ClientConfig, ToolsCallParams};
use stakpak_shared::file_watcher::{FileWatchEvent, create_and_start_watcher};
use stakpak_shared::local_store::LocalStore;

use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use crate::utils::{self, is_supported_file, read_gitignore_patterns, should_include_entry};
use chrono::{DateTime, Utc};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CodeIndex {
    pub last_updated: DateTime<Utc>,
    pub index: BuildIndexOutput,
}

const INDEX_FRESHNESS_MINUTES: i64 = 5;

pub async fn get_or_build_local_code_index(
    api_config: &ClientConfig,
    directory: Option<String>,
) -> Result<CodeIndex, String> {
    // Try to load existing index
    match load_existing_index() {
        Ok(index) if is_index_fresh(&index) => {
            // Index exists and is fresh (less than 10 minutes old)
            Ok(index)
        }
        Ok(_) => {
            // Index exists but is stale, rebuild it
            warn!("Code index is older than 10 minutes, rebuilding...");
            rebuild_and_load_index(api_config, directory).await
        }
        Err(_) => {
            // No index exists or failed to load, build a new one
            rebuild_and_load_index(api_config, directory).await
        }
    }
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
    api_config: &ClientConfig,
    directory: Option<String>,
) -> Result<CodeIndex, String> {
    build_local_code_index(api_config, directory).await?;
    load_existing_index()
}

/// Build local code index
async fn build_local_code_index(
    api_config: &ClientConfig,
    directory: Option<String>,
) -> Result<usize, String> {
    let directory = directory.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });

    let client = Client::new(api_config)?;

    let documents = process_directory(&directory)?;

    let arguments = serde_json::to_value(BuildCodeIndexToolArgs { documents })
        .map_err(|e| format!("Failed to convert documents to JSON: {}", e))?;

    let response = match client
        .call_mcp_tool(&ToolsCallParams {
            name: "build_code_index".to_string(),
            arguments,
        })
        .await
    {
        Ok(response) => response,
        Err(e) => {
            return Err(format!("Failed to build code index: {}", e));
        }
    };

    let response_text = response
        .iter()
        .map(|r| {
            if let Some(RawTextContent { text }) = r.as_text() {
                text.clone()
            } else {
                "".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("");

    let index: BuildIndexOutput = serde_json::from_str(&response_text)
        .map_err(|e| format!("Failed to parse build index output: {}", e))?;

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

        documents.push(SimpleDocument {
            uri: format!(
                "file:///{}",
                path.to_string_lossy()
                    .trim_start_matches('.')
                    .trim_start_matches('/')
            ),
            content,
        });
    }

    Ok(documents)
}

pub fn start_code_index_watcher(
    api_config: &ClientConfig,
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

    let api_config = api_config.clone();
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
                handle_code_index_update_event(&api_config, &directory, watch_event).await
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
    api_config: &ClientConfig,
    directory: &Option<String>,
    event: FileWatchEvent,
) -> Result<(), String> {
    match event {
        FileWatchEvent::Created { file } => {
            info!("File created: {}", file.uri);
            // TODO: Implement incremental index update for created file
            update_code_index_placeholder(api_config, directory, "created", &file.uri).await
        }
        FileWatchEvent::Modified {
            file,
            old_content: _,
        } => {
            info!("File modified: {}", file.uri);
            // TODO: Implement incremental index update for modified file
            update_code_index_placeholder(api_config, directory, "modified", &file.uri).await
        }
        FileWatchEvent::Deleted { file } => {
            info!("File deleted: {}", file.uri);
            // TODO: Implement incremental index update for deleted file
            update_code_index_placeholder(api_config, directory, "deleted", &file.uri).await
        }
        FileWatchEvent::Raw { event } => {
            debug!("Raw filesystem event: {:?}", event);
            // Usually we don't need to handle raw events as they're processed into the above variants
            Ok(())
        }
    }
}

async fn update_code_index_placeholder(
    _api_config: &ClientConfig,
    _directory: &Option<String>,
    _operation: &str,
    _file_uri: &str,
) -> Result<(), String> {
    // Log to debug file
    // let debug_message = format!(
    //     "[{}] {} file: {}\n",
    //     chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
    //     operation,
    //     file_uri
    // );

    // if let Err(e) = std::fs::OpenOptions::new()
    //     .create(true)
    //     .append(true)
    //     .open("debug.log")
    //     .and_then(|mut file| std::io::Write::write_all(&mut file, debug_message.as_bytes()))
    // {
    //     warn!("Failed to write to debug.log: {}", e);
    // }

    // TODO: Implement the following logic:
    // 1. For created/modified files:
    //    - Read the file content
    //    - Call the build_code_index API for just this file
    //    - Merge the result into the existing index
    //    - Update the timestamp
    //    - Save the updated index
    //
    // 2. For deleted files:
    //    - Remove the file's blocks from the existing index
    //    - Update the timestamp
    //    - Save the updated index
    //
    // 3. Consider debouncing to avoid too frequent updates
    //    - Could batch updates and process them every few seconds
    //    - Or use a more sophisticated strategy based on file type/size

    Ok(())
}

pub fn stop_code_index_watcher(handle: JoinHandle<Result<(), String>>) {
    info!("Stopping code index file watcher");
    handle.abort();
}

pub fn is_code_index_watcher_running(handle: &JoinHandle<Result<(), String>>) -> bool {
    !handle.is_finished()
}
