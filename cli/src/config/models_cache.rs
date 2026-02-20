//! Models Cache
//!
//! Manages local caching of model definitions from models.dev.
//! Cache location: `~/.stakpak/cache/models.json`
//! Cache TTL: 1 hour

use stakai::{ProviderInfo, fetch_models_dev};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;

// =============================================================================
// Constants
// =============================================================================

/// Cache time-to-live (1 hour)
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

/// Cache file path relative to home directory
const CACHE_PATH: &str = ".stakpak/cache/models.json";

// =============================================================================
// ModelsCache
// =============================================================================

/// Cached model definitions with metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelsCache {
    /// Unix timestamp when the cache was last fetched
    pub fetched_at: u64,
    /// Provider data keyed by provider ID
    pub providers: HashMap<String, ProviderInfo>,
}

impl ModelsCache {
    /// Load models from cache or fetch from API if stale/missing
    ///
    /// Strategy:
    /// 1. If valid cache exists, return it
    /// 2. If stale cache exists, try to refresh; use stale on failure
    /// 3. If no cache, fetch from API
    pub async fn get() -> Result<Self, String> {
        match Self::load().await {
            Some(cache) if !cache.is_stale() => Ok(cache),
            Some(stale_cache) => Self::refresh_or_use_stale(stale_cache).await,
            None => Self::fetch_and_save().await,
        }
    }

    // -------------------------------------------------------------------------
    // Private Methods
    // -------------------------------------------------------------------------

    fn is_stale(&self) -> bool {
        let fetched = UNIX_EPOCH + Duration::from_secs(self.fetched_at);
        SystemTime::now()
            .duration_since(fetched)
            .map(|d| d > CACHE_TTL)
            .unwrap_or(true)
    }

    fn cache_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(CACHE_PATH)
    }

    async fn load() -> Option<Self> {
        let content = fs::read_to_string(Self::cache_path()).await.ok()?;
        serde_json::from_str(&content).ok()
    }

    async fn save(&self) -> Result<(), String> {
        let path = Self::cache_path();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create cache directory: {e}"))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize cache: {e}"))?;

        fs::write(&path, json)
            .await
            .map_err(|e| format!("Failed to write cache: {e}"))
    }

    async fn fetch() -> Result<Self, String> {
        let providers = fetch_models_dev()
            .await
            .map_err(|e| format!("Failed to fetch from models.dev: {e}"))?;

        let fetched_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(Self {
            fetched_at,
            providers,
        })
    }

    async fn fetch_and_save() -> Result<Self, String> {
        let cache = Self::fetch().await?;
        if let Err(e) = cache.save().await {
            tracing::warn!("Failed to save models cache: {e}");
        }
        Ok(cache)
    }

    async fn refresh_or_use_stale(stale: Self) -> Result<Self, String> {
        match Self::fetch().await {
            Ok(fresh) => {
                if let Err(e) = fresh.save().await {
                    tracing::warn!("Failed to save models cache: {e}");
                }
                Ok(fresh)
            }
            Err(e) => {
                tracing::warn!("Using stale cache: {e}");
                Ok(stale)
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_cache_not_stale() {
        let cache = ModelsCache {
            fetched_at: now_secs(),
            providers: HashMap::new(),
        };
        assert!(!cache.is_stale());
    }

    #[test]
    fn test_old_cache_is_stale() {
        let cache = ModelsCache {
            fetched_at: now_secs() - 7200, // 2 hours ago
            providers: HashMap::new(),
        };
        assert!(cache.is_stale());
    }

    #[test]
    fn test_cache_path_contains_expected_path() {
        let path = ModelsCache::cache_path();
        assert!(
            path.to_string_lossy()
                .contains(".stakpak/cache/models.json")
        );
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}
