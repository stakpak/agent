use anyhow::{Result, anyhow};

use crate::types::{ChannelId, ChatType, InboundMessage, PeerId};

#[derive(Debug, Clone)]
pub enum ChannelTarget {
    Telegram {
        chat_id: String,
        thread_id: Option<String>,
    },
    Discord {
        channel_id: String,
        thread_id: Option<String>,
        message_id: Option<String>,
    },
    Slack {
        channel: String,
        thread_ts: Option<String>,
    },
}

impl ChannelTarget {
    pub fn parse(channel: &str, target: &serde_json::Value) -> Result<Self> {
        let obj = target
            .as_object()
            .ok_or_else(|| anyhow!("target must be an object"))?;

        match channel {
            "telegram" => {
                let chat_id = obj
                    .get("chat_id")
                    .and_then(value_as_string)
                    .ok_or_else(|| anyhow!("missing required field: target.chat_id"))?;
                let thread_id = obj.get("thread_id").and_then(value_as_string);
                Ok(Self::Telegram { chat_id, thread_id })
            }
            "discord" => {
                let channel_id = obj
                    .get("channel_id")
                    .and_then(value_as_string)
                    .ok_or_else(|| anyhow!("missing required field: target.channel_id"))?;
                let thread_id = obj.get("thread_id").and_then(value_as_string);
                let message_id = obj.get("message_id").and_then(value_as_string);
                Ok(Self::Discord {
                    channel_id,
                    thread_id,
                    message_id,
                })
            }
            "slack" => {
                let channel = obj
                    .get("channel")
                    .and_then(value_as_string)
                    .ok_or_else(|| anyhow!("missing required field: target.channel"))?;
                let thread_ts = obj.get("thread_ts").and_then(value_as_string);
                Ok(Self::Slack { channel, thread_ts })
            }
            other => Err(anyhow!("unsupported channel target: {other}")),
        }
    }

    pub fn target_key(&self) -> String {
        match self {
            Self::Telegram { chat_id, thread_id } => match thread_id {
                Some(thread_id) => {
                    format!("telegram:chat:{chat_id}:thread:{thread_id}")
                }
                None => format!("telegram:chat:{chat_id}"),
            },
            Self::Discord {
                channel_id,
                thread_id,
                ..
            } => match thread_id {
                Some(thread_id) => {
                    format!("discord:channel:{channel_id}:thread:{thread_id}")
                }
                None => format!("discord:channel:{channel_id}"),
            },
            Self::Slack { channel, thread_ts } => match thread_ts {
                Some(thread_ts) => {
                    format!("slack:channel:{channel}:thread:{thread_ts}")
                }
                None => format!("slack:channel:{channel}"),
            },
        }
    }

    pub fn peer_id(&self) -> PeerId {
        match self {
            Self::Telegram { chat_id, .. } => chat_id.clone().into(),
            Self::Discord { channel_id, .. } => channel_id.clone().into(),
            Self::Slack { channel, .. } => channel.clone().into(),
        }
    }

    pub fn chat_type(&self) -> ChatType {
        match self {
            Self::Telegram { chat_id, thread_id } => match thread_id {
                Some(thread_id) => ChatType::Thread {
                    group_id: chat_id.clone(),
                    thread_id: thread_id.clone(),
                },
                None => ChatType::Group {
                    id: chat_id.clone(),
                },
            },
            Self::Discord {
                channel_id,
                thread_id,
                ..
            } => match thread_id {
                Some(thread_id) => ChatType::Thread {
                    group_id: channel_id.clone(),
                    thread_id: thread_id.clone(),
                },
                None => ChatType::Group {
                    id: channel_id.clone(),
                },
            },
            Self::Slack { channel, thread_ts } => match thread_ts {
                Some(thread_ts) => ChatType::Thread {
                    group_id: channel.clone(),
                    thread_id: thread_ts.clone(),
                },
                None => ChatType::Group {
                    id: channel.clone(),
                },
            },
        }
    }

    pub fn metadata(&self) -> serde_json::Value {
        match self {
            Self::Telegram { chat_id, thread_id } => serde_json::json!({
                "chat_id": chat_id,
                "thread_id": thread_id,
            }),
            Self::Discord {
                channel_id,
                thread_id,
                message_id,
            } => serde_json::json!({
                "channel_id": channel_id,
                "thread_id": thread_id,
                "message_id": message_id,
            }),
            Self::Slack { channel, thread_ts } => serde_json::json!({
                "channel": channel,
                "thread_ts": thread_ts,
            }),
        }
    }
}

pub fn target_key_from_inbound(message: &InboundMessage) -> String {
    match message.channel.0.as_str() {
        "telegram" => {
            let chat_id = message
                .metadata
                .get("chat_id")
                .and_then(value_as_string)
                .or_else(|| fallback_group_id(&message.chat_type))
                .unwrap_or_else(|| message.peer_id.0.clone());
            let thread_id = message.metadata.get("thread_id").and_then(value_as_string);
            match thread_id {
                Some(thread_id) => {
                    format!("telegram:chat:{chat_id}:thread:{thread_id}")
                }
                None => format!("telegram:chat:{chat_id}"),
            }
        }
        "discord" => {
            let channel_id = message
                .metadata
                .get("channel_id")
                .and_then(value_as_string)
                .or_else(|| fallback_group_id(&message.chat_type))
                .unwrap_or_else(|| message.peer_id.0.clone());
            let thread_id = message.metadata.get("thread_id").and_then(value_as_string);
            match thread_id {
                Some(thread_id) => {
                    format!("discord:channel:{channel_id}:thread:{thread_id}")
                }
                None => format!("discord:channel:{channel_id}"),
            }
        }
        "slack" => {
            let channel = message
                .metadata
                .get("channel")
                .and_then(value_as_string)
                .or_else(|| fallback_group_id(&message.chat_type))
                .unwrap_or_else(|| message.peer_id.0.clone());
            let thread_ts = message.metadata.get("thread_ts").and_then(value_as_string);
            match thread_ts {
                Some(thread_ts) => {
                    format!("slack:channel:{channel}:thread:{thread_ts}")
                }
                None => format!("slack:channel:{channel}"),
            }
        }
        _ => {
            let chat =
                fallback_group_id(&message.chat_type).unwrap_or_else(|| message.peer_id.0.clone());
            format!("{}:chat:{chat}", message.channel.0)
        }
    }
}

pub fn target_key_from_channel_chat(
    channel: &ChannelId,
    chat_type: &ChatType,
    peer: &PeerId,
) -> String {
    match channel.0.as_str() {
        "telegram" => match chat_type {
            ChatType::Thread {
                group_id,
                thread_id,
            } => format!("telegram:chat:{group_id}:thread:{thread_id}"),
            ChatType::Group { id } => format!("telegram:chat:{id}"),
            ChatType::Direct => format!("telegram:chat:{}", peer.0),
        },
        "discord" => match chat_type {
            ChatType::Thread {
                group_id,
                thread_id,
            } => format!("discord:channel:{group_id}:thread:{thread_id}"),
            ChatType::Group { id } => format!("discord:channel:{id}"),
            ChatType::Direct => format!("discord:channel:{}", peer.0),
        },
        "slack" => match chat_type {
            ChatType::Thread {
                group_id,
                thread_id,
            } => format!("slack:channel:{group_id}:thread:{thread_id}"),
            ChatType::Group { id } => format!("slack:channel:{id}"),
            ChatType::Direct => format!("slack:channel:{}", peer.0),
        },
        _ => match chat_type {
            ChatType::Thread {
                group_id,
                thread_id,
            } => format!("{}:thread:{group_id}:{thread_id}", channel.0),
            ChatType::Group { id } => format!("{}:chat:{id}", channel.0),
            ChatType::Direct => format!("{}:chat:{}", channel.0, peer.0),
        },
    }
}

fn fallback_group_id(chat_type: &ChatType) -> Option<String> {
    match chat_type {
        ChatType::Group { id } => Some(id.clone()),
        ChatType::Thread { group_id, .. } => Some(group_id.clone()),
        ChatType::Direct => None,
    }
}

fn value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}
