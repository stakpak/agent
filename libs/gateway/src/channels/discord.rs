use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::{
    channels::{ApprovalButton, ButtonStyle, Channel, ChannelTestResult, parse_approval_callback},
    chunking::chunk_text,
    types::{ChannelId, ChatType, InboundMessage, OutboundReply, PeerId},
};

const DISCORD_TEXT_LIMIT: usize = 2000;
const DISCORD_INTENTS: u64 = (1 << 0) | (1 << 9) | (1 << 12) | (1 << 15); // 37377

const DISCORD_OP_DISPATCH: u8 = 0;
const DISCORD_OP_HEARTBEAT: u8 = 1;
const DISCORD_OP_IDENTIFY: u8 = 2;
const DISCORD_OP_RECONNECT: u8 = 7;
const DISCORD_OP_INVALID_SESSION: u8 = 9;
const DISCORD_OP_HELLO: u8 = 10;
const DISCORD_OP_HEARTBEAT_ACK: u8 = 11;

const DISCORD_INTERACTION_TYPE_COMPONENT: u8 = 3;
const DISCORD_MESSAGE_TYPE_DEFAULT: u8 = 0;
const DISCORD_CHANNEL_TYPE_PUBLIC_THREAD: u8 = 11;
const DISCORD_CHANNEL_TYPE_PRIVATE_THREAD: u8 = 12;

const DISCORD_COMPONENT_TYPE_ACTION_ROW: u8 = 1;
const DISCORD_COMPONENT_TYPE_BUTTON: u8 = 2;
const DISCORD_BUTTON_STYLE_SUCCESS: u8 = 3;
const DISCORD_BUTTON_STYLE_DANGER: u8 = 4;

const DISCORD_INTERACTION_ACK_DEFERRED_UPDATE_MESSAGE: u8 = 6;

pub struct DiscordChannel {
    id: ChannelId,
    token: String,
    http: reqwest::Client,
    bot_user_id: Mutex<Option<String>>,
    channel_cache: Mutex<HashMap<String, DiscordChannelMeta>>,
}

#[derive(Debug, Clone)]
struct DiscordChannelMeta {
    kind: u8,
    parent_id: Option<String>,
}

impl DiscordChannel {
    pub fn new(token: String) -> Self {
        Self {
            id: "discord".into(),
            token,
            http: reqwest::Client::new(),
            bot_user_id: Mutex::new(None),
            channel_cache: Mutex::new(HashMap::new()),
        }
    }

    fn auth_header(&self) -> String {
        format!("Bot {}", self.token)
    }

    async fn current_user(&self) -> Result<DiscordUser> {
        let response = self
            .http
            .get("https://discord.com/api/v10/users/@me")
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("discord users/@me request failed")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("discord users/@me failed: {body}"));
        }

        response
            .json()
            .await
            .context("discord users/@me decode failed")
    }

    async fn gateway_url(&self) -> Result<String> {
        let response = self
            .http
            .get("https://discord.com/api/v10/gateway/bot")
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("discord gateway/bot request failed")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("discord gateway/bot failed: {body}"));
        }

        let payload: GatewayBotResponse = response
            .json()
            .await
            .context("discord gateway/bot decode failed")?;

        let mut url = payload.url;
        if !url.contains('?') {
            // Discord returns "wss://gateway.discord.gg" without a trailing slash
            // and Cloudflare rejects URLs with no path component.
            if !url.ends_with('/') {
                url.push('/');
            }
            url.push_str("?v=10&encoding=json");
        } else {
            if !url.contains("v=") {
                url.push_str("&v=10");
            }
            if !url.contains("encoding=") {
                url.push_str("&encoding=json");
            }
        }

        Ok(url)
    }

    async fn fetch_channel_meta(&self, channel_id: &str) -> Result<DiscordChannelMeta> {
        if let Ok(cache) = self.channel_cache.lock()
            && let Some(meta) = cache.get(channel_id)
        {
            return Ok(meta.clone());
        }

        let response = self
            .http
            .get(format!("https://discord.com/api/v10/channels/{channel_id}"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .context("discord channel lookup request failed")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("discord channel lookup failed: {body}"));
        }

        let channel: DiscordChannelResponse = response
            .json()
            .await
            .context("discord channel lookup decode failed")?;

        let meta = DiscordChannelMeta {
            kind: channel.kind,
            parent_id: channel.parent_id,
        };

        if let Ok(mut cache) = self.channel_cache.lock() {
            cache.insert(channel.id, meta.clone());
        }

        Ok(meta)
    }

    async fn response_or_retry_after_rate_limit(
        response: reqwest::Response,
    ) -> Option<reqwest::Response> {
        if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Some(response);
        }

        let retry = response
            .json::<DiscordRateLimitResponse>()
            .await
            .ok()
            .map(|value| value.retry_after)
            .unwrap_or(1.0);
        tokio::time::sleep(std::time::Duration::from_secs_f64(retry.max(0.1))).await;
        None
    }

    async fn post_message(
        &self,
        channel_id: &str,
        content: &str,
        reply_to_message_id: Option<&str>,
        components: Option<Vec<DiscordComponent>>,
    ) -> Result<String> {
        let payload = CreateMessage {
            content: content.to_string(),
            message_reference: reply_to_message_id.map(|id| MessageReference {
                message_id: id.to_string(),
            }),
            components,
        };

        loop {
            let response = self
                .http
                .post(format!(
                    "https://discord.com/api/v10/channels/{channel_id}/messages"
                ))
                .header("Authorization", self.auth_header())
                .json(&payload)
                .send()
                .await
                .context("discord create message request failed")?;

            let Some(response) = Self::response_or_retry_after_rate_limit(response).await else {
                continue;
            };

            if !response.status().is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("discord create message failed: {body}"));
            }

            let message: DiscordMessage = response
                .json()
                .await
                .context("discord create message decode failed")?;
            return Ok(message.id);
        }
    }

    async fn patch_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
        components: Vec<DiscordComponent>,
    ) -> Result<()> {
        let payload = EditMessage {
            content: content.to_string(),
            components,
        };

        loop {
            let response = self
                .http
                .patch(format!(
                    "https://discord.com/api/v10/channels/{channel_id}/messages/{message_id}"
                ))
                .header("Authorization", self.auth_header())
                .json(&payload)
                .send()
                .await
                .context("discord edit message request failed")?;

            let Some(response) = Self::response_or_retry_after_rate_limit(response).await else {
                continue;
            };

            if !response.status().is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("discord edit message failed: {body}"));
            }

            return Ok(());
        }
    }

    async fn acknowledge_interaction(
        &self,
        interaction_id: &str,
        interaction_token: &str,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "type": DISCORD_INTERACTION_ACK_DEFERRED_UPDATE_MESSAGE
        });

        let response = self
            .http
            .post(format!(
                "https://discord.com/api/v10/interactions/{interaction_id}/{interaction_token}/callback"
            ))
            .json(&payload)
            .send()
            .await
            .context("discord interaction ack request failed")?;

        if response.status().is_success() {
            return Ok(());
        }

        let body = response.text().await.unwrap_or_default();
        Err(anyhow!("discord interaction ack failed: {body}"))
    }

    fn parse_message_reply_id(metadata: &serde_json::Value) -> Option<String> {
        metadata
            .get("message_id")
            .and_then(value_as_string)
            .or_else(|| metadata.get("id").and_then(value_as_string))
    }

    fn extract_target(reply: &OutboundReply) -> Result<String> {
        let channel_id = reply
            .metadata
            .get("channel_id")
            .and_then(value_as_string)
            .unwrap_or_else(|| reply.peer_id.0.clone());

        if channel_id.is_empty() {
            return Err(anyhow!("discord reply missing channel_id"));
        }

        Ok(channel_id)
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    fn id(&self) -> &ChannelId {
        &self.id
    }

    fn display_name(&self) -> &str {
        "Discord"
    }

    async fn start(
        &self,
        inbound_tx: mpsc::Sender<InboundMessage>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let me = self.current_user().await?;
        if let Ok(mut guard) = self.bot_user_id.lock() {
            *guard = Some(me.id.clone());
        }

        let mut reconnect_backoff_secs = 1_u64;

        loop {
            if cancel.is_cancelled() {
                return Ok(());
            }

            let gateway_url = match self.gateway_url().await {
                Ok(url) => url,
                Err(error) => {
                    error!(error = %error, "discord gateway URL lookup failed");
                    tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs))
                        .await;
                    reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
                    continue;
                }
            };

            let ws = match tokio_tungstenite::connect_async(&gateway_url).await {
                Ok((stream, _response)) => stream,
                Err(error) => {
                    error!(error = ?error, url = %gateway_url, "discord websocket connect failed");
                    tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs))
                        .await;
                    reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
                    continue;
                }
            };

            reconnect_backoff_secs = 1;

            let (mut writer, mut reader) = ws.split();

            let hello_interval = loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = writer.send(WsMessage::Close(None)).await;
                        return Ok(());
                    }
                    next = reader.next() => {
                        let Some(next) = next else {
                            break None;
                        };

                        let text = match next {
                            Ok(WsMessage::Text(text)) => text,
                            Ok(_) => continue,
                            Err(error) => {
                                warn!(error = %error, "discord read error while waiting for hello");
                                break None;
                            }
                        };

                        let payload: GatewayPayload = match serde_json::from_str(&text) {
                            Ok(payload) => payload,
                            Err(error) => {
                                warn!(error = %error, "discord hello payload decode failed");
                                continue;
                            }
                        };

                        if payload.op == DISCORD_OP_HELLO {
                            let interval = payload
                                .d
                                .as_ref()
                                .and_then(|value| value.get("heartbeat_interval"))
                                .and_then(|value| value.as_u64())
                                .unwrap_or(30_000);
                            break Some(interval);
                        }
                    }
                }
            };

            let Some(heartbeat_interval_ms) = hello_interval else {
                tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs)).await;
                reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
                continue;
            };

            let identify = serde_json::json!({
                "op": DISCORD_OP_IDENTIFY,
                "d": {
                    "token": self.token,
                    "intents": DISCORD_INTENTS,
                    "properties": {
                        "os": std::env::consts::OS,
                        "browser": "stakpak",
                        "device": "stakpak"
                    }
                }
            });

            if writer
                .send(WsMessage::Text(identify.to_string()))
                .await
                .is_err()
            {
                tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs)).await;
                reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
                continue;
            }

            let mut heartbeat =
                tokio::time::interval(std::time::Duration::from_millis(heartbeat_interval_ms));
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let _ = heartbeat.tick().await;

            let mut last_sequence: Option<u64> = None;

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = writer.send(WsMessage::Close(None)).await;
                        return Ok(());
                    }
                    _ = heartbeat.tick() => {
                        let heartbeat_payload =
                            serde_json::json!({"op": DISCORD_OP_HEARTBEAT, "d": last_sequence});
                        if writer.send(WsMessage::Text(heartbeat_payload.to_string())).await.is_err() {
                            break;
                        }
                    }
                    next = reader.next() => {
                        let Some(next) = next else {
                            break;
                        };

                        let message = match next {
                            Ok(message) => message,
                            Err(error) => {
                                warn!(error = %error, "discord websocket read failed");
                                break;
                            }
                        };

                        match message {
                            WsMessage::Text(text) => {
                                let payload: GatewayPayload = match serde_json::from_str(&text) {
                                    Ok(payload) => payload,
                                    Err(error) => {
                                        warn!(error = %error, "discord payload decode failed");
                                        continue;
                                    }
                                };

                                if let Some(seq) = payload.s {
                                    last_sequence = Some(seq);
                                }

                                match payload.op {
                                    DISCORD_OP_DISPATCH => {
                                        let event = payload.t.unwrap_or_default();
                                        if event == "READY" {
                                            if let Ok(ready) = serde_json::from_value::<ReadyEvent>(payload.d.unwrap_or_default())
                                                && let Ok(mut guard) = self.bot_user_id.lock()
                                            {
                                                *guard = Some(ready.user.id);
                                            }
                                            continue;
                                        }

                                        if event == "INTERACTION_CREATE" {
                                            let interaction: InteractionCreateEvent = match serde_json::from_value(payload.d.unwrap_or_default()) {
                                                Ok(value) => value,
                                                Err(error) => {
                                                    warn!(error = %error, "discord INTERACTION_CREATE decode failed");
                                                    continue;
                                                }
                                            };

                                            if interaction.kind != DISCORD_INTERACTION_TYPE_COMPONENT {
                                                continue;
                                            }

                                            let custom_id = interaction
                                                .data
                                                .as_ref()
                                                .and_then(|data| data.custom_id.as_deref())
                                                .unwrap_or_default();

                                            let Some((approval_id, decision)) = parse_approval_callback(custom_id) else {
                                                continue;
                                            };

                                            if let Err(error) = self
                                                .acknowledge_interaction(&interaction.id, &interaction.token)
                                                .await
                                            {
                                                warn!(error = %error, "failed to acknowledge discord interaction");
                                                continue;
                                            }

                                            let inbound = InboundMessage {
                                                channel: self.id.clone(),
                                                peer_id: PeerId(interaction.user_id()),
                                                chat_type: interaction.chat_type(),
                                                text: String::new(),
                                                media: Vec::new(),
                                                metadata: serde_json::json!({
                                                    "type": "approval_response",
                                                    "approval_id": approval_id,
                                                    "decision": decision,
                                                    "channel_id": interaction.channel_id,
                                                    "message_id": interaction.message.as_ref().map(|msg| msg.id.clone()),
                                                }),
                                                timestamp: Utc::now(),
                                            };

                                            if inbound_tx.send(inbound).await.is_err() {
                                                return Ok(());
                                            }

                                            continue;
                                        }

                                        if event != "MESSAGE_CREATE" {
                                            continue;
                                        }

                                        let message_event: MessageCreateEvent = match serde_json::from_value(payload.d.unwrap_or_default()) {
                                            Ok(value) => value,
                                            Err(error) => {
                                                warn!(error = %error, "discord MESSAGE_CREATE decode failed");
                                                continue;
                                            }
                                        };

                                        if message_event.author.bot.unwrap_or(false) {
                                            continue;
                                        }

                                        let own_bot_id = self
                                            .bot_user_id
                                            .lock()
                                            .ok()
                                            .and_then(|guard| guard.clone())
                                            .unwrap_or_default();
                                        if !own_bot_id.is_empty() && own_bot_id == message_event.author.id {
                                            continue;
                                        }

                                        if message_event.kind != DISCORD_MESSAGE_TYPE_DEFAULT
                                            || message_event.content.trim().is_empty()
                                        {
                                            continue;
                                        }

                                        let channel_meta = self.fetch_channel_meta(&message_event.channel_id).await.ok();

                                        let chat_type = match (&message_event.guild_id, channel_meta) {
                                            (None, _) => ChatType::Direct,
                                            (Some(_guild_id), Some(meta))
                                                if meta.kind == DISCORD_CHANNEL_TYPE_PUBLIC_THREAD
                                                    || meta.kind == DISCORD_CHANNEL_TYPE_PRIVATE_THREAD =>
                                            {
                                                ChatType::Thread {
                                                    group_id: meta
                                                        .parent_id
                                                        .unwrap_or_else(|| message_event.channel_id.clone()),
                                                    thread_id: message_event.channel_id.clone(),
                                                }
                                            }
                                            (Some(_), _) => ChatType::Group {
                                                id: message_event.channel_id.clone(),
                                            },
                                        };

                                        let timestamp = DateTime::parse_from_rfc3339(&message_event.timestamp)
                                            .map(|value| value.with_timezone(&Utc))
                                            .unwrap_or_else(|_| Utc::now());

                                        let inbound = InboundMessage {
                                            channel: self.id.clone(),
                                            peer_id: PeerId(message_event.author.id),
                                            chat_type,
                                            text: message_event.content,
                                            media: Vec::new(),
                                            metadata: serde_json::json!({
                                                "channel_id": message_event.channel_id,
                                                "guild_id": message_event.guild_id,
                                                "message_id": message_event.id,
                                            }),
                                            timestamp,
                                        };

                                        if inbound_tx.send(inbound).await.is_err() {
                                            return Ok(());
                                        }
                                    }
                                    DISCORD_OP_HEARTBEAT => {
                                        let heartbeat_payload = serde_json::json!({
                                            "op": DISCORD_OP_HEARTBEAT,
                                            "d": last_sequence
                                        });
                                        if writer
                                            .send(WsMessage::Text(heartbeat_payload.to_string()))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    DISCORD_OP_RECONNECT | DISCORD_OP_INVALID_SESSION => {
                                        break;
                                    }
                                    DISCORD_OP_HEARTBEAT_ACK => {}
                                    _ => {}
                                }
                            }
                            WsMessage::Ping(payload) => {
                                if writer.send(WsMessage::Pong(payload)).await.is_err() {
                                    break;
                                }
                            }
                            WsMessage::Close(_) => break,
                            _ => {}
                        }
                    }
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(reconnect_backoff_secs)).await;
            reconnect_backoff_secs = (reconnect_backoff_secs * 2).min(30);
        }
    }

    async fn send(&self, reply: OutboundReply) -> Result<()> {
        let channel_id = Self::extract_target(&reply)?;
        let reply_to = Self::parse_message_reply_id(&reply.metadata);

        let chunks = chunk_text(&reply.text, DISCORD_TEXT_LIMIT);
        for (index, chunk) in chunks.iter().enumerate() {
            let reply_ref = if index == 0 {
                reply_to.as_deref()
            } else {
                None
            };
            let _ = self
                .post_message(&channel_id, chunk, reply_ref, None)
                .await?;
        }

        Ok(())
    }

    async fn send_with_buttons(
        &self,
        reply: OutboundReply,
        buttons: Vec<ApprovalButton>,
    ) -> Result<String> {
        let channel_id = Self::extract_target(&reply)?;
        let row = DiscordActionRow {
            kind: DISCORD_COMPONENT_TYPE_ACTION_ROW,
            components: buttons
                .iter()
                .map(|button| DiscordButton {
                    kind: DISCORD_COMPONENT_TYPE_BUTTON,
                    style: match button.style {
                        ButtonStyle::Success => DISCORD_BUTTON_STYLE_SUCCESS,
                        ButtonStyle::Danger => DISCORD_BUTTON_STYLE_DANGER,
                    },
                    label: button.label.clone(),
                    custom_id: button.callback_data.clone(),
                })
                .collect(),
        };

        let reply_to = Self::parse_message_reply_id(&reply.metadata);
        let message_id = self
            .post_message(
                &channel_id,
                &reply.text,
                reply_to.as_deref(),
                Some(vec![DiscordComponent::ActionRow(row)]),
            )
            .await?;

        Ok(format!("{channel_id}:{message_id}"))
    }

    async fn edit_message(&self, message_id: &str, new_text: &str) -> Result<()> {
        let Some((channel_id, msg_id)) = parse_discord_message_id(message_id) else {
            return Ok(());
        };

        self.patch_message(channel_id, msg_id, new_text, Vec::new())
            .await
    }

    async fn test(&self) -> Result<ChannelTestResult> {
        let user = self.current_user().await?;

        Ok(ChannelTestResult {
            channel: self.id.0.clone(),
            identity: format!("{}#{}", user.username, user.discriminator),
            details: format!("id={}", user.id),
        })
    }
}

#[derive(Debug, Deserialize)]
struct GatewayPayload {
    op: u8,
    #[serde(default)]
    d: Option<serde_json::Value>,
    #[serde(default)]
    s: Option<u64>,
    #[serde(default)]
    t: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReadyEvent {
    user: DiscordUser,
}

#[derive(Debug, Deserialize)]
struct MessageCreateEvent {
    id: String,
    channel_id: String,
    #[serde(default)]
    guild_id: Option<String>,
    author: DiscordUser,
    content: String,
    timestamp: String,
    #[serde(rename = "type")]
    kind: u8,
}

#[derive(Debug, Deserialize)]
struct InteractionCreateEvent {
    id: String,
    token: String,
    #[serde(rename = "type")]
    kind: u8,
    channel_id: String,
    #[serde(default)]
    guild_id: Option<String>,
    #[serde(default)]
    member: Option<InteractionMember>,
    #[serde(default)]
    user: Option<DiscordUser>,
    #[serde(default)]
    data: Option<InteractionData>,
    #[serde(default)]
    message: Option<DiscordMessage>,
}

impl InteractionCreateEvent {
    fn user_id(&self) -> String {
        self.member
            .as_ref()
            .map(|member| member.user.id.clone())
            .or_else(|| self.user.as_ref().map(|user| user.id.clone()))
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn chat_type(&self) -> ChatType {
        if self.guild_id.is_none() {
            return ChatType::Direct;
        }

        ChatType::Group {
            id: self.channel_id.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct InteractionMember {
    user: DiscordUser,
}

#[derive(Debug, Deserialize)]
struct InteractionData {
    #[serde(default)]
    custom_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    discriminator: String,
    #[serde(default)]
    bot: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GatewayBotResponse {
    url: String,
}

#[derive(Debug, Deserialize)]
struct DiscordChannelResponse {
    id: String,
    #[serde(rename = "type")]
    kind: u8,
    #[serde(default)]
    parent_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateMessage {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_reference: Option<MessageReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    components: Option<Vec<DiscordComponent>>,
}

#[derive(Debug, Serialize)]
struct EditMessage {
    content: String,
    components: Vec<DiscordComponent>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum DiscordComponent {
    ActionRow(DiscordActionRow),
}

#[derive(Debug, Serialize)]
struct DiscordActionRow {
    #[serde(rename = "type")]
    kind: u8,
    components: Vec<DiscordButton>,
}

#[derive(Debug, Serialize)]
struct DiscordButton {
    #[serde(rename = "type")]
    kind: u8,
    style: u8,
    label: String,
    custom_id: String,
}

#[derive(Debug, Serialize)]
struct MessageReference {
    message_id: String,
}

#[derive(Debug, Deserialize)]
struct DiscordMessage {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DiscordRateLimitResponse {
    retry_after: f64,
}

fn value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn parse_discord_message_id(message_id: &str) -> Option<(&str, &str)> {
    message_id.rsplit_once(':')
}

#[cfg(test)]
mod tests {
    use super::{
        DISCORD_OP_DISPATCH, GatewayPayload, MessageCreateEvent, parse_discord_message_id,
    };

    #[test]
    fn gateway_payload_deserializes() {
        let raw = r#"{"op":0,"s":1,"t":"READY","d":{"user":{"id":"1","username":"bot","discriminator":"0001"}}}"#;
        let payload: GatewayPayload = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse payload: {error}"),
        };

        assert_eq!(payload.op, DISCORD_OP_DISPATCH);
        assert_eq!(payload.t.as_deref(), Some("READY"));
        assert_eq!(payload.s, Some(1));
    }

    #[test]
    fn message_create_deserializes() {
        let raw = r#"{
            "id":"m1",
            "channel_id":"c1",
            "guild_id":"g1",
            "author":{"id":"u1","username":"alice","discriminator":"1234","bot":false},
            "content":"hello",
            "timestamp":"2026-01-01T00:00:00.000000+00:00",
            "type":0
        }"#;

        let event: MessageCreateEvent = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(error) => panic!("failed to parse event: {error}"),
        };

        assert_eq!(event.id, "m1");
        assert_eq!(event.channel_id, "c1");
        assert_eq!(event.guild_id.as_deref(), Some("g1"));
        assert_eq!(event.content, "hello");
        assert_eq!(event.kind, 0);
    }

    #[test]
    fn parse_discord_message_id_splits_on_last_colon() {
        assert_eq!(parse_discord_message_id("123:456"), Some(("123", "456")));
        assert_eq!(
            parse_discord_message_id("team:123:456"),
            Some(("team:123", "456"))
        );
    }
}
