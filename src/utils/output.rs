use std::sync::mpsc;
use std::{future::Future, pin::Pin, sync::Arc};

use rust_socketio::asynchronous::ClientBuilder;
use serde_json::json;

use crate::config::AppConfig;

pub struct OutputHandler {
    tx: mpsc::Sender<String>,
}

pub type Handler = Box<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static>;

impl OutputHandler {
    pub fn new(handler: Handler) -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        tokio::spawn(async move {
            while let Ok(msg) = rx.recv() {
                let fut = handler(msg);
                fut.await;
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
    // Attempt to connect to the socket
    let socket_client = match ClientBuilder::new(config.api_endpoint.clone())
        .namespace("/v1/sessions")
        .reconnect(true)
        .reconnect_delay(1000, 5000)
        .opening_header(
            String::from("Authorization"),
            format!("Bearer {}", config.api_key.clone().unwrap_or_default()),
        )
        .connect()
        .await
    {
        Ok(client) => Arc::new(client),
        Err(e) => {
            return Err(format!("Failed to connect to server: {}", e));
        }
    };

    // Create output handler with the connected client
    let output_handler = OutputHandler::new(Box::new(move |msg: String| {
        println!("{}", msg);
        let socket_client = socket_client.clone();
        let msg_clone = msg.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let mut retries = 0;
            while let Err(e) = socket_client
                .emit(
                    "publish",
                    json!({
                        "text": msg_clone,
                        "session_id": session_id
                    }),
                )
                .await
            {
                tokio::time::sleep(std::time::Duration::from_millis(100 * (retries + 1))).await;
                retries += 1;
                if retries >= 5 {
                    eprintln!("Failed to publish message: {}", e);
                    break;
                }
            }
        })
    }));

    // Return closure that forwards messages to the output handler
    Ok(move |msg: &str| {
        output_handler.send(msg.to_string());
    })
}
