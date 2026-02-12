use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::types::DeliveryContext;

#[derive(Debug, Clone)]
pub struct SessionMapping {
    pub session_id: String,
    pub title: String,
    pub delivery: DeliveryContext,
    pub created_at: i64,
}

pub struct GatewayStore {
    conn: Mutex<Connection>,
}

impl GatewayStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create store parent dir: {}", parent.display())
            })?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("failed to open sqlite db: {}", path.display()))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite db")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn get(&self, routing_key: &str) -> Result<Option<SessionMapping>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at
             FROM sessions
             WHERE routing_key = ?1",
        )?;

        stmt.query_row(params![routing_key], |row| {
            let chat_type_json: String = row.get(4)?;
            let channel_meta_json: String = row.get(5)?;
            let chat_type = parse_json_value(&chat_type_json)?;
            let channel_meta = parse_json_value(&channel_meta_json)?;

            let channel: String = row.get(2)?;
            let peer_id: String = row.get(3)?;

            Ok(SessionMapping {
                session_id: row.get(0)?,
                title: row.get(1)?,
                delivery: DeliveryContext {
                    channel: channel.into(),
                    peer_id: peer_id.into(),
                    chat_type,
                    channel_meta,
                    updated_at: row.get(7)?,
                },
                created_at: row.get(6)?,
            })
        })
        .optional()
        .context("failed to fetch session mapping")
    }

    pub fn set(&self, routing_key: &str, mapping: &SessionMapping) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO sessions
             (routing_key, session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                routing_key,
                mapping.session_id,
                mapping.title,
                mapping.delivery.channel.0,
                mapping.delivery.peer_id.0,
                serde_json::to_string(&mapping.delivery.chat_type).context("failed to serialize chat_type")?,
                serde_json::to_string(&mapping.delivery.channel_meta)
                    .context("failed to serialize channel_meta")?,
                mapping.created_at,
                mapping.delivery.updated_at,
            ],
        )
        .context("failed to upsert session mapping")?;

        Ok(())
    }

    pub fn find_by_session_id(&self, session_id: &str) -> Result<Option<(String, SessionMapping)>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT routing_key, session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at
             FROM sessions
             WHERE session_id = ?1
             ORDER BY updated_at DESC
             LIMIT 1",
        )?;

        stmt.query_row(params![session_id], |row| {
            let routing_key: String = row.get(0)?;
            let chat_type_json: String = row.get(5)?;
            let channel_meta_json: String = row.get(6)?;
            let chat_type = parse_json_value(&chat_type_json)?;
            let channel_meta = parse_json_value(&channel_meta_json)?;

            let channel: String = row.get(3)?;
            let peer_id: String = row.get(4)?;

            Ok((
                routing_key,
                SessionMapping {
                    session_id: row.get(1)?,
                    title: row.get(2)?,
                    delivery: DeliveryContext {
                        channel: channel.into(),
                        peer_id: peer_id.into(),
                        chat_type,
                        channel_meta,
                        updated_at: row.get(8)?,
                    },
                    created_at: row.get(7)?,
                },
            ))
        })
        .optional()
        .context("failed to query by session_id")
    }

    pub fn update_delivery(&self, routing_key: &str, delivery: &DeliveryContext) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "UPDATE sessions
             SET channel = ?1,
                 peer_id = ?2,
                 chat_type = ?3,
                 channel_meta = ?4,
                 updated_at = ?5
             WHERE routing_key = ?6",
            params![
                delivery.channel.0,
                delivery.peer_id.0,
                serde_json::to_string(&delivery.chat_type)
                    .context("failed to serialize chat_type")?,
                serde_json::to_string(&delivery.channel_meta)
                    .context("failed to serialize channel_meta")?,
                delivery.updated_at,
                routing_key,
            ],
        )
        .context("failed to update delivery context")?;

        Ok(())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<(String, SessionMapping)>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT routing_key, session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at
             FROM sessions
             ORDER BY updated_at DESC
             LIMIT ?1",
        )?;

        let mut rows = stmt.query(params![limit as i64])?;
        let mut out = Vec::new();

        while let Some(row) = rows.next()? {
            let routing_key: String = row.get(0)?;
            let chat_type_json: String = row.get(5)?;
            let channel_meta_json: String = row.get(6)?;
            let chat_type = parse_json_value(&chat_type_json)?;
            let channel_meta = parse_json_value(&channel_meta_json)?;

            let channel: String = row.get(3)?;
            let peer_id: String = row.get(4)?;

            out.push((
                routing_key,
                SessionMapping {
                    session_id: row.get(1)?,
                    title: row.get(2)?,
                    delivery: DeliveryContext {
                        channel: channel.into(),
                        peer_id: peer_id.into(),
                        chat_type,
                        channel_meta,
                        updated_at: row.get(8)?,
                    },
                    created_at: row.get(7)?,
                },
            ));
        }

        Ok(out)
    }

    pub fn delete(&self, routing_key: &str) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "DELETE FROM sessions WHERE routing_key = ?1",
            params![routing_key],
        )
        .context("failed to delete routing key")?;
        Ok(())
    }

    pub fn prune(&self, max_age_ms: i64) -> Result<usize> {
        let cutoff = now_millis() - max_age_ms;
        let conn = self.lock_conn()?;
        let deleted = conn
            .execute(
                "DELETE FROM sessions WHERE updated_at < ?1",
                params![cutoff],
            )
            .context("failed to prune stale sessions")?;
        Ok(deleted)
    }

    pub fn set_delivery_context(
        &self,
        channel: &str,
        target_key: &str,
        context: &serde_json::Value,
        ttl_hours: u64,
    ) -> Result<()> {
        let delivered_at = now_millis();
        let expires_at = delivered_at + (ttl_hours as i64 * 60 * 60 * 1000);
        let context_json = serde_json::to_string(context).context("failed to serialize context")?;

        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO delivery_context
             (channel, target_key, context, delivered_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![channel, target_key, context_json, delivered_at, expires_at],
        )
        .context("failed to set delivery context")?;

        Ok(())
    }

    pub fn pop_delivery_context(
        &self,
        channel: &str,
        target_key: &str,
    ) -> Result<Option<serde_json::Value>> {
        let conn = self.lock_conn()?;

        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT context, expires_at
                 FROM delivery_context
                 WHERE channel = ?1 AND target_key = ?2",
                params![channel, target_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context("failed to fetch delivery context")?;

        let Some((context_json, expires_at)) = row else {
            return Ok(None);
        };

        conn.execute(
            "DELETE FROM delivery_context WHERE channel = ?1 AND target_key = ?2",
            params![channel, target_key],
        )
        .context("failed to remove delivery context")?;

        if expires_at <= now_millis() {
            return Ok(None);
        }

        let value =
            serde_json::from_str(&context_json).context("failed to parse delivery context")?;
        Ok(Some(value))
    }

    pub fn prune_delivery_contexts(&self) -> Result<usize> {
        let conn = self.lock_conn()?;
        let deleted = conn
            .execute(
                "DELETE FROM delivery_context WHERE expires_at <= ?1",
                params![now_millis()],
            )
            .context("failed to prune delivery contexts")?;
        Ok(deleted)
    }

    fn run_migrations(&self) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                routing_key  TEXT PRIMARY KEY,
                session_id   TEXT NOT NULL,
                title        TEXT NOT NULL,
                channel      TEXT NOT NULL,
                peer_id      TEXT NOT NULL,
                chat_type    TEXT NOT NULL,
                channel_meta TEXT NOT NULL DEFAULT '{}',
                created_at   INTEGER NOT NULL,
                updated_at   INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_session_id ON sessions(session_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_channel ON sessions(channel);

            CREATE TABLE IF NOT EXISTS delivery_context (
                channel      TEXT NOT NULL,
                target_key   TEXT NOT NULL,
                context      TEXT NOT NULL,
                delivered_at INTEGER NOT NULL,
                expires_at   INTEGER NOT NULL,
                PRIMARY KEY (channel, target_key)
            );
            ",
        )
        .context("failed to run gateway store migrations")?;

        Ok(())
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock sqlite connection"))
    }
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn parse_json_value<T: serde::de::DeserializeOwned>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{GatewayStore, SessionMapping, now_millis};
    use crate::types::{ChannelId, ChatType, DeliveryContext, PeerId};

    fn sample_mapping(session_id: &str, updated_at: i64) -> SessionMapping {
        SessionMapping {
            session_id: session_id.to_string(),
            title: "sample".to_string(),
            delivery: DeliveryContext {
                channel: ChannelId::from("telegram"),
                peer_id: PeerId::from("123"),
                chat_type: ChatType::Direct,
                channel_meta: json!({"chat_id": 1}),
                updated_at,
            },
            created_at: updated_at - 10,
        }
    }

    #[test]
    fn set_get_round_trip() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };
        let mapping = sample_mapping("s1", now_millis());

        if let Err(error) = store.set("rk", &mapping) {
            panic!("set failed: {error}");
        }

        let fetched = match store.get("rk") {
            Ok(value) => value,
            Err(error) => panic!("get failed: {error}"),
        };

        assert!(fetched.is_some());
        let fetched = fetched.unwrap_or_else(|| panic!("expected mapping"));
        assert_eq!(fetched.session_id, "s1");
        assert_eq!(fetched.delivery.channel.0, "telegram");
    }

    #[test]
    fn find_by_session_id_returns_mapping() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) = store.set("rk", &sample_mapping("s1", now_millis())) {
            panic!("set failed: {error}");
        }

        let found = match store.find_by_session_id("s1") {
            Ok(value) => value,
            Err(error) => panic!("find_by_session_id failed: {error}"),
        };

        assert!(found.is_some());
        let (routing_key, mapping) = found.unwrap_or_else(|| panic!("expected mapping"));
        assert_eq!(routing_key, "rk");
        assert_eq!(mapping.session_id, "s1");
    }

    #[test]
    fn update_delivery_only_changes_delivery_fields() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) = store.set("rk", &sample_mapping("s1", now_millis() - 1000)) {
            panic!("set failed: {error}");
        }

        let delivery = DeliveryContext {
            channel: ChannelId::from("discord"),
            peer_id: PeerId::from("456"),
            chat_type: ChatType::Group {
                id: "g1".to_string(),
            },
            channel_meta: json!({"channel_id": "abc"}),
            updated_at: now_millis(),
        };

        if let Err(error) = store.update_delivery("rk", &delivery) {
            panic!("update_delivery failed: {error}");
        }

        let fetched = match store.get("rk") {
            Ok(value) => value,
            Err(error) => panic!("get failed: {error}"),
        };

        let fetched = fetched.unwrap_or_else(|| panic!("expected mapping"));
        assert_eq!(fetched.session_id, "s1");
        assert_eq!(fetched.delivery.channel.0, "discord");
        assert_eq!(fetched.delivery.peer_id.0, "456");
    }

    #[test]
    fn get_unknown_returns_none() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        let fetched = match store.get("missing") {
            Ok(value) => value,
            Err(error) => panic!("get failed: {error}"),
        };

        assert!(fetched.is_none());
    }

    #[test]
    fn prune_removes_old_entries() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        let now = now_millis();

        if let Err(error) = store.set("old", &sample_mapping("s-old", now - 20_000)) {
            panic!("set old failed: {error}");
        }

        if let Err(error) = store.set("new", &sample_mapping("s-new", now)) {
            panic!("set new failed: {error}");
        }

        let deleted = match store.prune(5_000) {
            Ok(value) => value,
            Err(error) => panic!("prune failed: {error}"),
        };

        assert_eq!(deleted, 1);

        let old_exists = match store.get("old") {
            Ok(value) => value.is_some(),
            Err(error) => panic!("get old failed: {error}"),
        };
        let new_exists = match store.get("new") {
            Ok(value) => value.is_some(),
            Err(error) => panic!("get new failed: {error}"),
        };

        assert!(!old_exists);
        assert!(new_exists);
    }

    #[test]
    fn set_overwrites_existing_key() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) = store.set("rk", &sample_mapping("s1", now_millis())) {
            panic!("set first failed: {error}");
        }
        if let Err(error) = store.set("rk", &sample_mapping("s2", now_millis())) {
            panic!("set second failed: {error}");
        }

        let fetched = match store.get("rk") {
            Ok(value) => value,
            Err(error) => panic!("get failed: {error}"),
        };

        let fetched = fetched.unwrap_or_else(|| panic!("expected mapping"));
        assert_eq!(fetched.session_id, "s2");
    }

    #[test]
    fn list_orders_by_updated_at_desc() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };
        let now = now_millis();

        if let Err(error) = store.set("rk1", &sample_mapping("s1", now - 100)) {
            panic!("set rk1 failed: {error}");
        }
        if let Err(error) = store.set("rk2", &sample_mapping("s2", now)) {
            panic!("set rk2 failed: {error}");
        }

        let rows = match store.list(10) {
            Ok(value) => value,
            Err(error) => panic!("list failed: {error}"),
        };

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "rk2");
        assert_eq!(rows[1].0, "rk1");
    }

    #[test]
    fn delete_removes_mapping() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) = store.set("rk", &sample_mapping("s1", now_millis())) {
            panic!("set failed: {error}");
        }
        if let Err(error) = store.delete("rk") {
            panic!("delete failed: {error}");
        }

        let exists = match store.get("rk") {
            Ok(value) => value.is_some(),
            Err(error) => panic!("get failed: {error}"),
        };

        assert!(!exists);
    }

    #[test]
    fn set_and_pop_delivery_context_round_trip() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) = store.set_delivery_context(
            "telegram",
            "telegram:chat:1",
            &json!({"trigger": "cleanup"}),
            4,
        ) {
            panic!("set_delivery_context failed: {error}");
        }

        let popped = match store.pop_delivery_context("telegram", "telegram:chat:1") {
            Ok(value) => value,
            Err(error) => panic!("pop_delivery_context failed: {error}"),
        };

        let popped = popped.unwrap_or_else(|| panic!("expected delivery context"));
        assert_eq!(popped["trigger"], "cleanup");

        let popped_again = match store.pop_delivery_context("telegram", "telegram:chat:1") {
            Ok(value) => value,
            Err(error) => panic!("second pop_delivery_context failed: {error}"),
        };

        assert!(popped_again.is_none());
    }

    #[test]
    fn pop_delivery_context_returns_none_for_expired_rows() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) = store.set_delivery_context(
            "telegram",
            "telegram:chat:1",
            &json!({"trigger": "cleanup"}),
            0,
        ) {
            panic!("set_delivery_context failed: {error}");
        }

        let popped = match store.pop_delivery_context("telegram", "telegram:chat:1") {
            Ok(value) => value,
            Err(error) => panic!("pop_delivery_context failed: {error}"),
        };

        assert!(popped.is_none());
    }

    #[test]
    fn set_delivery_context_replaces_existing_target() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) =
            store.set_delivery_context("telegram", "telegram:chat:1", &json!({"trigger": "old"}), 4)
        {
            panic!("set first context failed: {error}");
        }

        if let Err(error) =
            store.set_delivery_context("telegram", "telegram:chat:1", &json!({"trigger": "new"}), 4)
        {
            panic!("set second context failed: {error}");
        }

        let popped = match store.pop_delivery_context("telegram", "telegram:chat:1") {
            Ok(value) => value,
            Err(error) => panic!("pop_delivery_context failed: {error}"),
        };

        let popped = popped.unwrap_or_else(|| panic!("expected delivery context"));
        assert_eq!(popped["trigger"], "new");
    }

    #[test]
    fn prune_delivery_contexts_removes_expired_only() {
        let store = match GatewayStore::open_in_memory() {
            Ok(value) => value,
            Err(error) => panic!("failed to open in-memory store: {error}"),
        };

        if let Err(error) =
            store.set_delivery_context("telegram", "telegram:chat:expired", &json!({"id": 1}), 0)
        {
            panic!("set expired context failed: {error}");
        }

        if let Err(error) =
            store.set_delivery_context("telegram", "telegram:chat:valid", &json!({"id": 2}), 4)
        {
            panic!("set valid context failed: {error}");
        }

        let deleted = match store.prune_delivery_contexts() {
            Ok(value) => value,
            Err(error) => panic!("prune_delivery_contexts failed: {error}"),
        };

        assert_eq!(deleted, 1);

        let valid = match store.pop_delivery_context("telegram", "telegram:chat:valid") {
            Ok(value) => value,
            Err(error) => panic!("pop valid context failed: {error}"),
        };
        assert!(valid.is_some());
    }
}
