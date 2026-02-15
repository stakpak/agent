use std::path::{Path, PathBuf};

use stakpak_agent_core::{CheckpointEnvelopeV1, deserialize_checkpoint, serialize_checkpoint};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CheckpointStore {
    root: PathBuf,
}

impl CheckpointStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let _ = std::fs::create_dir_all(&root);
        Self { root }
    }

    pub fn default_local() -> Self {
        let root = std::env::var("HOME")
            .map(|home| {
                PathBuf::from(home)
                    .join(".stakpak")
                    .join("server")
                    .join("checkpoints")
            })
            .unwrap_or_else(|_| PathBuf::from(".stakpak").join("server").join("checkpoints"));

        Self::new(root)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn load_latest(
        &self,
        session_id: Uuid,
    ) -> Result<Option<CheckpointEnvelopeV1>, String> {
        let latest_path = self.latest_path(session_id);

        let payload = match tokio::fs::read(&latest_path).await {
            Ok(payload) => payload,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "Failed to read checkpoint envelope from {}: {}",
                    latest_path.display(),
                    error
                ));
            }
        };

        deserialize_checkpoint(&payload)
            .map(Some)
            .map_err(|error| format!("Failed to deserialize checkpoint envelope: {error}"))
    }

    pub async fn save_latest(
        &self,
        session_id: Uuid,
        envelope: &CheckpointEnvelopeV1,
    ) -> Result<(), String> {
        let latest_path = self.latest_path(session_id);
        if let Some(parent) = latest_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                format!(
                    "Failed to create checkpoint directory {}: {}",
                    parent.display(),
                    error
                )
            })?;
        }

        let payload = serialize_checkpoint(envelope)
            .map_err(|error| format!("Failed to serialize checkpoint envelope: {error}"))?;

        let temp_path = latest_path.with_extension("tmp");

        tokio::fs::write(&temp_path, payload)
            .await
            .map_err(|error| {
                format!(
                    "Failed to write temporary checkpoint envelope {}: {}",
                    temp_path.display(),
                    error
                )
            })?;

        tokio::fs::rename(&temp_path, &latest_path)
            .await
            .map_err(|error| {
                format!(
                    "Failed to finalize checkpoint envelope {}: {}",
                    latest_path.display(),
                    error
                )
            })?;

        Ok(())
    }

    fn latest_path(&self, session_id: Uuid) -> PathBuf {
        self.root
            .join(session_id.to_string())
            .join("latest.checkpoint")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use stakai::{Message, Role};

    #[tokio::test]
    async fn save_and_load_checkpoint_envelope_roundtrip() {
        let root = std::env::temp_dir().join(format!("stakpak-checkpoint-test-{}", Uuid::new_v4()));
        let store = CheckpointStore::new(root);
        let session_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        let envelope = CheckpointEnvelopeV1::new(
            Some(run_id),
            vec![Message::new(Role::User, "hello")],
            json!({"turn": 1}),
        );

        let save = store.save_latest(session_id, &envelope).await;
        assert!(save.is_ok());

        let loaded = store.load_latest(session_id).await;
        assert!(loaded.is_ok());

        let Some(loaded_envelope) = loaded.ok().flatten() else {
            panic!("expected checkpoint envelope");
        };

        assert_eq!(loaded_envelope.run_id, Some(run_id));
        assert_eq!(
            loaded_envelope
                .messages
                .first()
                .and_then(stakai::Message::text),
            Some("hello".to_string())
        );
    }

    #[tokio::test]
    async fn load_latest_migrates_legacy_messages_array_payload() {
        let root = std::env::temp_dir().join(format!("stakpak-checkpoint-test-{}", Uuid::new_v4()));
        let store = CheckpointStore::new(root);
        let session_id = Uuid::new_v4();

        let legacy_payload = json!([
            {
                "role": "user",
                "content": "legacy"
            }
        ]);

        let latest_path = store
            .root()
            .join(session_id.to_string())
            .join("latest.checkpoint");

        if let Some(parent) = latest_path.parent() {
            let create_dir = tokio::fs::create_dir_all(parent).await;
            assert!(create_dir.is_ok());
        }

        let write_result = tokio::fs::write(&latest_path, legacy_payload.to_string()).await;
        assert!(write_result.is_ok());

        let loaded = store.load_latest(session_id).await;
        assert!(loaded.is_ok());

        let Some(loaded_envelope) = loaded.ok().flatten() else {
            panic!("expected migrated checkpoint envelope");
        };

        assert_eq!(
            loaded_envelope
                .messages
                .first()
                .and_then(stakai::Message::text),
            Some("legacy".to_string())
        );
    }

    #[tokio::test]
    async fn load_latest_returns_none_for_missing_session() {
        let root = std::env::temp_dir().join(format!("stakpak-checkpoint-test-{}", Uuid::new_v4()));
        let store = CheckpointStore::new(root);

        let loaded = store.load_latest(Uuid::new_v4()).await;
        assert!(loaded.is_ok());
        assert!(loaded.ok().flatten().is_none());
    }
}
