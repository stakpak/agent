use std::{
    collections::HashSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::future::BoxFuture;
use rust_socketio::{
    Payload,
    asynchronous::{Client as SocketClient, ClientBuilder},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{sync::mpsc, time::sleep};

use crate::{
    commands::flow::{clone, create_edit},
    config::AppConfig,
};
use stakpak_api::{
    Client,
    models::{Document, FlowRef},
};
use stakpak_shared::file_watcher::{FileWatchEvent, create_and_start_watcher};

#[derive(Deserialize, Serialize, Debug)]
pub struct DocumentsChange {
    pub flow_ref: String,
    pub documents: Vec<Document>,
    pub touched_document_uris: HashSet<String>,
}

pub fn is_supported_file(file_path: &Path) -> bool {
    let is_file = file_path.is_file();
    let file_name = file_path.file_name().and_then(|n| n.to_str());

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

pub async fn sync(
    config: &AppConfig,
    client: &Client,
    flow_ref: &FlowRef,
    dir: Option<&str>,
) -> Result<(), String> {
    // Initial setup
    clone(client, flow_ref, dir).await?;
    #[allow(clippy::unwrap_used)]
    let dir = dir
        .map(|d| Path::new(&d).to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    // Create and start the file watcher with our filter
    let (_file_watcher, mut file_events) =
        create_and_start_watcher(dir.clone(), is_supported_file).await?;

    // Set up remote change subscription
    let (change_tx, mut change_rx) = mpsc::channel(32);
    subscribe_to_remote_changes(config, flow_ref, change_tx).await?;

    println!("🔄 Started syncing changes...");

    // Main event loop
    loop {
        tokio::select! {
            // Handle processed file events
            Some(file_event) = file_events.recv() => {
                if let Err(e) = handle_file_event(file_event, client, flow_ref).await {
                    eprintln!("Error handling file event: {}", e);
                }
            }
            // Handle remote changes
            Some(remote_change) = change_rx.recv() => {
                handle_remote_change(remote_change, &dir);
            }
            else => break,
        }
    }

    Ok(())
}

async fn handle_file_event(
    event: FileWatchEvent,
    client: &Client,
    flow_ref: &FlowRef,
) -> Result<(), String> {
    let mut edits = Vec::new();

    match event {
        FileWatchEvent::Modified { file, old_content } => {
            println!("📝 File modified: {}", file.uri);
            // Create delete and insert edits for the modification
            edits.push(create_edit(&file.uri, &old_content, "delete"));
            edits.push(create_edit(&file.uri, &file.content, "insert"));
        }
        FileWatchEvent::Created { file } => {
            println!("📄 File created: {}", file.uri);
            edits.push(create_edit(&file.uri, &file.content, "insert"));
        }
        FileWatchEvent::Deleted { file } => {
            println!("🗑️ File deleted: {}", file.uri);
            edits.push(create_edit(&file.uri, &file.content, "delete"));
        }
        FileWatchEvent::Raw { .. } => {
            // Handle raw events if needed
            return Ok(());
        }
    }

    if !edits.is_empty() {
        println!("🚀 Pushing changes...");
        client.save_edits(flow_ref, edits).await?;
    }

    Ok(())
}

fn handle_remote_change(change: DocumentsChange, dir: &Path) {
    println!("🔄 Syncing remote changes...");
    let document_uris: HashSet<String> = change.documents.iter().map(|d| d.uri.clone()).collect();

    // Handle deleted files
    for uri in change.touched_document_uris {
        if !document_uris.contains(&uri) {
            let absolute_path = Path::new(dir).join(uri.strip_prefix("file:///").unwrap_or(&uri));
            std::fs::remove_file(&absolute_path).ok();
        }
    }

    // Handle created/modified files
    for doc in change.documents {
        let uri = doc.uri.clone();
        let absolute_path = Path::new(dir).join(uri.strip_prefix("file:///").unwrap_or(&uri));

        // Create parent directories if they don't exist
        if let Some(parent) = absolute_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        #[allow(clippy::unwrap_used)]
        std::fs::write(&absolute_path, &doc.content).unwrap();
    }
}

async fn subscribe_to_remote_changes(
    config: &AppConfig,
    flow_ref: &FlowRef,
    tx: mpsc::Sender<DocumentsChange>,
) -> Result<(), String> {
    let socket_client = setup_socket_client(config, tx).await?;
    wait_for_subscription(&socket_client, flow_ref).await?;
    Ok(())
}

async fn setup_socket_client(
    config: &AppConfig,
    tx: mpsc::Sender<DocumentsChange>,
) -> Result<Arc<SocketClient>, String> {
    ClientBuilder::new(config.api_endpoint.clone())
        .namespace("/v1/flows")
        .reconnect(true)
        .reconnect_delay(1000, 5000)
        .reconnect_on_disconnect(true)
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
                                #[allow(clippy::unwrap_used)]
                                text.first().unwrap().clone(),
                            ) {
                                let _ = tx.send(status).await;
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
        sleep(Duration::from_millis(200 * (retry + 1))).await;

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
            if retry >= 9 {
                return Err(format!("Failed to subscribe to session: {}", e));
            }
        }

        if subscription_complete.load(Ordering::SeqCst) {
            break;
        }

        if retry >= 5 {
            return Err("Failed to subscribe to session: Timed out".to_string());
        }
    }

    Ok(())
}
