use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct IdempotencyRequest {
    pub method: String,
    pub path: String,
    pub key: String,
    pub body: serde_json::Value,
}

impl IdempotencyRequest {
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        key: impl Into<String>,
        body: serde_json::Value,
    ) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            key: key.into(),
            body,
        }
    }

    fn storage_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.method.to_ascii_uppercase(),
            self.path,
            self.key
        )
    }

    fn body_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        match serde_json::to_vec(&self.body) {
            Ok(bytes) => bytes.hash(&mut hasher),
            Err(_) => self.body.to_string().hash(&mut hasher),
        }
        hasher.finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredResponse {
    pub status_code: u16,
    pub body: serde_json::Value,
}

impl StoredResponse {
    pub fn new(status_code: u16, body: serde_json::Value) -> Self {
        Self { status_code, body }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LookupResult {
    Proceed,
    Replay(StoredResponse),
    Conflict,
}

#[derive(Clone)]
pub struct IdempotencyStore {
    retention: Duration,
    records: Arc<RwLock<HashMap<String, Record>>>,
}

#[derive(Debug, Clone)]
struct Record {
    body_hash: u64,
    response: StoredResponse,
    inserted_at: Instant,
}

impl IdempotencyStore {
    pub fn new(retention: Duration) -> Self {
        Self {
            retention,
            records: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn lookup(&self, request: &IdempotencyRequest) -> LookupResult {
        self.prune_expired().await;

        let key = request.storage_key();
        let body_hash = request.body_hash();

        let guard = self.records.read().await;
        match guard.get(&key) {
            None => LookupResult::Proceed,
            Some(record) if record.body_hash == body_hash => {
                LookupResult::Replay(record.response.clone())
            }
            Some(_) => LookupResult::Conflict,
        }
    }

    pub async fn save(&self, request: &IdempotencyRequest, response: StoredResponse) {
        self.prune_expired().await;

        let mut guard = self.records.write().await;
        guard.insert(
            request.storage_key(),
            Record {
                body_hash: request.body_hash(),
                response,
                inserted_at: Instant::now(),
            },
        );
    }

    async fn prune_expired(&self) {
        let mut guard = self.records.write().await;
        let retention = self.retention;

        guard.retain(|_, record| record.inserted_at.elapsed() <= retention);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn returns_proceed_for_first_request_then_replay_after_save() {
        let store = IdempotencyStore::new(Duration::from_secs(60));
        let request =
            IdempotencyRequest::new("POST", "/v1/sessions", "abc", json!({"title":"test"}));

        let first = store.lookup(&request).await;
        assert_eq!(first, LookupResult::Proceed);

        let response = StoredResponse::new(201, json!({"session_id":"s_1"}));
        store.save(&request, response.clone()).await;

        let second = store.lookup(&request).await;
        assert_eq!(second, LookupResult::Replay(response));
    }

    #[tokio::test]
    async fn returns_conflict_for_same_key_with_different_body() {
        let store = IdempotencyStore::new(Duration::from_secs(60));
        let first = IdempotencyRequest::new("POST", "/v1/sessions", "abc", json!({"a":1}));
        let second = IdempotencyRequest::new("POST", "/v1/sessions", "abc", json!({"a":2}));

        store
            .save(&first, StoredResponse::new(200, json!({"ok":true})))
            .await;

        let lookup = store.lookup(&second).await;
        assert_eq!(lookup, LookupResult::Conflict);
    }

    #[tokio::test]
    async fn same_key_on_different_path_is_independent() {
        let store = IdempotencyStore::new(Duration::from_secs(60));
        let first = IdempotencyRequest::new("POST", "/v1/sessions", "abc", json!({"a":1}));
        let second = IdempotencyRequest::new(
            "POST",
            "/v1/sessions/123/cancel",
            "abc",
            json!({"run_id":"r1"}),
        );

        store
            .save(&first, StoredResponse::new(200, json!({"ok":true})))
            .await;

        let lookup = store.lookup(&second).await;
        assert_eq!(lookup, LookupResult::Proceed);
    }

    #[tokio::test]
    async fn records_expire_after_retention_window() {
        let store = IdempotencyStore::new(Duration::from_millis(10));
        let request = IdempotencyRequest::new("POST", "/v1/sessions", "abc", json!({"a":1}));

        store
            .save(&request, StoredResponse::new(200, json!({"ok":true})))
            .await;

        tokio::time::sleep(Duration::from_millis(20)).await;

        let lookup = store.lookup(&request).await;
        assert_eq!(lookup, LookupResult::Proceed);
    }
}
