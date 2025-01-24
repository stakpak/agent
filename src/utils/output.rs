use std::sync::mpsc;
use std::{future::Future, pin::Pin, sync::Arc};

use super::socket::SocketClient;

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

pub fn setup_output_handler(socket_client: Arc<SocketClient>) -> impl Fn(&str) {
    let output_handler = OutputHandler::new(Box::new(move |msg: String| {
        println!("{}", msg);
        let socket_client = socket_client.clone();
        let msg = msg.clone();
        Box::pin(async move {
            socket_client.publish(&msg).await.unwrap_or(());
        })
    }));

    // Return a closure that sends messages to the `OutputHandler`
    move |msg: &str| {
        output_handler.send(msg.to_string());
    }
}
