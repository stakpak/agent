use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use futures_util::future::BoxFuture;
use notify::{event::ModifyKind, Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rust_socketio::{
    asynchronous::{Client as SocketClient, ClientBuilder},
    Payload,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{sync::mpsc, time::sleep};
use walkdir::WalkDir;

use crate::{
    client::{
        models::{Document, FlowRef},
        Client, Edit,
    },
    commands::flow::{clone, create_edit, is_supported_file},
    config::AppConfig,
};

#[derive(Deserialize, Serialize, Debug)]
pub struct DocumentBuffer {
    pub content: String,
    pub uri: String,
    pub hash: u64,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DocumentsChange {
    pub flow_ref: String,
    pub documents: Vec<Document>,
    pub uris: HashSet<String>,
}

pub enum Change {
    Internal(Event),
    Remote(DocumentsChange),
}

pub async fn sync(
    config: &AppConfig,
    client: &Client,
    flow_ref: &FlowRef,
    dir: Option<&str>,
) -> Result<(), String> {
    // Initial setup
    clone(client, flow_ref, dir).await?;
    let dir = dir
        .map(|d| Path::new(&d).to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    // Initialize state
    let mut watched_files = initialize_watched_files(&dir);
    let (tx, mut rx) = mpsc::channel(32);

    // Set up watchers
    let mut watcher = setup_file_watcher(tx.clone())?;
    watcher
        .watch(&dir, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    subscribe_to_remote_changes(config, flow_ref, tx.clone()).await?;

    // Main event loop
    while let Some(change) = rx.recv().await {
        match change {
            Change::Internal(event) => {
                handle_internal_change(event, &dir, &mut watched_files, client, flow_ref)
                    .await
                    .ok();
            }
            Change::Remote(change) => {
                handle_remote_change(change, &dir, &mut watched_files);
            }
        }
    }

    Ok(())
}

fn initialize_watched_files(dir: &Path) -> HashMap<String, DocumentBuffer> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path().is_file()
                && is_supported_file(entry.path().file_name().unwrap().to_str(), true)
        })
        .filter_map(|entry| {
            let path = entry.path();
            hash_file(path).ok().map(|hash| {
                let uri = get_uri(dir, path);
                (
                    uri.clone(),
                    DocumentBuffer {
                        content: std::fs::read_to_string(path).unwrap(),
                        uri,
                        hash,
                    },
                )
            })
        })
        .collect()
}

fn setup_file_watcher(tx: mpsc::Sender<Change>) -> Result<RecommendedWatcher, String> {
    RecommendedWatcher::new(
        move |result| {
            if let Ok(event) = result {
                let _ = tx.blocking_send(Change::Internal(event));
            }
        },
        Config::default(),
    )
    .map_err(|e| format!("Failed to create watcher: {}", e))
}

async fn handle_internal_change(
    event: Event,
    dir: &Path,
    watched_files: &mut HashMap<String, DocumentBuffer>,
    client: &Client,
    flow_ref: &FlowRef,
) -> Result<(), String> {
    let Some(event_path) = event.paths.first() else {
        return Ok(());
    };

    if !is_supported_file(
        event_path.file_name().unwrap().to_str(),
        event_path.is_file(),
    ) {
        return Ok(());
    }

    let mut edits = Vec::new();

    // Handle deletions and renames
    if matches!(
        event.kind,
        notify::EventKind::Modify(ModifyKind::Name(_)) | notify::EventKind::Remove(_)
    ) {
        process_deleted_files(watched_files, &mut edits);
    }

    // Handle modifications
    process_modified_files(&event, dir, watched_files, &mut edits);

    if !edits.is_empty() {
        println!("🚀 Pushing changes...");
        client.save_edits(flow_ref, edits).await?;
    }

    Ok(())
}

fn process_deleted_files(
    watched_files: &mut HashMap<String, DocumentBuffer>,
    edits: &mut Vec<Edit>,
) {
    let invalid_paths: Vec<_> = watched_files
        .keys()
        .filter(|path| std::fs::read_to_string(path).is_err())
        .cloned()
        .collect();

    for path in invalid_paths {
        if let Some(buffer) = watched_files.get(&path) {
            edits.push(create_edit(&buffer.uri, &buffer.content, "delete"));
        }
        watched_files.remove(&path);
    }
}

fn process_modified_files(
    event: &Event,
    dir: &Path,
    watched_files: &mut HashMap<String, DocumentBuffer>,
    edits: &mut Vec<Edit>,
) {
    for path in &event.paths {
        if let Ok(hash) = hash_file(path) {
            let uri = get_uri(dir, path);
            if let Some(buffer) = watched_files.get(&uri) {
                if buffer.hash != hash {
                    let new_content = std::fs::read_to_string(path).unwrap();
                    edits.extend([
                        create_edit(&uri, &buffer.content, "delete"),
                        create_edit(&uri, &new_content, "insert"),
                    ]);
                    watched_files.insert(
                        uri.clone(),
                        DocumentBuffer {
                            content: new_content,
                            uri,
                            hash,
                        },
                    );
                }
            }
        }
    }
}

fn handle_remote_change(
    change: DocumentsChange,
    dir: &Path,
    watched_files: &mut HashMap<String, DocumentBuffer>,
) {
    println!("🔄 Syncing changes...");
    let document_uris: HashSet<String> = change.documents.iter().map(|d| d.uri.clone()).collect();
    for uri in change.uris {
        if !document_uris.contains(&uri) {
            let absolute_path = Path::new(dir).join(uri.strip_prefix("file:///").unwrap_or(&uri));
            watched_files.remove(&uri);
            std::fs::remove_file(&absolute_path).unwrap();
        }
    }
    for doc in change.documents {
        let uri = doc.uri.clone();
        let absolute_path = Path::new(dir).join(uri.strip_prefix("file:///").unwrap_or(&uri));
        std::fs::write(&absolute_path, &doc.content).unwrap();

        if let Ok(hash) = hash_file(&absolute_path) {
            watched_files.insert(
                uri.clone(),
                DocumentBuffer {
                    content: doc.content,
                    uri,
                    hash,
                },
            );
        }
    }
}

fn hash_file(path: &Path) -> Result<u64, String> {
    std::fs::read_to_string(path)
        .map(|content| {
            let mut hasher = DefaultHasher::new();
            content.hash(&mut hasher);
            hasher.finish()
        })
        .map_err(|_| "Cannot read file".to_string())
}

fn get_uri(dir: &Path, path: &Path) -> String {
    format!(
        "file:///{}",
        path.strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    )
}

async fn subscribe_to_remote_changes(
    config: &AppConfig,
    flow_ref: &FlowRef,
    tx: mpsc::Sender<Change>,
) -> Result<(), String> {
    let socket_client = setup_socket_client(config, tx).await?;
    wait_for_subscription(&socket_client, flow_ref).await?;
    Ok(())
}

async fn setup_socket_client(
    config: &AppConfig,
    tx: mpsc::Sender<Change>,
) -> Result<Arc<SocketClient>, String> {
    ClientBuilder::new(config.api_endpoint.clone())
        .namespace("/v1/flows")
        .reconnect(true)
        .reconnect_delay(1000, 5000)
        .opening_header(
            "Authorization",
            format!("Bearer {}", config.api_key.clone().unwrap_or_default()),
        )
        .on(
            "change",
            move |msg: Payload, _client: SocketClient| -> BoxFuture<'static, ()> {
                Box::pin({
                    let tx = tx.clone();
                    async move {
                        if let Payload::Text(text) = msg {
                            if let Ok(status) = serde_json::from_value::<DocumentsChange>(
                                text.first().unwrap().clone(),
                            ) {
                                let _ = tx.send(Change::Remote(status)).await;
                            }
                        }
                    }
                })
            },
        )
        .connect()
        .await
        .map(Arc::new)
        .map_err(|e| format!("Failed to connect to server: {}", e))
}

async fn wait_for_subscription(
    socket_client: &Arc<SocketClient>,
    flow_ref: &FlowRef,
) -> Result<(), String> {
    let subscription_complete = Arc::new(AtomicBool::new(false));

    for retry in 0.. {
        sleep(Duration::from_millis(100 * (retry + 1))).await;

        let subscription_complete_clone = Arc::clone(&subscription_complete);
        let ack_callback = move |_: Payload, _: SocketClient| -> BoxFuture<'static, ()> {
            let subscription_complete = Arc::clone(&subscription_complete_clone);
            Box::pin(async move {
                subscription_complete.store(true, Ordering::SeqCst);
            })
        };

        if let Err(e) = socket_client
            .emit_with_ack(
                "sync",
                json!({ "flow_ref": flow_ref }),
                Duration::from_secs(2),
                ack_callback,
            )
            .await
        {
            return Err(format!("Failed to subscribe to session: {}", e));
        }

        if subscription_complete.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}
