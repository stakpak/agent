use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct ChannelId(pub String);

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ChannelId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for ChannelId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct PeerId(pub String);

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for PeerId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for PeerId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatType {
    Direct,
    Group { id: String },
    Thread { group_id: String, thread_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct MediaAttachment {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: ChannelId,
    pub peer_id: PeerId,
    pub chat_type: ChatType,
    pub text: String,
    pub media: Vec<MediaAttachment>,
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundReply {
    pub channel: ChannelId,
    pub peer_id: PeerId,
    pub chat_type: ChatType,
    pub text: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryContext {
    pub channel: ChannelId,
    pub peer_id: PeerId,
    pub chat_type: ChatType,
    pub channel_meta: serde_json::Value,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ChatType;

    #[test]
    fn chat_type_direct_round_trip() {
        let chat_type = ChatType::Direct;

        let serialized = match serde_json::to_value(&chat_type) {
            Ok(value) => value,
            Err(error) => panic!("serialization failed: {error}"),
        };

        assert_eq!(serialized, json!({"kind": "direct"}));

        let deserialized: ChatType = match serde_json::from_value(serialized) {
            Ok(value) => value,
            Err(error) => panic!("deserialization failed: {error}"),
        };

        assert_eq!(deserialized, chat_type);
    }

    #[test]
    fn chat_type_group_round_trip() {
        let chat_type = ChatType::Group {
            id: "group-1".to_string(),
        };

        let serialized = match serde_json::to_value(&chat_type) {
            Ok(value) => value,
            Err(error) => panic!("serialization failed: {error}"),
        };

        assert_eq!(serialized, json!({"kind": "group", "id": "group-1"}));

        let deserialized: ChatType = match serde_json::from_value(serialized) {
            Ok(value) => value,
            Err(error) => panic!("deserialization failed: {error}"),
        };

        assert_eq!(deserialized, chat_type);
    }

    #[test]
    fn chat_type_thread_round_trip() {
        let chat_type = ChatType::Thread {
            group_id: "group-1".to_string(),
            thread_id: "thread-9".to_string(),
        };

        let serialized = match serde_json::to_value(&chat_type) {
            Ok(value) => value,
            Err(error) => panic!("serialization failed: {error}"),
        };

        assert_eq!(
            serialized,
            json!({"kind": "thread", "group_id": "group-1", "thread_id": "thread-9"})
        );

        let deserialized: ChatType = match serde_json::from_value(serialized) {
            Ok(value) => value,
            Err(error) => panic!("deserialization failed: {error}"),
        };

        assert_eq!(deserialized, chat_type);
    }
}
