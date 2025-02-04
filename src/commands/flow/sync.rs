use std::{path::Path, time::Duration};

use notify::{event::ModifyKind, Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::commands::flow::is_supported_file;

pub async fn sync(
    flow_ref: String,
    dir: Option<String>,
    _ignore_delete: bool,
    auto_approve: bool,
) -> Result<(), String> {
    println!("Syncing configurations");
    println!("Flow ref: {}", flow_ref);

    // Use the specified directory or default to current directory
    let watch_path = dir
        .map(|d| Path::new(&d).to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let (tx, mut rx) = mpsc::channel(32);

    let mut watcher = RecommendedWatcher::new(
        move |result| {
            if let Ok(event) = result {
                let _ = tx.blocking_send(event);
            }
        },
        Config::default(),
    )
    .map_err(|e| format!("Failed to create watcher: {}", e))?;
    watcher
        .watch(&watch_path, RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;

    loop {
        tokio::select! {
            // Handle file system events
            Some(event) = rx.recv() => {
                let event_path = event.paths.first();
                if event_path.is_none() {
                    continue;
                }
                if !is_supported_file(
                    event_path.unwrap().file_name().unwrap().to_str(),
                    event_path.unwrap().is_file()
                ) {
                    continue;
                }
                println!("Event {:#?}", event);
                match event.kind {
                    notify::EventKind::Create(_) => {
                        println!("File/directory created: {:?}", event.paths);
                    }
                    notify::EventKind::Modify(data) => {
                        match data {
                            ModifyKind::Data(d) => {
                                println!("Data {:#?}", d);
                            }
                            ModifyKind::Name(name) => {
                                println!("Name: {:?}", name);
                            }
                            _ => {
                                println!("Modify {:#?}", data);
                            }
                        }
                        println!("File/directory modified: {:?}", event.paths);
                    }
                    notify::EventKind::Remove(_) => {
                        println!("File/directory deleted: {:?}", event.paths);
                    }
                    _ => {}
                }

                // Optional: Add auto-approve logic
                if auto_approve {
                    // Implement auto-approve mechanism
                }
            }
            // Periodic task or heartbeat
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                println!("Periodic sync check...");
                // Add periodic sync logic if needed
            }
        }
    }
}
