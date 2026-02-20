//! Models.dev Registry
//!
//! Fetches and caches model definitions from <https://models.dev/api.json>.
//! Models are filtered to only include those compatible with agent use (tool calling support).

use crate::error::{Error, Result};
use crate::types::{Model, ModelCost, ModelLimit};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// =============================================================================
// Constants
// =============================================================================

/// URL to fetch model definitions from
pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Default cache file path (relative to home directory)
pub const DEFAULT_CACHE_PATH: &str = ".stakpak/cache/models.json";

// =============================================================================
// Public Types
// =============================================================================

/// Provider information from models.dev
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// Provider identifier (e.g., "anthropic", "openai", "google")
    pub id: String,
    /// Human-readable provider name
    pub name: String,
    /// Environment variable names for authentication
    #[serde(default)]
    pub env: Vec<String>,
    /// API base URL
    #[serde(default)]
    pub api: Option<String>,
    /// Available models keyed by model ID
    pub models: HashMap<String, Model>,
}

// =============================================================================
// Raw API Types (Internal)
// =============================================================================

#[derive(Debug, Clone, Deserialize)]
struct RawModel {
    id: String,
    name: String,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    tool_call: bool,
    #[serde(default)]
    cost: Option<RawCost>,
    #[serde(default)]
    limit: RawLimit,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawCost {
    #[serde(default)]
    input: f64,
    #[serde(default)]
    output: f64,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawLimit {
    #[serde(default)]
    context: u64,
    #[serde(default)]
    output: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct RawProvider {
    name: String,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    api: Option<String>,
    models: HashMap<String, RawModel>,
}

/// Cache file structure (wraps providers with metadata)
#[derive(Debug, Clone, Deserialize)]
struct CacheFile {
    #[allow(dead_code)]
    fetched_at: u64,
    providers: HashMap<String, ProviderInfo>,
}

// =============================================================================
// API Functions
// =============================================================================

/// Fetch models from models.dev API
///
/// Makes an HTTP request to models.dev and returns parsed provider data.
/// Models are filtered to only include those with tool calling support.
pub async fn fetch_models_dev() -> Result<HashMap<String, ProviderInfo>> {
    let client = crate::providers::tls::create_platform_tls_client()
        .map_err(|e| Error::NetworkError(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(MODELS_DEV_URL)
        .header("User-Agent", "stakpak-cli")
        .send()
        .await
        .map_err(|e| Error::NetworkError(format!("Failed to fetch models.dev: {}", e)))?;

    if !response.status().is_success() {
        return Err(Error::NetworkError(format!(
            "models.dev returned status {}",
            response.status()
        )));
    }

    let raw: HashMap<String, RawProvider> = response
        .json()
        .await
        .map_err(|e| Error::NetworkError(format!("Failed to parse models.dev response: {}", e)))?;

    Ok(convert_raw_providers(raw))
}

/// Parse raw models.dev JSON (for testing or direct API responses)
pub fn parse_models_dev(json: &str) -> Result<HashMap<String, ProviderInfo>> {
    let raw: HashMap<String, RawProvider> = serde_json::from_str(json)
        .map_err(|e| Error::ConfigError(format!("Failed to parse models JSON: {}", e)))?;

    Ok(convert_raw_providers(raw))
}

// =============================================================================
// Cache Functions
// =============================================================================

/// Load models for a specific provider from the default cache location
///
/// Returns models for the given provider ID, or an empty vec if not found.
/// Uses `~/.stakpak/cache/models.json` as the cache file.
pub fn load_models_for_provider(provider_id: &str) -> Result<Vec<Model>> {
    let cache_path = dirs::home_dir()
        .unwrap_or_default()
        .join(DEFAULT_CACHE_PATH);

    load_models_for_provider_from_path(provider_id, &cache_path)
}

/// Load models for a specific provider from a custom cache path
pub fn load_models_for_provider_from_path(provider_id: &str, path: &Path) -> Result<Vec<Model>> {
    let providers = load_cache_file(path)?;

    Ok(providers
        .get(provider_id)
        .map(|p| p.models.values().cloned().collect())
        .unwrap_or_default())
}

/// Load all models from providers that have authentication configured
pub fn load_available_models() -> Result<Vec<Model>> {
    let cache_path = dirs::home_dir()
        .unwrap_or_default()
        .join(DEFAULT_CACHE_PATH);

    let providers = load_cache_file(&cache_path)?;
    Ok(get_available_models(&providers))
}

// =============================================================================
// Filter Functions
// =============================================================================

/// Filter providers to only those with authentication configured
///
/// A provider is considered configured if any of its environment variables are set.
pub fn filter_configured_providers(
    providers: &HashMap<String, ProviderInfo>,
) -> HashMap<String, ProviderInfo> {
    providers
        .iter()
        .filter(|(_, p)| p.env.iter().any(|var| is_env_set(var)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Get all models from configured providers as a flat list
pub fn get_available_models(providers: &HashMap<String, ProviderInfo>) -> Vec<Model> {
    filter_configured_providers(providers)
        .values()
        .flat_map(|p| p.models.values().cloned())
        .collect()
}

// =============================================================================
// Internal Helpers
// =============================================================================

/// Load and parse cache file (handles both cache wrapper and raw formats)
fn load_cache_file(path: &Path) -> Result<HashMap<String, ProviderInfo>> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::ConfigError(format!(
            "Failed to read models cache at {}: {}",
            path.display(),
            e
        ))
    })?;

    // Try cache wrapper format first (has fetched_at + providers)
    if let Ok(cache) = serde_json::from_str::<CacheFile>(&content) {
        return Ok(cache.providers);
    }

    // Fall back to raw API format
    parse_models_dev(&content)
}

/// Convert raw API providers to our model format with filtering
fn convert_raw_providers(raw: HashMap<String, RawProvider>) -> HashMap<String, ProviderInfo> {
    raw.into_iter()
        .map(|(id, provider)| {
            let models = provider
                .models
                .into_iter()
                .filter(|(_, m)| is_model_compatible(m))
                .map(|(model_id, m)| (model_id, convert_raw_model(m, &id)))
                .collect();

            let info = ProviderInfo {
                id: id.clone(),
                name: provider.name,
                env: provider.env,
                api: provider.api,
                models,
            };

            (id, info)
        })
        .collect()
}

/// Check if a model is compatible with agent use
fn is_model_compatible(model: &RawModel) -> bool {
    // Must support tool calls for agent functionality
    model.tool_call
        // Exclude deprecated models
        && model.status.as_deref() != Some("deprecated")
        // Exclude alpha/experimental models
        && model.status.as_deref() != Some("alpha")
        // Exclude embedding models
        && !is_embedding_model(&model.id)
}

/// Check if a model is an embedding model (not for chat/completions)
fn is_embedding_model(model_id: &str) -> bool {
    let id_lower = model_id.to_lowercase();
    id_lower.contains("embed")
}

/// Convert a raw model to our model format
fn convert_raw_model(raw: RawModel, provider_id: &str) -> Model {
    Model {
        id: raw.id,
        name: raw.name,
        provider: provider_id.to_string(),
        reasoning: raw.reasoning,
        cost: raw.cost.map(|c| ModelCost {
            input: c.input,
            output: c.output,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
        }),
        limit: ModelLimit {
            context: raw.limit.context,
            output: raw.limit.output,
        },
        release_date: raw.release_date,
    }
}

/// Check if an environment variable is set and non-empty
fn is_env_set(var: &str) -> bool {
    std::env::var(var).map(|v| !v.is_empty()).unwrap_or(false)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_models_dev() {
        let json = r#"{
            "anthropic": {
                "name": "Anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "api": "https://api.anthropic.com/v1",
                "models": {
                    "claude-sonnet-4": {
                        "id": "claude-sonnet-4",
                        "name": "Claude Sonnet 4",
                        "reasoning": true,
                        "tool_call": true,
                        "cost": { "input": 3.0, "output": 15.0, "cache_read": 0.3 },
                        "limit": { "context": 200000, "output": 64000 }
                    }
                }
            }
        }"#;

        let providers = parse_models_dev(json).unwrap();

        assert!(providers.contains_key("anthropic"));
        let anthropic = &providers["anthropic"];
        assert_eq!(anthropic.name, "Anthropic");
        assert_eq!(anthropic.env, vec!["ANTHROPIC_API_KEY"]);

        let model = &anthropic.models["claude-sonnet-4"];
        assert_eq!(model.name, "Claude Sonnet 4");
        assert!(model.reasoning);
        assert_eq!(model.cost.as_ref().unwrap().input, 3.0);
    }

    #[test]
    fn test_model_filtering() {
        let json = r#"{
            "test": {
                "name": "Test Provider",
                "env": [],
                "models": {
                    "good": { "id": "good", "name": "Good Model", "tool_call": true, "limit": {} },
                    "deprecated": { "id": "deprecated", "name": "Old", "tool_call": true, "status": "deprecated", "limit": {} },
                    "alpha": { "id": "alpha", "name": "Experimental", "tool_call": true, "status": "alpha", "limit": {} },
                    "no_tools": { "id": "no_tools", "name": "No Tools", "tool_call": false, "limit": {} },
                    "text-embedding-3": { "id": "text-embedding-3", "name": "Embedding", "tool_call": true, "limit": {} }
                }
            }
        }"#;

        let providers = parse_models_dev(json).unwrap();
        let test = &providers["test"];

        // Only "good" should remain (others filtered out)
        assert_eq!(test.models.len(), 1);
        assert!(test.models.contains_key("good"));
    }

    #[test]
    fn test_embedding_model_detection() {
        assert!(is_embedding_model("text-embedding-3-large"));
        assert!(is_embedding_model("openai/text-embedding-ada-002"));
        assert!(is_embedding_model("gemini-embedding-001"));
        assert!(is_embedding_model("EMBED-something"));
        assert!(!is_embedding_model("gpt-4"));
        assert!(!is_embedding_model("claude-sonnet-4"));
    }
}
