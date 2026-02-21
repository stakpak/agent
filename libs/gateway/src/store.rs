use std::path::Path;

use anyhow::{Context, Result, anyhow};
use libsql::{Connection, Database};
use tempfile::TempDir;

use crate::types::DeliveryContext;

#[derive(Debug, Clone)]
pub struct SessionMapping {
    pub session_id: String,
    pub title: String,
    pub delivery: DeliveryContext,
    pub created_at: i64,
}

pub struct GatewayStore {
    /// Keep the libsql Database handle alive for the lifetime of each operation connection.
    db: Database,
    /// Owns temporary backing storage for in-memory test mode and cleans it on drop.
    _temp_dir: Option<TempDir>,
}

impl GatewayStore {
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create store parent dir: {}", parent.display())
            })?;
        }

        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .with_context(|| format!("failed to open sqlite db: {}", path.display()))?;
        let store = Self {
            db,
            _temp_dir: None,
        };
        store.configure_pragmas().await?;
        store.run_migrations().await?;
        Ok(store)
    }

    pub async fn open_in_memory() -> Result<Self> {
        // libsql in-memory databases are connection-scoped; use a temp directory
        // and clean it automatically when the store is dropped.
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let db_path = temp_dir.path().join("gateway.db");
        let db = libsql::Builder::new_local(&db_path)
            .build()
            .await
            .with_context(|| format!("failed to open temp sqlite db: {}", db_path.display()))?;
        let store = Self {
            db,
            _temp_dir: Some(temp_dir),
        };
        store.configure_pragmas().await?;
        store.run_migrations().await?;
        Ok(store)
    }

    async fn configure_pragmas(&self) -> Result<()> {
        let conn = self.connection()?;
        // journal_mode returns a result row, so use query() instead of execute()
        conn.query("PRAGMA journal_mode = WAL", ())
            .await
            .context("failed to set journal_mode")?;
        conn.query("PRAGMA busy_timeout = 5000", ())
            .await
            .context("failed to set busy_timeout")?;
        conn.query("PRAGMA synchronous = NORMAL", ())
            .await
            .context("failed to set synchronous")?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        self.db.connect().context("failed to connect sqlite db")
    }

    pub async fn get(&self, routing_key: &str) -> Result<Option<SessionMapping>> {
        let conn = self.connection()?;
        let mut rows = conn
            .query(
                "SELECT session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at
                 FROM sessions
                 WHERE routing_key = ?",
                [routing_key],
            )
            .await
            .context("failed to fetch session mapping")?;

        let Some(row) = rows
            .next()
            .await
            .context("failed to fetch session mapping row")?
        else {
            return Ok(None);
        };

        Ok(Some(parse_session_mapping_row(&row, 0)?))
    }

    pub async fn set(&self, routing_key: &str, mapping: &SessionMapping) -> Result<()> {
        let chat_type = serde_json::to_string(&mapping.delivery.chat_type)
            .context("failed to serialize chat_type")?;
        let channel_meta = serde_json::to_string(&mapping.delivery.channel_meta)
            .context("failed to serialize channel_meta")?;

        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO sessions
             (routing_key, session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                routing_key,
                mapping.session_id.as_str(),
                mapping.title.as_str(),
                mapping.delivery.channel.0.as_str(),
                mapping.delivery.peer_id.0.as_str(),
                chat_type.as_str(),
                channel_meta.as_str(),
                mapping.created_at,
                mapping.delivery.updated_at,
            ),
        )
        .await
        .context("failed to upsert session mapping")?;

        Ok(())
    }

    pub async fn find_by_session_id(
        &self,
        session_id: &str,
    ) -> Result<Option<(String, SessionMapping)>> {
        let conn = self.connection()?;
        let mut rows = conn
            .query(
                "SELECT routing_key, session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at
                 FROM sessions
                 WHERE session_id = ?
                 ORDER BY updated_at DESC
                 LIMIT 1",
                [session_id],
            )
            .await
            .context("failed to query by session_id")?;

        let Some(row) = rows.next().await.context("failed to read session_id row")? else {
            return Ok(None);
        };

        let routing_key: String = row.get(0).context("failed to parse routing_key")?;
        let mapping = parse_session_mapping_row(&row, 1)?;

        Ok(Some((routing_key, mapping)))
    }

    pub async fn update_delivery(
        &self,
        routing_key: &str,
        delivery: &DeliveryContext,
    ) -> Result<()> {
        let chat_type =
            serde_json::to_string(&delivery.chat_type).context("failed to serialize chat_type")?;
        let channel_meta = serde_json::to_string(&delivery.channel_meta)
            .context("failed to serialize channel_meta")?;

        let conn = self.connection()?;
        conn.execute(
            "UPDATE sessions
             SET channel = ?,
                 peer_id = ?,
                 chat_type = ?,
                 channel_meta = ?,
                 updated_at = ?
             WHERE routing_key = ?",
            (
                delivery.channel.0.as_str(),
                delivery.peer_id.0.as_str(),
                chat_type.as_str(),
                channel_meta.as_str(),
                delivery.updated_at,
                routing_key,
            ),
        )
        .await
        .context("failed to update delivery context")?;

        Ok(())
    }

    pub async fn list(&self, limit: usize) -> Result<Vec<(String, SessionMapping)>> {
        let conn = self.connection()?;
        let mut rows = conn
            .query(
                "SELECT routing_key, session_id, title, channel, peer_id, chat_type, channel_meta, created_at, updated_at
                 FROM sessions
                 ORDER BY updated_at DESC
                 LIMIT ?",
                [limit as i64],
            )
            .await
            .context("failed to list session mappings")?;

        let mut out = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .context("failed to read session mappings row")?
        {
            let routing_key: String = row.get(0).context("failed to parse routing_key")?;
            let mapping = parse_session_mapping_row(&row, 1)?;
            out.push((routing_key, mapping));
        }

        Ok(out)
    }

    pub async fn delete(&self, routing_key: &str) -> Result<()> {
        let conn = self.connection()?;
        conn.execute("DELETE FROM sessions WHERE routing_key = ?", [routing_key])
            .await
            .context("failed to delete routing key")?;

        Ok(())
    }

    pub async fn prune(&self, max_age_ms: i64) -> Result<usize> {
        let cutoff = now_millis() - max_age_ms;
        let conn = self.connection()?;
        let deleted = conn
            .execute("DELETE FROM sessions WHERE updated_at < ?", [cutoff])
            .await
            .context("failed to prune stale sessions")?;

        Ok(deleted as usize)
    }

    pub async fn set_delivery_context(
        &self,
        channel: &str,
        target_key: &str,
        context: &serde_json::Value,
        ttl_hours: u64,
    ) -> Result<()> {
        let delivered_at = now_millis();
        let expires_at = delivered_at + (ttl_hours as i64 * 60 * 60 * 1000);
        let context_json = serde_json::to_string(context).context("failed to serialize context")?;

        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO delivery_context
             (channel, target_key, context, delivered_at, expires_at)
             VALUES (?, ?, ?, ?, ?)",
            (
                channel,
                target_key,
                context_json.as_str(),
                delivered_at,
                expires_at,
            ),
        )
        .await
        .context("failed to set delivery context")?;

        Ok(())
    }

    pub async fn pop_delivery_context(
        &self,
        channel: &str,
        target_key: &str,
    ) -> Result<Option<serde_json::Value>> {
        let conn = self.connection()?;

        let mut rows = conn
            .query(
                "DELETE FROM delivery_context
                 WHERE channel = ? AND target_key = ?
                 RETURNING context, expires_at",
                (channel, target_key),
            )
            .await
            .context("failed to atomically pop delivery context")?;

        let Some(row) = rows
            .next()
            .await
            .context("failed to read popped delivery context row")?
        else {
            return Ok(None);
        };

        let context_json: String = row.get(0).context("failed to parse delivery context")?;
        let expires_at: i64 = row
            .get(1)
            .context("failed to parse delivery context expiry")?;

        if expires_at <= now_millis() {
            return Ok(None);
        }

        let value =
            serde_json::from_str(&context_json).context("failed to parse delivery context")?;
        Ok(Some(value))
    }

    pub async fn prune_delivery_contexts(&self) -> Result<usize> {
        let conn = self.connection()?;
        let deleted = conn
            .execute(
                "DELETE FROM delivery_context WHERE expires_at <= ?",
                [now_millis()],
            )
            .await
            .context("failed to prune delivery contexts")?;

        Ok(deleted as usize)
    }

    async fn run_migrations(&self) -> Result<()> {
        let conn = self.connection()?;
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
        .await
        .context("failed to run gateway store migrations")?;

        Ok(())
    }
}

fn parse_session_mapping_row(row: &libsql::Row, start_idx: usize) -> Result<SessionMapping> {
    let col = |offset: usize| -> Result<i32> {
        i32::try_from(start_idx + offset)
            .map_err(|_| anyhow!("column index overflow at {}", start_idx + offset))
    };

    let session_id: String = row
        .get(col(0)?)
        .with_context(|| format!("failed to parse session_id at column {start_idx}"))?;
    let title: String = row
        .get(col(1)?)
        .with_context(|| format!("failed to parse title at column {}", start_idx + 1))?;
    let channel: String = row
        .get(col(2)?)
        .with_context(|| format!("failed to parse channel at column {}", start_idx + 2))?;
    let peer_id: String = row
        .get(col(3)?)
        .with_context(|| format!("failed to parse peer_id at column {}", start_idx + 3))?;
    let chat_type_json: String = row
        .get(col(4)?)
        .with_context(|| format!("failed to parse chat_type at column {}", start_idx + 4))?;
    let channel_meta_json: String = row
        .get(col(5)?)
        .with_context(|| format!("failed to parse channel_meta at column {}", start_idx + 5))?;
    let created_at: i64 = row
        .get(col(6)?)
        .with_context(|| format!("failed to parse created_at at column {}", start_idx + 6))?;
    let updated_at: i64 = row
        .get(col(7)?)
        .with_context(|| format!("failed to parse updated_at at column {}", start_idx + 7))?;

    let chat_type = parse_json_value(&chat_type_json, "chat_type")?;
    let channel_meta = parse_json_value(&channel_meta_json, "channel_meta")?;

    Ok(SessionMapping {
        session_id,
        title,
        delivery: DeliveryContext {
            channel: channel.into(),
            peer_id: peer_id.into(),
            chat_type,
            channel_meta,
            updated_at,
        },
        created_at,
    })
}

fn parse_json_value<T: serde::de::DeserializeOwned>(value: &str, field: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|error| anyhow!("failed to parse {field} JSON: {error}"))
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(all(test, feature = "libsql-test"))]
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

    #[tokio::test]
    async fn set_get_round_trip() {
        let store = GatewayStore::open_in_memory().await.expect("store");
        let mapping = sample_mapping("s1", now_millis());

        store.set("rk", &mapping).await.expect("set");
        let fetched = store.get("rk").await.expect("get").expect("mapping");

        assert_eq!(fetched.session_id, "s1");
        assert_eq!(fetched.delivery.channel.0, "telegram");
    }

    #[tokio::test]
    async fn find_by_session_id_returns_mapping() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set("rk", &sample_mapping("s1", now_millis()))
            .await
            .expect("set");

        let (routing_key, mapping) = store
            .find_by_session_id("s1")
            .await
            .expect("find")
            .expect("mapping");

        assert_eq!(routing_key, "rk");
        assert_eq!(mapping.session_id, "s1");
    }

    #[tokio::test]
    async fn update_delivery_only_changes_delivery_fields() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set("rk", &sample_mapping("s1", now_millis() - 1000))
            .await
            .expect("set");

        let delivery = DeliveryContext {
            channel: ChannelId::from("discord"),
            peer_id: PeerId::from("456"),
            chat_type: ChatType::Group {
                id: "g1".to_string(),
            },
            channel_meta: json!({"channel_id": "abc"}),
            updated_at: now_millis(),
        };

        store
            .update_delivery("rk", &delivery)
            .await
            .expect("update_delivery");

        let fetched = store.get("rk").await.expect("get").expect("mapping");
        assert_eq!(fetched.session_id, "s1");
        assert_eq!(fetched.delivery.channel.0, "discord");
        assert_eq!(fetched.delivery.peer_id.0, "456");
    }

    #[tokio::test]
    async fn get_unknown_returns_none() {
        let store = GatewayStore::open_in_memory().await.expect("store");
        let fetched = store.get("missing").await.expect("get");
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn prune_removes_old_entries() {
        let store = GatewayStore::open_in_memory().await.expect("store");
        let now = now_millis();

        store
            .set("old", &sample_mapping("s-old", now - 20_000))
            .await
            .expect("set old");
        store
            .set("new", &sample_mapping("s-new", now))
            .await
            .expect("set new");

        let deleted = store.prune(5_000).await.expect("prune");
        assert_eq!(deleted, 1);

        assert!(store.get("old").await.expect("get old").is_none());
        assert!(store.get("new").await.expect("get new").is_some());
    }

    #[tokio::test]
    async fn set_overwrites_existing_key() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set("rk", &sample_mapping("s1", now_millis()))
            .await
            .expect("set 1");
        store
            .set("rk", &sample_mapping("s2", now_millis()))
            .await
            .expect("set 2");

        let fetched = store.get("rk").await.expect("get").expect("mapping");
        assert_eq!(fetched.session_id, "s2");
    }

    #[tokio::test]
    async fn list_orders_by_updated_at_desc() {
        let store = GatewayStore::open_in_memory().await.expect("store");
        let now = now_millis();

        store
            .set("rk1", &sample_mapping("s1", now - 100))
            .await
            .expect("set rk1");
        store
            .set("rk2", &sample_mapping("s2", now))
            .await
            .expect("set rk2");

        let rows = store.list(10).await.expect("list");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "rk2");
        assert_eq!(rows[1].0, "rk1");
    }

    #[tokio::test]
    async fn delete_removes_mapping() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set("rk", &sample_mapping("s1", now_millis()))
            .await
            .expect("set");
        store.delete("rk").await.expect("delete");

        assert!(store.get("rk").await.expect("get").is_none());
    }

    #[tokio::test]
    async fn set_and_pop_delivery_context_round_trip() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set_delivery_context(
                "telegram",
                "telegram:chat:1",
                &json!({"trigger": "cleanup"}),
                4,
            )
            .await
            .expect("set_delivery_context");

        let popped = store
            .pop_delivery_context("telegram", "telegram:chat:1")
            .await
            .expect("pop_delivery_context")
            .expect("delivery context");
        assert_eq!(popped["trigger"], "cleanup");

        let popped_again = store
            .pop_delivery_context("telegram", "telegram:chat:1")
            .await
            .expect("second pop");
        assert!(popped_again.is_none());
    }

    #[tokio::test]
    async fn pop_delivery_context_is_atomic_under_concurrency() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set_delivery_context(
                "telegram",
                "telegram:chat:1",
                &json!({"trigger": "cleanup"}),
                4,
            )
            .await
            .expect("set_delivery_context");

        let (first, second) = tokio::join!(
            store.pop_delivery_context("telegram", "telegram:chat:1"),
            store.pop_delivery_context("telegram", "telegram:chat:1")
        );

        let first = first.expect("first pop");
        let second = second.expect("second pop");
        let popped_count = [first, second].into_iter().flatten().count();
        assert_eq!(
            popped_count, 1,
            "delivery context should be consumed exactly once"
        );
    }

    #[tokio::test]
    async fn pop_delivery_context_returns_none_for_expired_rows() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set_delivery_context(
                "telegram",
                "telegram:chat:1",
                &json!({"trigger": "cleanup"}),
                0,
            )
            .await
            .expect("set_delivery_context");

        let popped = store
            .pop_delivery_context("telegram", "telegram:chat:1")
            .await
            .expect("pop_delivery_context");
        assert!(popped.is_none());
    }

    #[tokio::test]
    async fn set_delivery_context_replaces_existing_target() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set_delivery_context("telegram", "telegram:chat:1", &json!({"trigger": "old"}), 4)
            .await
            .expect("set old");
        store
            .set_delivery_context("telegram", "telegram:chat:1", &json!({"trigger": "new"}), 4)
            .await
            .expect("set new");

        let popped = store
            .pop_delivery_context("telegram", "telegram:chat:1")
            .await
            .expect("pop")
            .expect("delivery context");
        assert_eq!(popped["trigger"], "new");
    }

    #[tokio::test]
    async fn prune_delivery_contexts_removes_expired_only() {
        let store = GatewayStore::open_in_memory().await.expect("store");

        store
            .set_delivery_context("telegram", "telegram:chat:expired", &json!({"id": 1}), 0)
            .await
            .expect("set expired");
        store
            .set_delivery_context("telegram", "telegram:chat:valid", &json!({"id": 2}), 4)
            .await
            .expect("set valid");

        let deleted = store
            .prune_delivery_contexts()
            .await
            .expect("prune_delivery_contexts");
        assert_eq!(deleted, 1);

        let valid = store
            .pop_delivery_context("telegram", "telegram:chat:valid")
            .await
            .expect("pop valid");
        assert!(valid.is_some());
    }
}
