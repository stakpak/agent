use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{
    channels::{Channel, ChannelTestResult, DeliveryReceipt},
    slack_blocks::markdown_to_slack_messages,
    types::{ChannelId, ChatType, InboundMessage, OutboundReply, PeerId},
};

const RECEIVED_REACTION: &str = "eyes";

pub struct SlackChannel {
    id: ChannelId,
    bot_token: String,
    app_token: String,
    http: reqwest::Client,
    bot_user_id: Mutex<Option<String>>,
    dedup: Mutex<DedupBuffer>,
    active_threads: Mutex<HashSet<(String, String)>>,
}

impl SlackChannel {
    pub fn new(bot_token: String, app_token: String) -> Self {
        Self {
            id: "slack".into(),
            bot_token,
            app_token,
            http: reqwest::Client::new(),
            bot_user_id: Mutex::new(None),
            dedup: Mutex::new(DedupBuffer::new(2048)),
            active_threads: Mutex::new(HashSet::new()),
        }
    }

    async fn auth_test(&self) -> Result<AuthTestResponse> {
        let response = self
            .http
            .post("https://slack.com/api/auth.test")
            .bearer_auth(&self.bot_token)
            .send()
            .await
            .context("slack auth.test request failed")?;

        let payload: AuthTestResponse = response
            .json()
            .await
            .context("slack auth.test decode failed")?;

        if payload.ok {
            Ok(payload)
        } else {
            Err(anyhow!(
                "slack auth.test failed: {}",
                payload.error.unwrap_or_else(|| "unknown error".to_string())
            ))
        }
    }

    async fn open_socket_url(&self) -> Result<String> {
        let response = self
            .http
            .post("https://slack.com/api/apps.connections.open")
            .bearer_auth(&self.app_token)
            .send()
            .await
            .context("slack apps.connections.open request failed")?;

        let payload: AppsConnectionsOpenResponse = response
            .json()
            .await
            .context("slack apps.connections.open decode failed")?;

        if payload.ok {
            payload
                .url
                .ok_or_else(|| anyhow!("slack apps.connections.open missing websocket url"))
        } else {
            Err(anyhow!(
                "slack apps.connections.open failed: {}",
                payload.error.unwrap_or_else(|| "unknown error".to_string())
            ))
        }
    }

    async fn post_message(
        &self,
        channel: &str,
        text: &str,
        blocks: Option<Vec<serde_json::Value>>,
        attachments: Option<Vec<serde_json::Value>>,
        thread_ts: Option<&str>,
    ) -> Result<String> {
        let payload = ChatPostMessage {
            channel: channel.to_string(),
            text: text.to_string(),
            blocks,
            attachments,
            thread_ts: thread_ts.map(ToOwned::to_owned),
        };

        loop {
            let response = self
                .http
                .post("https://slack.com/api/chat.postMessage")
                .bearer_auth(&self.bot_token)
                .json(&payload)
                .send()
                .await
                .context("slack chat.postMessage request failed")?;

            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1);
                tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                continue;
            }

            let payload: ChatPostMessageResponse = response
                .json()
                .await
                .context("slack chat.postMessage decode failed")?;

            if payload.ok {
                return payload
                    .ts
                    .ok_or_else(|| anyhow!("slack chat.postMessage missing message timestamp"));
            }

            return Err(anyhow!(
                "slack chat.postMessage failed: {}",
                payload.error.unwrap_or_else(|| "unknown error".to_string())
            ));
        }
    }

    async fn add_reaction(&self, channel: &str, ts: &str, name: &str) -> Result<()> {
        let payload = ReactionsAdd {
            channel: channel.to_string(),
            name: name.to_string(),
            timestamp: ts.to_string(),
        };

        loop {
            let response = self
                .http
                .post("https://slack.com/api/reactions.add")
                .bearer_auth(&self.bot_token)
                .json(&payload)
                .send()
                .await
                .context("slack reactions.add request failed")?;

            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1);
                tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                continue;
            }

            let payload: SlackApiResponse = response
                .json()
                .await
                .context("slack reactions.add decode failed")?;

            if payload.ok {
                return Ok(());
            }

            // Ignore known non-fatal reaction errors for UX best-effort behavior.
            if let Some(error) = payload.error {
                if matches!(
                    error.as_str(),
                    "already_reacted" | "message_not_found" | "not_in_channel"
                ) {
                    return Ok(());
                }
                return Err(anyhow!("slack reactions.add failed: {error}"));
            }

            return Err(anyhow!("slack reactions.add failed: unknown error"));
        }
    }

    async fn handle_socket_payload<Writer>(
        &self,
        payload_text: String,
        writer: &mut Writer,
        inbound_tx: &mpsc::Sender<InboundMessage>,
    ) -> Result<HandleAction>
    where
        Writer:
            futures_util::Sink<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    {
        let payload: SocketPayload =
            serde_json::from_str(&payload_text).context("slack socket payload decode failed")?;

        match payload.payload_type.as_str() {
            "hello" => {
                info!("slack socket hello received");
                Ok(HandleAction::Continue)
            }
            "disconnect" => {
                let reason = payload
                    .reason
                    .unwrap_or_else(|| "unknown reason".to_string());
                warn!(reason = %reason, "slack socket requested disconnect");
                Ok(HandleAction::Reconnect)
            }
            "events_api" => {
                let Some(envelope_id) = payload.envelope_id else {
                    return Ok(HandleAction::Continue);
                };

                let ack = SocketAck {
                    envelope_id: envelope_id.clone(),
                };
                let ack_text = serde_json::to_string(&ack).context("slack ack encode failed")?;
                writer
                    .send(WsMessage::Text(ack_text))
                    .await
                    .context("slack ack send failed")?;

                if payload.retry_attempt.unwrap_or(0) > 0 {
                    return Ok(HandleAction::Continue);
                }

                let Some(event_payload_raw) = payload.payload else {
                    return Ok(HandleAction::Continue);
                };

                let event_payload: EventPayload = serde_json::from_value(event_payload_raw)
                    .context("slack event payload decode failed")?;

                if event_payload.event_type != "event_callback" {
                    return Ok(HandleAction::Continue);
                }

                let event = event_payload.event;
                if event.event_type != "message" {
                    return Ok(HandleAction::Continue);
                }

                if event.subtype.is_some() || event.bot_id.is_some() {
                    return Ok(HandleAction::Continue);
                }

                let Some(user) = event.user else {
                    return Ok(HandleAction::Continue);
                };

                let own_bot_id = self
                    .bot_user_id
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .unwrap_or_default();
                if !own_bot_id.is_empty() && user == own_bot_id {
                    return Ok(HandleAction::Continue);
                }

                let Some(channel) = event.channel else {
                    return Ok(HandleAction::Continue);
                };
                let Some(raw_text) = event.text else {
                    return Ok(HandleAction::Continue);
                };
                if raw_text.trim().is_empty() {
                    return Ok(HandleAction::Continue);
                }

                let ts = event.ts.clone().unwrap_or_else(|| format_ts(Utc::now()));
                if self.is_duplicate(&channel, &ts) {
                    return Ok(HandleAction::Continue);
                }

                let channel_type = event.channel_type.unwrap_or_else(|| "channel".to_string());
                let is_dm = channel_type == "im";
                let mentioned = is_bot_mentioned(&raw_text, &own_bot_id);

                let mut effective_thread_ts = event.thread_ts.clone();

                if !is_dm {
                    match event.thread_ts.as_deref() {
                        Some(thread_ts) => {
                            if mentioned {
                                self.activate_thread(&channel, thread_ts);
                            } else if !self.is_thread_active(&channel, thread_ts) {
                                return Ok(HandleAction::Continue);
                            }
                            effective_thread_ts = Some(thread_ts.to_string());
                        }
                        None => {
                            // Top-level channel/group message: only respond when bot is mentioned.
                            if !mentioned {
                                return Ok(HandleAction::Continue);
                            }
                            // Force thread session semantics by anchoring to the top-level message ts.
                            self.activate_thread(&channel, &ts);
                            effective_thread_ts = Some(ts.clone());
                        }
                    }
                }

                let cleaned_text = if is_dm {
                    raw_text.trim().to_string()
                } else {
                    strip_bot_mention(&raw_text, &own_bot_id).trim().to_string()
                };

                if cleaned_text.is_empty() {
                    return Ok(HandleAction::Continue);
                }

                let chat_type =
                    map_chat_type(&channel, &channel_type, effective_thread_ts.as_deref());

                if let Err(error) = self.add_reaction(&channel, &ts, RECEIVED_REACTION).await {
                    warn!(error = %error, channel = %channel, ts = %ts, "failed to add slack receipt reaction");
                }

                let inbound = InboundMessage {
                    channel: self.id.clone(),
                    peer_id: PeerId(user.clone()),
                    chat_type,
                    text: cleaned_text,
                    media: Vec::new(),
                    metadata: serde_json::json!({
                        "channel": channel,
                        "ts": ts,
                        "thread_ts": effective_thread_ts,
                        "channel_type": channel_type,
                        "mentioned": mentioned,
                        "user_id": user,
                    }),
                    timestamp: parse_slack_ts_to_datetime(event.ts.as_deref()),
                };

                if inbound_tx.send(inbound).await.is_err() {
                    return Ok(HandleAction::Stop);
                }

                Ok(HandleAction::Continue)
            }
            _ => Ok(HandleAction::Continue),
        }
    }

    fn is_duplicate(&self, channel: &str, ts: &str) -> bool {
        match self.dedup.lock() {
            Ok(mut guard) => guard.is_duplicate(channel.to_string(), ts.to_string()),
            Err(_) => false,
        }
    }

    fn activate_thread(&self, channel: &str, thread_ts: &str) {
        if let Ok(mut guard) = self.active_threads.lock() {
            guard.insert((channel.to_string(), thread_ts.to_string()));
        }
    }

    fn is_thread_active(&self, channel: &str, thread_ts: &str) -> bool {
        match self.active_threads.lock() {
            Ok(guard) => guard.contains(&(channel.to_string(), thread_ts.to_string())),
            Err(_) => false,
        }
    }
}

#[async_trait]
impl Channel for SlackChannel {
    fn id(&self) -> &ChannelId {
        &self.id
    }

    fn display_name(&self) -> &str {
        "Slack"
    }

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let auth = self.auth_test().await?;
        if let Ok(mut guard) = self.bot_user_id.lock() {
            *guard = auth.user_id.clone();
        }

        let mut reconnect_backoff_secs = 1_u64;

        loop {
            if cancel.is_cancelled() {
                break;
            }

            let socket_url = match self.open_socket_url().await {
                Ok(url) => url,
                Err(error) => {
                    error!(error = %error, "slack open socket failed");
                    tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs))
                        .await;
                    reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
                    continue;
                }
            };

            let ws = match tokio_tungstenite::connect_async(&socket_url).await {
                Ok((stream, _response)) => stream,
                Err(error) => {
                    error!(error = %error, "slack websocket connect failed");
                    tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs))
                        .await;
                    reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
                    continue;
                }
            };

            reconnect_backoff_secs = 1;
            let (mut writer, mut reader) = ws.split();

            let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(20));
            keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let _ = keepalive.tick().await;

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = writer.send(WsMessage::Close(None)).await;
                        return Ok(());
                    }
                    _ = keepalive.tick() => {
                        if writer.send(WsMessage::Ping(Vec::<u8>::new())).await.is_err() {
                            warn!("slack websocket keepalive ping failed; reconnecting");
                            break;
                        }
                    }
                    next = reader.next() => {
                        let Some(next) = next else {
                            break;
                        };

                        match next {
                            Ok(WsMessage::Text(text)) => {
                                match self.handle_socket_payload(text, &mut writer, &inbound_tx).await {
                                    Ok(HandleAction::Continue) => {}
                                    Ok(HandleAction::Reconnect) => break,
                                    Ok(HandleAction::Stop) => return Ok(()),
                                    Err(error) => {
                                        warn!(error = %error, "slack socket payload handler failed");
                                    }
                                }
                            }
                            Ok(WsMessage::Ping(payload)) => {
                                if writer.send(WsMessage::Pong(payload)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(WsMessage::Close(_)) => break,
                            Ok(_) => {}
                            Err(error) => {
                                warn!(error = %error, "slack websocket read failed");
                                break;
                            }
                        }
                    }
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs)).await;
            reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
            continue;
        }

        Ok(())
    }

    async fn send(&self, reply: OutboundReply) -> Result<()> {
        self.send_with_receipt(reply).await.map(|_| ())
    }

    async fn send_with_receipt(&self, reply: OutboundReply) -> Result<DeliveryReceipt> {
        let channel = reply
            .metadata
            .get("channel")
            .and_then(value_as_string)
            .unwrap_or_else(|| reply.peer_id.0.clone());

        if channel.is_empty() {
            return Err(anyhow!("slack reply missing target channel"));
        }

        let channel_type = reply
            .metadata
            .get("channel_type")
            .and_then(value_as_string)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| match reply.chat_type {
                ChatType::Direct => "im".to_string(),
                _ => "channel".to_string(),
            });

        let thread_ts = reply
            .metadata
            .get("thread_ts")
            .and_then(value_as_string)
            .or_else(|| {
                if channel_type == "im" {
                    None
                } else {
                    reply.metadata.get("ts").and_then(value_as_string)
                }
            })
            .filter(|value| !value.is_empty());

        let slack_messages = markdown_to_slack_messages(&reply.text);
        let mut first_message_ts: Option<String> = None;
        let multi_message = slack_messages.len() > 1;

        if slack_messages.is_empty() {
            // Empty content — send a minimal plain-text message.
            let ts = self
                .post_message(&channel, &reply.text, None, None, thread_ts.as_deref())
                .await?;
            first_message_ts = Some(ts);
        } else {
            for (i, msg) in slack_messages.into_iter().enumerate() {
                // Slack rate limit: ~1 message/second/channel.
                // Add a small delay between split messages to avoid hitting it.
                if multi_message && i > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
                }

                let blocks = if msg.blocks.is_empty() {
                    None
                } else {
                    Some(msg.blocks)
                };
                let ts = self
                    .post_message(
                        &channel,
                        &msg.fallback_text,
                        blocks,
                        msg.attachments,
                        thread_ts.as_deref(),
                    )
                    .await?;
                if first_message_ts.is_none() {
                    first_message_ts = Some(ts);
                }
            }
        }

        let effective_thread_id = thread_ts.or_else(|| first_message_ts.clone());

        if channel_type != "im"
            && let Some(thread_id) = effective_thread_id.as_deref()
        {
            self.activate_thread(&channel, thread_id);
        }

        Ok(DeliveryReceipt {
            message_id: first_message_ts,
            thread_id: effective_thread_id,
        })
    }

    async fn test(&self) -> Result<ChannelTestResult> {
        let auth = self.auth_test().await?;
        let _ = self.open_socket_url().await?;

        let identity = auth
            .user
            .or(auth.user_id)
            .unwrap_or_else(|| "unknown-bot".to_string());
        let workspace = auth.team.unwrap_or_else(|| "unknown-workspace".to_string());

        Ok(ChannelTestResult {
            channel: self.id.0.clone(),
            identity,
            details: format!("workspace={workspace}"),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum HandleAction {
    Continue,
    Reconnect,
    Stop,
}

#[derive(Debug, Deserialize)]
struct SocketPayload {
    #[serde(rename = "type")]
    payload_type: String,
    #[serde(default)]
    envelope_id: Option<String>,
    #[serde(default)]
    payload: Option<serde_json::Value>,
    #[serde(default)]
    retry_attempt: Option<u32>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventPayload {
    #[serde(rename = "type")]
    event_type: String,
    event: SlackEvent,
}

#[derive(Debug, Deserialize)]
struct SlackEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    thread_ts: Option<String>,
    #[serde(default)]
    channel_type: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    bot_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct SocketAck {
    envelope_id: String,
}

#[derive(Debug, Serialize)]
struct ChatPostMessage {
    channel: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachments: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_ts: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReactionsAdd {
    channel: String,
    name: String,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
struct SlackApiResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatPostMessageResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    ts: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppsConnectionsOpenResponse {
    ok: bool,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthTestResponse {
    ok: bool,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug)]
struct DedupBuffer {
    seen: HashSet<(String, String)>,
    order: VecDeque<(String, String)>,
    capacity: usize,
}

impl DedupBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn is_duplicate(&mut self, channel: String, ts: String) -> bool {
        let key = (channel, ts);
        if self.seen.contains(&key) {
            return true;
        }

        self.seen.insert(key.clone());
        self.order.push_back(key);

        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }

        false
    }
}

fn map_chat_type(channel: &str, channel_type: &str, thread_ts: Option<&str>) -> ChatType {
    match channel_type {
        "im" => ChatType::Direct,
        "mpim" => ChatType::Group {
            id: channel.to_string(),
        },
        "channel" | "group" => {
            if let Some(thread_ts) = thread_ts {
                ChatType::Thread {
                    group_id: channel.to_string(),
                    thread_id: thread_ts.to_string(),
                }
            } else {
                ChatType::Group {
                    id: channel.to_string(),
                }
            }
        }
        _ => {
            if let Some(thread_ts) = thread_ts {
                ChatType::Thread {
                    group_id: channel.to_string(),
                    thread_id: thread_ts.to_string(),
                }
            } else {
                ChatType::Group {
                    id: channel.to_string(),
                }
            }
        }
    }
}

fn parse_slack_ts_to_datetime(ts: Option<&str>) -> DateTime<Utc> {
    let Some(ts) = ts else {
        return Utc::now();
    };

    let seconds = ts.split('.').next().unwrap_or(ts);
    if let Ok(value) = seconds.parse::<i64>()
        && let Some(datetime) = DateTime::from_timestamp(value, 0)
    {
        return datetime;
    }

    Utc::now()
}

fn format_ts(dt: DateTime<Utc>) -> String {
    format!("{}.000000", dt.timestamp())
}

fn value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn is_bot_mentioned(text: &str, bot_user_id: &str) -> bool {
    if bot_user_id.trim().is_empty() {
        return false;
    }

    let mention = format!("<@{bot_user_id}>");
    text.contains(&mention)
}

fn strip_bot_mention(text: &str, bot_user_id: &str) -> String {
    if bot_user_id.trim().is_empty() {
        return text.to_string();
    }

    let mention = format!("<@{bot_user_id}>");
    let cleaned = text.replace(&mention, " ");
    collapse_whitespace(&cleaned)
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{DedupBuffer, is_bot_mentioned, map_chat_type, strip_bot_mention};
    use crate::types::ChatType;

    #[test]
    fn map_chat_type_im_to_direct() {
        let mapped = map_chat_type("C123", "im", None);
        assert!(matches!(mapped, ChatType::Direct));
    }

    #[test]
    fn map_chat_type_thread_when_thread_ts_present() {
        let mapped = map_chat_type("C123", "channel", Some("123.456"));
        assert!(matches!(
            mapped,
            ChatType::Thread {
                group_id,
                thread_id
            } if group_id == "C123" && thread_id == "123.456"
        ));
    }

    #[test]
    fn dedup_buffer_detects_duplicates_and_evicts() {
        let mut dedup = DedupBuffer::new(2);

        assert!(!dedup.is_duplicate("C1".to_string(), "1".to_string()));
        assert!(dedup.is_duplicate("C1".to_string(), "1".to_string()));

        assert!(!dedup.is_duplicate("C1".to_string(), "2".to_string()));
        assert!(!dedup.is_duplicate("C1".to_string(), "3".to_string()));

        assert!(!dedup.is_duplicate("C1".to_string(), "1".to_string()));
    }

    #[test]
    fn bot_mention_detection_works() {
        assert!(is_bot_mentioned("hello <@U12345>", "U12345"));
        assert!(!is_bot_mentioned("hello world", "U12345"));
        assert!(!is_bot_mentioned("hello <@U12345>", ""));
    }

    #[test]
    fn strip_bot_mention_removes_and_normalizes_whitespace() {
        let stripped = strip_bot_mention("<@U12345>   please help", "U12345");
        assert_eq!(stripped, "please help");

        let unchanged = strip_bot_mention("please help", "U12345");
        assert_eq!(unchanged, "please help");
    }
}
