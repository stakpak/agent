use rust_socketio::asynchronous::ClientBuilder;
use serde_json::json;
use std::future::Future;
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;

use crate::config::AppConfig;

pub struct OutputHandler {
    tx: std_mpsc::SyncSender<String>,
}

impl OutputHandler {
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(String) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (tx, rx) = std_mpsc::sync_channel::<String>(100);
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            while let Ok(msg) = rx.recv() {
                handler(msg).await;
            }
        });

        Self { tx: tx_clone }
    }

    pub fn send(&self, content: String) {
        self.tx.send(content).unwrap_or(());
    }
}

impl Clone for OutputHandler {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

pub async fn setup_output_handler(
    config: &AppConfig,
    session_id: String,
) -> Result<impl Fn(&str), String> {
    let socket_client = ClientBuilder::new(config.api_endpoint.clone())
        .namespace("/v1/agents/sessions")
        .reconnect(true)
        .reconnect_delay(1000, 5000)
        .reconnect_on_disconnect(true)
        .opening_header(
            String::from("Authorization"),
            format!("Bearer {}", config.api_key.clone().unwrap_or_default()),
        )
        .connect()
        .await
        .map_err(|e| format!("Failed to connect to server: {}", e))?;

    let socket_client = Arc::new(socket_client);

    let output_handler = OutputHandler::new(move |msg: String| {
        let socket_client = socket_client.clone();
        let session_id = session_id.clone();

        async move {
            let payload = json!({
                "text": msg,
                "session_id": session_id
            });

            for retry in 0..10 {
                match socket_client.emit("publish", payload.clone()).await {
                    Ok(_) => break,
                    Err(e) => {
                        if retry == 9 {
                            eprintln!("Failed to publish message: {}", e);
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(200 * (retry + 1)))
                            .await;
                    }
                }
            }
        }
    });

    let output_handler = Arc::new(output_handler);
    Ok(move |msg: &str| {
        let output_handler = output_handler.clone();
        let msg = msg.to_string();
        println!("{}", msg);
        output_handler.send(msg);
    })
}
