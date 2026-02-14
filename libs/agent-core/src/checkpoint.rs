use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

pub const CHECKPOINT_VERSION_V1: u16 = 1;
pub const CHECKPOINT_FORMAT_V1: &str = "stakai_message_v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointEnvelopeV1 {
    pub version: u16,
    pub format: String,
    pub run_id: Option<Uuid>,
    pub messages: Vec<stakai::Message>,
    pub metadata: serde_json::Value,
}

impl CheckpointEnvelopeV1 {
    pub fn new(
        run_id: Option<Uuid>,
        messages: Vec<stakai::Message>,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            version: CHECKPOINT_VERSION_V1,
            format: CHECKPOINT_FORMAT_V1.to_string(),
            run_id,
            messages,
            metadata,
        }
    }
}

#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("invalid checkpoint payload: {0}")]
    InvalidPayload(#[from] serde_json::Error),

    #[error("checkpoint payload is missing version")]
    MissingVersion,

    #[error("unsupported checkpoint version: {0}")]
    UnsupportedVersion(u16),

    #[error("unsupported checkpoint format: {0}")]
    UnsupportedFormat(String),
}

pub fn serialize_checkpoint(envelope: &CheckpointEnvelopeV1) -> Result<Vec<u8>, CheckpointError> {
    serde_json::to_vec(envelope).map_err(CheckpointError::InvalidPayload)
}

pub fn deserialize_checkpoint(payload: &[u8]) -> Result<CheckpointEnvelopeV1, CheckpointError> {
    let value: serde_json::Value = serde_json::from_slice(payload)?;

    let Some(version) = value.get("version").and_then(serde_json::Value::as_u64) else {
        if let Some(migrated) = migrate_legacy_checkpoint(&value) {
            return Ok(migrated);
        }
        return Err(CheckpointError::MissingVersion);
    };

    let version = version as u16;

    if version != CHECKPOINT_VERSION_V1 {
        return Err(CheckpointError::UnsupportedVersion(version));
    }

    let envelope: CheckpointEnvelopeV1 = serde_json::from_value(value)?;

    if envelope.format != CHECKPOINT_FORMAT_V1 {
        return Err(CheckpointError::UnsupportedFormat(envelope.format));
    }

    Ok(envelope)
}

fn migrate_legacy_checkpoint(value: &serde_json::Value) -> Option<CheckpointEnvelopeV1> {
    if value.is_array() {
        let messages: Vec<stakai::Message> = serde_json::from_value(value.clone()).ok()?;
        return Some(CheckpointEnvelopeV1::new(
            None,
            messages,
            json!({"migrated_from": "legacy_messages_array"}),
        ));
    }

    let object = value.as_object()?;
    let messages_value = object.get("messages")?;
    let messages: Vec<stakai::Message> = serde_json::from_value(messages_value.clone()).ok()?;

    let run_id = object
        .get("run_id")
        .and_then(|value| serde_json::from_value::<Uuid>(value.clone()).ok());

    let metadata = object.get("metadata").cloned().unwrap_or_else(|| json!({}));

    Some(CheckpointEnvelopeV1::new(run_id, messages, metadata))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use stakai::{Message, Role};

    #[test]
    fn roundtrip_v1_envelope() {
        let run_id = Some(Uuid::new_v4());
        let envelope = CheckpointEnvelopeV1::new(
            run_id,
            vec![Message::new(Role::User, "hello")],
            json!({"cwd":"/workspace"}),
        );

        let payload = match serialize_checkpoint(&envelope) {
            Ok(payload) => payload,
            Err(error) => panic!("serialization should succeed, got: {error}"),
        };

        let parsed = match deserialize_checkpoint(&payload) {
            Ok(parsed) => parsed,
            Err(error) => panic!("deserialization should succeed, got: {error}"),
        };

        assert_eq!(parsed.version, envelope.version);
        assert_eq!(parsed.format, envelope.format);
        assert_eq!(parsed.run_id, envelope.run_id);
        assert_eq!(parsed.metadata, envelope.metadata);

        let first_message_text = parsed.messages.first().and_then(stakai::Message::text);
        assert_eq!(first_message_text, Some("hello".to_string()));
    }

    #[test]
    fn migrates_legacy_messages_array() {
        let payload = json!([
            {
                "role": "user",
                "content": "legacy"
            }
        ]);

        let result = deserialize_checkpoint(payload.to_string().as_bytes());
        let envelope = match result {
            Ok(envelope) => envelope,
            Err(error) => panic!("legacy checkpoint should migrate: {error}"),
        };

        assert_eq!(envelope.version, CHECKPOINT_VERSION_V1);
        assert_eq!(envelope.format, CHECKPOINT_FORMAT_V1);
        assert_eq!(envelope.run_id, None);
        assert_eq!(
            envelope.messages.first().and_then(stakai::Message::text),
            Some("legacy".to_string())
        );
    }

    #[test]
    fn migrates_legacy_messages_object_with_run_id() {
        let run_id = Uuid::new_v4();
        let payload = json!({
            "run_id": run_id,
            "messages": [
                {
                    "role": "assistant",
                    "content": "legacy object"
                }
            ],
            "metadata": {"legacy": true}
        });

        let result = deserialize_checkpoint(payload.to_string().as_bytes());
        let envelope = match result {
            Ok(envelope) => envelope,
            Err(error) => panic!("legacy object checkpoint should migrate: {error}"),
        };

        assert_eq!(envelope.run_id, Some(run_id));
        assert_eq!(
            envelope.messages.first().and_then(stakai::Message::text),
            Some("legacy object".to_string())
        );
        assert_eq!(envelope.metadata, json!({"legacy": true}));
    }

    #[test]
    fn reject_unsupported_version() {
        let payload = json!({
            "version": 2,
            "format": CHECKPOINT_FORMAT_V1,
            "run_id": null,
            "messages": [],
            "metadata": {}
        });

        let result = deserialize_checkpoint(payload.to_string().as_bytes());
        assert_eq!(
            result.err().map(|e| e.to_string()),
            Some("unsupported checkpoint version: 2".to_string())
        );
    }

    #[test]
    fn reject_wrong_format() {
        let payload = json!({
            "version": 1,
            "format": "legacy",
            "run_id": null,
            "messages": [],
            "metadata": {}
        });

        let result = deserialize_checkpoint(payload.to_string().as_bytes());
        assert_eq!(
            result.err().map(|e| e.to_string()),
            Some("unsupported checkpoint format: legacy".to_string())
        );
    }

    #[test]
    fn reject_payload_without_version() {
        let payload = json!({
            "format": CHECKPOINT_FORMAT_V1,
            "run_id": null,
            "metadata": {}
        });

        let result = deserialize_checkpoint(payload.to_string().as_bytes());
        assert_eq!(
            result.err().map(|e| e.to_string()),
            Some("checkpoint payload is missing version".to_string())
        );
    }
}
