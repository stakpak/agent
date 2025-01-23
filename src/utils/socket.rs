use crate::config::AppConfig;
use rust_socketio::asynchronous::{Client, ClientBuilder};
use serde_json::json;
use std::io;

pub struct SocketClient {
    client: Client,
    session_id: Option<String>,
}

impl SocketClient {
    pub async fn connect(config: &AppConfig) -> io::Result<Self> {
        let client = ClientBuilder::new(config.api_endpoint.clone())
            .namespace("/v1/agents")
            .reconnect(true)
            .reconnect_delay(1000, 5000)
            .opening_header(
                String::from("Authorization"),
                format!("Bearer {}", config.api_key.clone().unwrap_or_default()),
            )
            .connect()
            .await?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        Ok(SocketClient {
            client,
            session_id: None,
        })
    }

    pub fn set_session_id(&mut self, session_id: String) {
        self.session_id = Some(session_id);
    }

    pub async fn publish(&mut self, data: &str) -> io::Result<()> {
        self.client
            .emit(
                "publish",
                json!({
                    "text": data,
                    "session_id": self.session_id
                }),
            )
            .await?;

        Ok(())
    }
}
