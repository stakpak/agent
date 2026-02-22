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

    pub fn with_thread_id(&self, thread_id: Option<String>) -> Self {
        match self {
            Self::Telegram { chat_id, .. } => Self::Telegram {
                chat_id: chat_id.clone(),
                thread_id,
            },
            Self::Discord {
                channel_id,
                message_id,
                ..
            } => Self::Discord {
                channel_id: channel_id.clone(),
                thread_id,
                message_id: message_id.clone(),
            },
            Self::Slack { channel, .. } => Self::Slack {
                channel: channel.clone(),
                thread_ts: thread_id,
            },
        }
    }

    pub fn thread_id(&self) -> Option<String> {
        match self {
            Self::Telegram { thread_id, .. }
            | Self::Discord { thread_id, .. }
            | Self::Slack {
                thread_ts: thread_id,
                ..
            } => thread_id.clone(),
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

pub fn render_title_template(
    template: &str,
    channel: &str,
    peer_id: &str,
    chat_type: &ChatType,
) -> String {
    let chat_type_name = match chat_type {
        ChatType::Direct => "dm".to_string(),
        ChatType::Group { .. } => "group".to_string(),
        ChatType::Thread { .. } => "thread".to_string(),
    };

    let chat_id = match chat_type {
        ChatType::Direct => peer_id.to_string(),
        ChatType::Group { id } => id.clone(),
        ChatType::Thread { group_id, .. } => group_id.clone(),
    };

    template
        .replace("{channel}", channel)
        .replace("{peer}", peer_id)
        .replace("{chat_type}", &chat_type_name)
        .replace("{chat_id}", &chat_id)
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

#[cfg(test)]
mod tests {
    use super::{ChannelTarget, render_title_template};
    use crate::types::ChatType;

    #[test]
    fn with_thread_id_sets_slack_thread() {
        let target = ChannelTarget::Slack {
            channel: "C123".to_string(),
            thread_ts: None,
        };

        let updated = target.with_thread_id(Some("1700.1".to_string()));
        assert_eq!(updated.thread_id().as_deref(), Some("1700.1"));
    }

    #[test]
    fn thread_id_none_for_group_target() {
        let target = ChannelTarget::Discord {
            channel_id: "chan-1".to_string(),
            thread_id: None,
            message_id: None,
        };

        assert!(target.thread_id().is_none());
    }

    #[test]
    fn render_title_template_formats_chat_placeholders() {
        let title = render_title_template(
            "{channel}:{peer}:{chat_type}:{chat_id}",
            "slack",
            "U123",
            &ChatType::Thread {
                group_id: "C456".to_string(),
                thread_id: "1700.1".to_string(),
            },
        );

        assert_eq!(title, "slack:U123:thread:C456");
    }
}
