use std::sync::Mutex;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::{
    channels::{Channel, ChannelTestResult},
    chunking::chunk_text,
    types::{ChannelId, ChatType, InboundMessage, OutboundReply, PeerId},
};

const TELEGRAM_TEXT_LIMIT: usize = 4096;

pub struct TelegramChannel {
    id: ChannelId,
    token: String,
    client: reqwest::Client,
    bot_user_id: Mutex<Option<i64>>,
}

impl TelegramChannel {
    pub fn new(token: String) -> Self {
        Self {
            id: "telegram".into(),
            token,
            client: reqwest::Client::new(),
            bot_user_id: Mutex::new(None),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.token, method)
    }

    async fn get_me(&self) -> Result<TgUser> {
        let response = self
            .client
            .get(self.api_url("getMe"))
            .send()
            .await
            .context("telegram getMe request failed")?;

        let payload: TgResponse<TgUser> = response
            .json()
            .await
            .context("telegram getMe decode failed")?;

        if payload.ok {
            payload
                .result
                .ok_or_else(|| anyhow!("telegram getMe missing result"))
        } else {
            Err(anyhow!(
                "telegram getMe error {}: {}",
                payload.error_code.unwrap_or_default(),
                payload
                    .description
                    .unwrap_or_else(|| "unknown error".to_string())
            ))
        }
    }

    async fn get_updates(&self, offset: Option<i64>) -> Result<Vec<TgUpdate>> {
        let payload = GetUpdatesParams {
            offset,
            timeout: 30,
            allowed_updates: vec!["message".to_string()],
        };

        let response = self
            .client
            .post(self.api_url("getUpdates"))
            .json(&payload)
            .send()
            .await
            .context("telegram getUpdates request failed")?;

        if response.status() == reqwest::StatusCode::CONFLICT {
            return Err(anyhow!(
                "telegram getUpdates conflict: another gateway instance may already be polling"
            ));
        }

        let payload: TgResponse<Vec<TgUpdate>> = response
            .json()
            .await
            .context("telegram getUpdates decode failed")?;

        if payload.ok {
            Ok(payload.result.unwrap_or_default())
        } else {
            Err(anyhow!(
                "telegram getUpdates error {}: {}",
                payload.error_code.unwrap_or_default(),
                payload
                    .description
                    .unwrap_or_else(|| "unknown error".to_string())
            ))
        }
    }

    async fn send_chunk(&self, chat_id: i64, thread_id: Option<i64>, text: &str) -> Result<()> {
        const MAX_RETRIES: u32 = 5;

        let params = SendMessageParams {
            chat_id,
            text: text.to_string(),
            reply_to_message_id: None,
            message_thread_id: thread_id,
        };

        let mut retries: u32 = 0;
        loop {
            let response = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&params)
                .send()
                .await
                .context("telegram sendMessage request failed")?;

            let status = response.status();
            let payload: TgResponse<serde_json::Value> = response
                .json()
                .await
                .context("telegram sendMessage decode failed")?;

            if payload.ok {
                return Ok(());
            }

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                retries += 1;
                if retries > MAX_RETRIES {
                    return Err(anyhow!("telegram rate limited after {MAX_RETRIES} retries"));
                }
                let retry_after = payload
                    .parameters
                    .as_ref()
                    .and_then(|p| p.retry_after)
                    .unwrap_or(1);
                let wait = u64::try_from(retry_after.max(1)).unwrap_or(1);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                continue;
            }

            let description = payload
                .description
                .unwrap_or_else(|| "unknown error".to_string());

            return Err(anyhow!(
                "telegram sendMessage error {}: {}",
                payload.error_code.unwrap_or_default(),
                description
            ));
        }
    }

    fn map_inbound(&self, update: TgUpdate) -> Option<InboundMessage> {
        let message = update.message?;
        let text = message.text?;
        let from = message.from?;

        if from.is_bot {
            return None;
        }

        let own_bot_id = self
            .bot_user_id
            .lock()
            .ok()
            .and_then(|guard| *guard)
            .unwrap_or_default();

        if own_bot_id != 0 && from.id == own_bot_id {
            return None;
        }

        let chat_type = match message.chat.r#type.as_str() {
            "private" => ChatType::Direct,
            "group" | "supergroup" | "channel" => {
                if let Some(thread_id) = message.message_thread_id {
                    ChatType::Thread {
                        group_id: message.chat.id.to_string(),
                        thread_id: thread_id.to_string(),
                    }
                } else {
                    ChatType::Group {
                        id: message.chat.id.to_string(),
                    }
                }
            }
            _ => ChatType::Group {
                id: message.chat.id.to_string(),
            },
        };

        let timestamp = DateTime::from_timestamp(message.date, 0).unwrap_or_else(Utc::now);

        Some(InboundMessage {
            channel: self.id.clone(),
            peer_id: PeerId(from.id.to_string()),
            chat_type,
            text,
            media: Vec::new(),
            metadata: serde_json::json!({
                "chat_id": message.chat.id,
                "message_id": message.message_id,
                "thread_id": message.message_thread_id,
                "chat_title": message.chat.title,
            }),
            timestamp,
        })
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn id(&self) -> &ChannelId {
        &self.id
    }

    fn display_name(&self) -> &str {
        "Telegram"
    }

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let me = self.get_me().await?;
        if let Ok(mut guard) = self.bot_user_id.lock() {
            *guard = Some(me.id);
        }

        let mut offset: Option<i64> = None;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    break;
                }
                updates = self.get_updates(offset) => {
                    let updates = match updates {
                        Ok(updates) => updates,
                        Err(error) => {
                            error!(error = %error, "telegram poll failed");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    };

                    for update in updates {
                        offset = Some(update.update_id + 1);
                        if let Some(inbound) = self.map_inbound(update)
                            && inbound_tx.send(inbound).await.is_err()
                        {
                            return Ok(());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn send(&self, reply: OutboundReply) -> Result<()> {
        let chat_id = reply
            .metadata
            .get("chat_id")
            .and_then(parse_i64_value)
            .or_else(|| reply.peer_id.0.parse::<i64>().ok())
            .ok_or_else(|| anyhow!("telegram reply missing chat_id in metadata/peer_id"))?;

        let thread_id = reply.metadata.get("thread_id").and_then(parse_i64_value);

        let chunks = chunk_text(&reply.text, TELEGRAM_TEXT_LIMIT);
        for chunk in chunks {
            if let Err(error) = self.send_chunk(chat_id, thread_id, &chunk).await {
                warn!(error = %error, "telegram send chunk failed");
                return Err(error);
            }
        }

        Ok(())
    }

    async fn test(&self) -> Result<ChannelTestResult> {
        let me = self.get_me().await?;

        Ok(ChannelTestResult {
            channel: self.id.0.clone(),
            identity: me
                .username
                .map(|username| format!("@{username}"))
                .unwrap_or_else(|| me.first_name.unwrap_or_else(|| me.id.to_string())),
            details: format!("bot_id={}", me.id),
        })
    }
}

fn parse_i64_value(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
struct GetUpdatesParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i64>,
    timeout: i64,
    allowed_updates: Vec<String>,
}

/// Telegram Bot API `sendMessage` parameters.
///
/// Optional fields use `skip_serializing_if` because Telegram's Bot API rejects
/// `null` values — it expects optional fields to be **omitted entirely** from the
/// JSON body, not sent as `"field": null`. Without this, every request with a
/// `None` optional (e.g., `"parse_mode": null`) returns HTTP 400.
#[derive(Debug, Serialize)]
struct SendMessageParams {
    chat_id: i64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TgResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
    error_code: Option<i64>,
    #[serde(default)]
    parameters: Option<TgErrorParameters>,
}

#[derive(Debug, Deserialize)]
struct TgErrorParameters {
    #[serde(default)]
    retry_after: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TgUpdate {
    update_id: i64,
    message: Option<TgMessage>,
}

#[derive(Debug, Deserialize)]
struct TgMessage {
    message_id: i64,
    from: Option<TgUser>,
    chat: TgChat,
    text: Option<String>,
    date: i64,
    message_thread_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TgUser {
    id: i64,
    #[serde(default)]
    is_bot: bool,
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgChat {
    id: i64,
    r#type: String,
    #[serde(default)]
    title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{GetUpdatesParams, SendMessageParams, TgResponse, TgUpdate};

    #[test]
    fn telegram_update_deserialization() {
        let raw = r#"{
            "ok": true,
            "result": [{
                "update_id": 1,
                "message": {
                    "message_id": 10,
                    "from": {"id": 123, "is_bot": false, "first_name": "Alice"},
                    "chat": {"id": 456, "type": "private"},
                    "date": 1710000000,
                    "text": "hello"
                }
            }]
        }"#;

        let payload: TgResponse<Vec<TgUpdate>> = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse telegram payload: {error}"),
        };

        assert!(payload.ok);
        assert_eq!(payload.result.unwrap_or_default().len(), 1);
    }

    /// Telegram Bot API rejects `null` values — optional fields must be omitted
    /// entirely from the JSON body. This test ensures `SendMessageParams` with
    /// `None` optional fields produces no null values in the serialized output.
    #[test]
    fn send_message_params_omits_none_fields() {
        let params = SendMessageParams {
            chat_id: 123,
            text: "hello".to_string(),
            reply_to_message_id: None,
            message_thread_id: None,
        };

        let json = serde_json::to_value(&params).expect("failed to serialize SendMessageParams");
        let obj = json.as_object().expect("expected JSON object");

        // Only required fields should be present
        assert_eq!(obj.len(), 2, "expected only chat_id and text, got: {obj:?}");
        assert_eq!(obj.get("chat_id").and_then(|v| v.as_i64()), Some(123));
        assert_eq!(obj.get("text").and_then(|v| v.as_str()), Some("hello"));

        // Optional fields must not appear (not even as null)
        assert!(
            !obj.contains_key("reply_to_message_id"),
            "reply_to_message_id should be omitted when None"
        );
        assert!(
            !obj.contains_key("message_thread_id"),
            "message_thread_id should be omitted when None"
        );
    }

    /// When optional fields are `Some`, they should appear in the serialized output.
    #[test]
    fn send_message_params_includes_some_fields() {
        let params = SendMessageParams {
            chat_id: 123,
            text: "hello".to_string(),
            reply_to_message_id: Some(42),
            message_thread_id: Some(7),
        };

        let json = serde_json::to_value(&params).expect("failed to serialize SendMessageParams");
        let obj = json.as_object().expect("expected JSON object");

        assert_eq!(obj.len(), 4);
        assert_eq!(
            obj.get("reply_to_message_id").and_then(|v| v.as_i64()),
            Some(42)
        );
        assert_eq!(
            obj.get("message_thread_id").and_then(|v| v.as_i64()),
            Some(7)
        );
    }

    /// Same null-omission guarantee for `GetUpdatesParams`.
    #[test]
    fn get_updates_params_omits_none_offset() {
        let params = GetUpdatesParams {
            offset: None,
            timeout: 30,
            allowed_updates: vec!["message".to_string()],
        };

        let json = serde_json::to_value(&params).expect("failed to serialize GetUpdatesParams");
        let obj = json.as_object().expect("expected JSON object");

        assert!(
            !obj.contains_key("offset"),
            "offset should be omitted when None"
        );
        assert_eq!(obj.get("timeout").and_then(|v| v.as_i64()), Some(30));
    }
}
