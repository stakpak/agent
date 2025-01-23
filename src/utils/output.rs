use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::{mpsc, Mutex};

use super::socket::SocketClient;

pub struct OutputHandler {
    tx: mpsc::Sender<String>,
}

pub type Handler = Box<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static>;

impl OutputHandler {
    pub fn new(buffer_size: usize, handler: Handler) -> Self {
        let (tx, mut rx) = mpsc::channel(buffer_size);

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let fut = handler(msg);
                fut.await;
            }
        });

        Self { tx }
    }

    pub fn send(&self, content: String) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tx.send(content).await {
                eprintln!("Failed to send through channel: {}", e);
            }
        });
    }
}

impl Clone for OutputHandler {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

pub fn setup_output_handler(socket_client: Arc<Mutex<SocketClient>>) -> impl Fn(&str) {
    let output_handler = OutputHandler::new(
        10,
        Box::new(move |msg: String| {
            let socket_client = socket_client.clone();
            let msg = msg.clone();
            Box::pin(async move {
                println!("{}", msg);
                let publish_result = {
                    let mut client_guard = socket_client.lock().await;
                    client_guard.publish(&msg).await
                };
                publish_result.unwrap_or(());
            })
        }),
    );

    // Return a closure that sends messages to the `OutputHandler`
    move |msg: &str| {
        output_handler.send(msg.to_string());
    }
}
