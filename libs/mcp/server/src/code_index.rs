use rmcp::model::RawTextContent;
use serde::{Deserialize, Serialize};
use stakpak_api::models::{BuildCodeIndexToolArgs, BuildIndexOutput, SimpleDocument};
use stakpak_api::{Client, ClientConfig, ToolsCallParams};
use stakpak_shared::local_store::LocalStore;

use tracing::{error, warn};
use walkdir::WalkDir;

use crate::utils::{read_gitignore_patterns, should_include_entry};
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

    // Log response text to debug.log for debugging
    if let Err(e) = std::fs::write("debug.log", &response_text) {
        warn!("Failed to write debug log: {}", e);
    }

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
