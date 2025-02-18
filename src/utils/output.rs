use rust_socketio::asynchronous::ClientBuilder;
use serde_json::json;
use std::future::Future;
use std::sync::mpsc;
use std::sync::Arc;

use crate::config::AppConfig;

pub struct OutputHandler {
    tx: mpsc::Sender<String>,
}

impl OutputHandler {
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(String) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<String>();

        tokio::spawn(async move {
            while let Ok(msg) = rx.recv() {
                handler(msg).await;
            }
        });

        Self { tx }
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
        .opening_header(
            String::from("Authorization"),
            format!("Bearer {}", config.api_key.clone().unwrap_or_default()),
        )
        .connect()
        .await
        .map_err(|e| format!("Failed to connect to server: {}", e))?;

    let socket_client = Arc::new(socket_client);

    let output_handler = OutputHandler::new(move |msg: String| {
        println!("{}", msg);
        let socket_client = socket_client.clone();
        let session_id = session_id.clone();

        async move {
            tokio::spawn(async move {
                let payload = json!({
                    "text": msg,
                    "session_id": session_id
                });

                for retry in 0..5 {
                    match socket_client.emit("publish", payload.clone()).await {
                        Ok(_) => break,
                        Err(e) => {
                            if retry == 4 {
                                eprintln!("Failed to publish message: {}", e);
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100 * (retry + 1)))
                                .await;
                        }
                    }
                }
            });
        }
    });

    Ok(move |msg: &str| {
        output_handler.send(msg.to_string());
    })
}
