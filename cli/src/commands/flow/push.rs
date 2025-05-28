use std::collections::{HashMap, HashSet};

use chrono::Utc;
use walkdir::WalkDir;

use stakpak_api::{
    Client, Edit, SaveEditsResponse,
    models::{Document, FlowRef},
};

pub async fn push(
    client: &Client,
    flow_ref: String,
    create: bool,
    dir: Option<String>,
    ignore_delete: bool,
    auto_approve: bool,
) -> Result<Option<SaveEditsResponse>, String> {
    let flow_ref = parse_flow_ref(flow_ref, create, client).await?;

    println!("Pushing to flow version: {}\n", flow_ref);

    let base_dir = dir.unwrap_or_else(|| ".".into());
    let documents_map = fetch_flow_documents(client, &flow_ref).await?;
    let (edits, files_synced, files_deleted) =
        process_directory(&base_dir, &documents_map, ignore_delete).await?;

    if files_synced + files_deleted == 0 {
        println!("No changes found");
        return Ok(None);
    }

    println!("\nSyncing {} files", files_synced);
    println!("Deleting {} files", files_deleted);

    if !auto_approve && !create && !confirm_action()? {
        return Ok(None);
    }

    Ok(Some(client.save_edits(&flow_ref, edits).await?))
}

async fn parse_flow_ref(
    flow_ref: String,
    create: bool,
    client: &Client,
) -> Result<FlowRef, String> {
    let parts: Vec<&str> = flow_ref.split('/').collect();
    match parts.len() {
        3 => Ok(FlowRef::Version {
            owner_name: parts[0].to_string(),
            flow_name: parts[1].to_string(),
            version_id: parts[2].to_string(),
        }),
        2 => {
            let owner_name = parts[0];
            let flow_name = parts[1];
            if create {
                let result = client.create_flow(flow_name, None).await?;
                println!("Created flow: {}/{}", result.owner_name, result.flow_name);
                Ok(FlowRef::Version {
                    owner_name: result.owner_name,
                    flow_name: result.flow_name,
                    version_id: result.version_id.to_string(),
                })
            } else {
                let result = client.get_flow(owner_name, flow_name).await?;
                let latest_version = result
                    .resource
                    .versions
                    .iter()
                    .max_by_key(|v| v.created_at)
                    .ok_or("No versions found")?;
                Ok(FlowRef::Version {
                    owner_name: owner_name.to_string(),
                    flow_name: flow_name.to_string(),
                    version_id: latest_version.id.to_string(),
                })
            }
        }
        _ => FlowRef::new(flow_ref).map_err(|e| format!("Failed to parse flow ref: {}", e)),
    }
}

async fn fetch_flow_documents(
    client: &Client,
    flow_ref: &FlowRef,
) -> Result<HashMap<String, Document>, String> {
    let documents = client.get_flow_documents(flow_ref).await?.documents;
    Ok(documents
        .into_iter()
        .map(|doc| (doc.uri.clone(), doc))
        .collect())
}

pub fn is_supported_file(file_name: Option<&str>, is_file: bool) -> bool {
    match file_name {
        Some(name) => {
            // Skip hidden files/dirs that aren't just "."
            if name.starts_with('.') && name.len() > 1 {
                return false;
            }
            // Only allow supported files
            if is_file {
                name.ends_with(".tf")
                    || name.ends_with(".yaml")
                    || name.ends_with(".yml")
                    || name.to_lowercase().contains("dockerfile")
            } else {
                true // Allow directories to be traversed
            }
        }
        None => false,
    }
}

async fn process_directory(
    base_dir: &str,
    documents_map: &HashMap<String, Document>,
    ignore_delete: bool,
) -> Result<(Vec<Edit>, usize, usize), String> {
    let mut edits = Vec::new();
    let mut processed_uris = HashSet::new();
    let mut files_synced = 0;
    let mut files_deleted = 0;

    for entry in WalkDir::new(base_dir)
        .into_iter()
        .filter_entry(|e| {
            // Skip hidden directories and non-supported files
            let file_name = e.file_name().to_str();
            is_supported_file(file_name, e.file_type().is_file())
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let content = std::fs::read_to_string(path).map_err(|_| "Failed to read file")?;
        let document_uri = format!(
            "file:///{}",
            path.strip_prefix(base_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        );
        processed_uris.insert(document_uri.clone());

        if let Some(document) = documents_map.get(&document_uri) {
            if content != document.content {
                edits.push(create_edit(&document_uri, &document.content, "delete"));
                edits.push(create_edit(&document_uri, &content, "insert"));
                files_synced += 1;
            }
        } else {
            edits.push(create_edit(&document_uri, &content, "insert"));
            files_synced += 1;
        }
    }

    if !ignore_delete {
        for (uri, document) in documents_map {
            if !processed_uris.contains(uri) {
                edits.push(create_edit(uri, &document.content, "delete"));
                files_deleted += 1;
            }
        }
    }

    Ok((edits, files_synced, files_deleted))
}

pub fn create_edit(document_uri: &str, content: &str, operation: &str) -> Edit {
    Edit {
        document_uri: document_uri.to_string(),
        start_byte: 0,
        start_row: 0,
        start_column: 0,
        end_byte: content.len(),
        end_row: content.lines().count(),
        end_column: content.lines().last().map_or(0, |line| line.len()),
        content: content.to_string(),
        language: "".to_string(),
        operation: operation.to_string(),
        timestamp: Utc::now(),
    }
}

fn confirm_action() -> Result<bool, String> {
    println!("\nDo you want to continue? Type 'yes' to confirm: ");
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("Failed to read input: {}", e))?;
    Ok(input.trim() == "yes")
}
