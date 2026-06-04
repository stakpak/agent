//! Knowledge-store endpoints (`/v1/knowledge/...`).
//!
//! Wraps the public knowledge CRUD surface with a local on-disk
//! revalidation cache. The cache is keyed by `<account>/<path>` and uses
//! the file body's SHA-256 as the ETag for `If-None-Match`. See [`cache`]
//! for the on-disk layout.

mod cache;

use super::client::{ApiError, StakpakApiClient};
use super::models::*;
use crate::models::GetMyAccountResponse;
use reqwest::{Response, StatusCode, header};
use serde::de::DeserializeOwned;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::debug;

/// How long to suppress repeat `/v1/account` lookups after a failed resolution.
/// Keeps a transient blip from turning every cached read into two round-trips
/// for the lifetime of the client.
const ACCOUNT_RESOLVE_BACKOFF: Duration = Duration::from_secs(60);

/// Memoized result of resolving the cache account name. Lives on
/// [`StakpakApiClient`] so the negative-cache survives across calls.
#[derive(Debug, Clone)]
pub(super) enum AccountCacheState {
    /// Not yet attempted.
    Unknown,
    /// Successfully resolved; reuse forever.
    Resolved(String),
    /// Last attempt failed; don't retry until `until`.
    Failed { until: Instant },
}

/// Structured error returned by the knowledge-store APIs.
#[derive(Debug, Clone)]
pub enum KnowledgeApiError {
    /// Resource does not exist (HTTP 404).
    NotFound { message: String },
    /// Resource already exists (HTTP 409).
    Conflict { message: String },
    /// Caller is not authorized (HTTP 401 / 403).
    Forbidden { message: String },
    /// Request was rejected by the server (HTTP 400).
    BadRequest { message: String },
    /// Catch-all for any other HTTP error status, plus the raw body.
    Http { status: StatusCode, message: String },
    /// Transport / serialization / IO failure (no HTTP status available).
    Transport { message: String },
}

impl KnowledgeApiError {
    pub fn message(&self) -> &str {
        match self {
            Self::NotFound { message }
            | Self::Conflict { message }
            | Self::Forbidden { message }
            | Self::BadRequest { message }
            | Self::Http { message, .. }
            | Self::Transport { message } => message,
        }
    }

    /// Returns the HTTP status if the error came from the server.
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::NotFound { .. } => Some(StatusCode::NOT_FOUND),
            Self::Conflict { .. } => Some(StatusCode::CONFLICT),
            Self::Forbidden { .. } => Some(StatusCode::FORBIDDEN),
            Self::BadRequest { .. } => Some(StatusCode::BAD_REQUEST),
            Self::Http { status, .. } => Some(*status),
            Self::Transport { .. } => None,
        }
    }
}

impl std::fmt::Display for KnowledgeApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { message } => write!(f, "not found: {}", message),
            Self::Conflict { message } => write!(f, "conflict: {}", message),
            Self::Forbidden { message } => write!(f, "forbidden: {}", message),
            Self::BadRequest { message } => write!(f, "bad request: {}", message),
            Self::Http { status, message } => write!(f, "http {}: {}", status, message),
            Self::Transport { message } => write!(f, "transport error: {}", message),
        }
    }
}

impl std::error::Error for KnowledgeApiError {}

impl From<reqwest::Error> for KnowledgeApiError {
    fn from(err: reqwest::Error) -> Self {
        Self::Transport {
            message: err.to_string(),
        }
    }
}

/// Percent-encode each segment of a path independently, preserving `/`
/// separators so the URL still matches Axum's `{*path}` greedy capture
/// after the server's path extractor decodes it.
fn encode_path_segments(path: &str) -> String {
    path.split('/')
        .map(|seg| urlencoding::encode(seg).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Normalize and validate a knowledge-store path using the same component
/// rules as local AK path resolution.
///
/// Rejected components:
/// - `..` parent traversal
/// - absolute/rooted paths
/// - platform prefixes (e.g. `C:` on Windows)
///
/// Accepted and normalized:
/// - `.` components are removed
/// - repeated separators collapse via component iteration
fn normalize_knowledge_path(path: &str) -> Result<String, KnowledgeApiError> {
    if path.is_empty() {
        return Ok(String::new());
    }

    let mut relative = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(KnowledgeApiError::BadRequest {
                    message: format!("invalid store path: {path}"),
                });
            }
        }
    }

    Ok(relative.to_string_lossy().into_owned())
}

impl StakpakApiClient {
    /// Resolve the account name used as the cache root.
    /// Successful resolutions are memoized for the lifetime of the client.
    /// Failures are negatively cached for [`ACCOUNT_RESOLVE_BACKOFF`] so a
    /// transient `/v1/account` outage doesn't turn every cached read into
    /// two round-trips.
    ///
    /// Returns `None` when resolution is currently unavailable. Callers
    /// should treat this as "cache disabled" and proceed.
    async fn resolve_cache_account(&self) -> Option<String> {
        // Fast path under the lock: reuse a prior result if we have one.
        {
            let state = self.account_name.lock().await;
            match &*state {
                AccountCacheState::Resolved(name) => return Some(name.clone()),
                AccountCacheState::Failed { until } if Instant::now() < *until => return None,
                _ => {}
            }
        }

        let url = format!("{}/v1/account", self.base_url);
        let resolved: Option<String> = match self.client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                match response.json::<GetMyAccountResponse>().await {
                    Ok(account) => Some(match account.scope {
                        Some(scope) => scope.name,
                        None => account.username,
                    }),
                    Err(e) => {
                        debug!("knowledge cache: failed to parse account: {}", e);
                        None
                    }
                }
            }
            Ok(response) => {
                debug!(
                    "knowledge cache: /v1/account returned {}",
                    response.status()
                );
                None
            }
            Err(e) => {
                debug!("knowledge cache: failed to fetch account: {}", e);
                None
            }
        };

        let mut state = self.account_name.lock().await;
        match resolved {
            Some(name) => {
                // Another task may have raced us to a Resolved value; either
                // is fine since both came from the same endpoint. Last writer
                // wins.
                *state = AccountCacheState::Resolved(name.clone());
                Some(name)
            }
            None => {
                // Don't clobber a Resolved value that arrived while we were
                // failing in parallel.
                if !matches!(&*state, AccountCacheState::Resolved(_)) {
                    *state = AccountCacheState::Failed {
                        until: Instant::now() + ACCOUNT_RESOLVE_BACKOFF,
                    };
                }
                None
            }
        }
    }

    /// Read a knowledge file. Uses the on-disk cache at
    /// `~/.stakpak/remote-knowledge/<account>/<path>` together with the
    /// server's `If-None-Match` support to avoid re-downloading unchanged
    /// content.
    pub async fn read_knowledge_file(&self, path: &str) -> Result<Vec<u8>, KnowledgeApiError> {
        self.read_knowledge_file_inner(path, false).await
    }

    /// Read at most the first `max_bytes` of a knowledge file. The server
    /// supports a `peek` query parameter that returns a compact preview; if
    /// the response exceeds `max_bytes` we truncate client-side.
    ///
    /// Peek requests bypass the on-disk cache because they share the path
    /// with full reads but return a different body.
    pub async fn peek_knowledge_file(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, KnowledgeApiError> {
        let mut bytes = self.read_knowledge_file_inner(path, true).await?;
        if bytes.len() > max_bytes {
            bytes.truncate(max_bytes);
        }
        Ok(bytes)
    }

    async fn read_knowledge_file_inner(
        &self,
        path: &str,
        peek_only: bool,
    ) -> Result<Vec<u8>, KnowledgeApiError> {
        let normalized_path = normalize_knowledge_path(path)?;
        let encoded_path = encode_path_segments(&normalized_path);
        let url = format!("{}/v1/knowledge/{}", self.base_url, encoded_path);

        // Cache is only consulted for full reads. Peek bodies are different
        // content for the same path - mixing them would corrupt the cache.
        let cache_target: Option<PathBuf> = if peek_only {
            None
        } else {
            self.resolve_cache_account()
                .await
                .and_then(|account| cache::cached_path(&account, &normalized_path))
        };

        let cached = match &cache_target {
            Some(p) => cache::read_cached(p).await,
            None => None,
        };

        let mut request = self.client.get(&url);
        if peek_only {
            request = request.query(&[("peek", "true")]);
        }
        if let Some((_, etag)) = &cached {
            request = request.header(header::IF_NONE_MATCH, etag.as_str());
        }
        let response = request.send().await?;

        match response.status() {
            StatusCode::NOT_MODIFIED => match cached {
                Some((bytes, _)) => Ok(bytes),
                // We only set `If-None-Match` when we had a cached entry, so
                // a 304 here means a proxy or middlebox injected the header.
                // Surface a transport error rather than panicking; callers
                // will retry without the cache on the next request.
                None => Err(KnowledgeApiError::Transport {
                    message: "received 304 Not Modified without sending If-None-Match".into(),
                }),
            },
            status if status.is_success() => {
                let bytes = response.bytes().await?.to_vec();
                if let Some(target) = cache_target.as_ref() {
                    // Best-effort cache write; never fails the request.
                    cache::write_cached_atomic(target, &bytes).await;
                }
                Ok(bytes)
            }
            StatusCode::NOT_FOUND => {
                // Server says it's gone - evict any stale local copy so we
                // don't keep sending stale ETags for it.
                if let Some(target) = cache_target.as_ref() {
                    cache::evict_cached(target).await;
                }
                Err(Self::knowledge_error_from_response(response).await)
            }
            _ => Err(Self::knowledge_error_from_response(response).await),
        }
    }

    /// Cheap existence check using HTTP HEAD. Does not transfer the body.
    pub async fn knowledge_file_exists(&self, path: &str) -> Result<bool, KnowledgeApiError> {
        let normalized_path = normalize_knowledge_path(path)?;
        let encoded_path = encode_path_segments(&normalized_path);
        let url = format!("{}/v1/knowledge/{}", self.base_url, encoded_path);
        let response = self.client.head(&url).send().await?;

        let status = response.status();
        if status.is_success() {
            Ok(true)
        } else if status == StatusCode::NOT_FOUND {
            Ok(false)
        } else {
            Err(Self::knowledge_error_from_response(response).await)
        }
    }

    /// List knowledge files with optional filtering.
    /// Bypasses the on-disk cache (no ETag mechanism for list responses).
    pub async fn list_knowledge_files(
        &self,
        query: &ListKnowledgeFilesQuery,
    ) -> Result<ListKnowledgeFilesResponse, KnowledgeApiError> {
        let normalized_path = query
            .path
            .as_deref()
            .map(normalize_knowledge_path)
            .transpose()?;
        let normalized_query = ListKnowledgeFilesQuery {
            path: normalized_path,
            glob: query.glob.clone(),
        };

        let url = format!("{}/v1/knowledge", self.base_url);
        let response = self
            .client
            .get(&url)
            .query(&normalized_query)
            .send()
            .await?;
        self.handle_knowledge_response(response).await
    }

    /// Create a new knowledge file. Returns `Conflict` if a file already
    /// exists at the target path.
    ///
    /// The cache is not populated here: the local cache only holds bodies
    /// that came from a `GET /v1/knowledge/...` so we know the cached SHA
    /// matches the server's ETag. The next read will populate it.
    pub async fn create_knowledge_file(
        &self,
        path: &str,
        content: &[u8],
    ) -> Result<CreateKnowledgeFileResponse, KnowledgeApiError> {
        let normalized_path = normalize_knowledge_path(path)?;
        let encoded_path = encode_path_segments(&normalized_path);
        let url = format!("{}/v1/knowledge/{}", self.base_url, encoded_path);
        let response = self
            .client
            .post(&url)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(content.to_vec())
            .send()
            .await?;
        self.handle_knowledge_response(response).await
    }

    /// Overwrite an existing knowledge file (or create if not exists).
    ///
    /// The cache is not populated here. Any stale local copy will be
    /// revalidated on the next read: `If-None-Match` will miss against the
    /// new server ETag and the client will refetch + replace the cached
    /// body.
    pub async fn overwrite_knowledge_file(
        &self,
        path: &str,
        content: &[u8],
    ) -> Result<UpdateKnowledgeFileResponse, KnowledgeApiError> {
        let normalized_path = normalize_knowledge_path(path)?;
        let encoded_path = encode_path_segments(&normalized_path);
        let url = format!("{}/v1/knowledge/{}", self.base_url, encoded_path);
        let response = self
            .client
            .put(&url)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(content.to_vec())
            .send()
            .await?;
        self.handle_knowledge_response(response).await
    }

    /// Delete a knowledge file or directory. On success, evicts the matching
    /// cache entry (file or directory tree).
    pub async fn delete_knowledge_file(&self, path: &str) -> Result<(), KnowledgeApiError> {
        let normalized_path = normalize_knowledge_path(path)?;
        let encoded_path = encode_path_segments(&normalized_path);
        let url = format!("{}/v1/knowledge/{}", self.base_url, encoded_path);
        let response = self.client.delete(&url).send().await?;

        if !response.status().is_success() {
            return Err(Self::knowledge_error_from_response(response).await);
        }

        if let Some(account) = self.resolve_cache_account().await
            && let Some(target) = cache::cached_path(&account, &normalized_path)
        {
            cache::evict_cached(&target).await;
        }

        Ok(())
    }

    /// Decode a JSON body on success; otherwise convert the response into a
    /// typed [`KnowledgeApiError`].
    async fn handle_knowledge_response<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> Result<T, KnowledgeApiError> {
        if !response.status().is_success() {
            return Err(Self::knowledge_error_from_response(response).await);
        }
        let url = response.url().to_string();
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| KnowledgeApiError::Transport {
                message: format!(
                    "Failed to read response body from {} (status {}): {}",
                    url, status, e
                ),
            })?;
        serde_json::from_str(&body).map_err(|e| {
            let truncated_body: String = body.chars().take(500).collect();
            KnowledgeApiError::Transport {
                message: format!(
                    "Failed to decode response from {} (status {}): {} | body: {}",
                    url, status, e, truncated_body
                ),
            }
        })
    }

    /// Map a non-success HTTP response into a [`KnowledgeApiError`], using
    /// the structured `ApiError` payload when present so we can surface the
    /// server-provided message verbatim.
    async fn knowledge_error_from_response(response: Response) -> KnowledgeApiError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        let message = serde_json::from_str::<ApiError>(&body)
            .map(|api| api.error.message)
            .unwrap_or_else(|_| {
                if body.is_empty() {
                    status.canonical_reason().unwrap_or("error").to_string()
                } else {
                    body.clone()
                }
            });

        match status {
            StatusCode::NOT_FOUND => KnowledgeApiError::NotFound { message },
            StatusCode::CONFLICT => KnowledgeApiError::Conflict { message },
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                KnowledgeApiError::Forbidden { message }
            }
            StatusCode::BAD_REQUEST => KnowledgeApiError::BadRequest { message },
            other => KnowledgeApiError::Http {
                status: other,
                message,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KnowledgeApiError, encode_path_segments, normalize_knowledge_path};

    #[test]
    fn normalize_path_rejects_parent_components() {
        let err = normalize_knowledge_path("docs/../secrets.txt").unwrap_err();
        assert!(matches!(err, KnowledgeApiError::BadRequest { .. }));
    }

    #[test]
    fn normalize_path_rejects_absolute_paths() {
        let err = normalize_knowledge_path("/etc/passwd").unwrap_err();
        assert!(matches!(err, KnowledgeApiError::BadRequest { .. }));
    }

    #[test]
    fn normalize_path_removes_dot_and_empty_segments() {
        let normalized = normalize_knowledge_path("docs//./guides///intro.md").unwrap();
        assert_eq!(normalized, "docs/guides/intro.md");
    }

    #[test]
    fn encode_keeps_separators_and_encodes_each_segment() {
        let encoded = encode_path_segments("team notes/2026 plan.md");
        assert_eq!(encoded, "team%20notes/2026%20plan.md");
    }
}
