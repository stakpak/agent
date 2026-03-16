//! GitHub Copilot provider implementation
//!
//! ## Authentication flow
//!
//! 1. The user authenticates via GitHub Device Flow → receives a GitHub OAuth
//!    token (`ghu_...`).
//! 2. Before each API call the provider exchanges that token for a short-lived
//!    Copilot API token via:
//!    `GET https://api.github.com/copilot_internal/v2/token`
//!    (Authorization: token <github-oauth-token>)
//! 3. The response contains:
//!    - `token`: short-lived Copilot API token (sent as Bearer on chat requests)
//!    - `expires_at`: Unix timestamp (integer seconds)
//!    - `endpoints.api`: resolved chat API base URL (e.g. `https://api.individual.githubcopilot.com`)
//! 4. The Copilot API token is cached in memory and refreshed 5 min before expiry.
//!
//! ## Required headers on chat requests
//!
//! - `Authorization: Bearer <copilot-api-token>`
//! - `Editor-Version: vscode/1.95.0`  (override via `STAKPAK_COPILOT_EDITOR_VERSION`)
//! - `Editor-Plugin-Version: copilot-chat/0.22.4`  (override via `STAKPAK_COPILOT_EDITOR_PLUGIN_VERSION`)
//! - `Openai-Intent: conversation-edits`
//! - `x-initiator: agent`
//! - `User-Agent: stakpak/<version>`

use super::types::{CachedCopilotToken, CopilotConfig};
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::providers::openai::convert::{from_openai_response, to_openai_request};
use crate::providers::openai::stream::create_completions_stream;
use crate::providers::openai::types::ChatCompletionResponse;
use crate::providers::tls::create_platform_tls_client;
use crate::types::{GenerateRequest, GenerateResponse, GenerateStream, Headers, Model};
use async_trait::async_trait;
use reqwest::Client;
use reqwest_eventsource::EventSource;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

#[derive(serde::Deserialize)]
struct Endpoints {
    api: String,
}

#[derive(serde::Deserialize)]
struct TokenResp {
    token: String,
    expires_at: u64,
    endpoints: Option<Endpoints>,
}

/// Refresh the cached token this many seconds before it actually expires.
const REFRESH_BUFFER_SECS: u64 = 300;

/// Hardcoded VSCode version string required by the Copilot backend.
/// Update this when GitHub changes its accepted version range.
const EDITOR_VERSION: &str = "vscode/1.95.0";

/// Hardcoded Copilot Chat plugin version required by the Copilot backend.
/// Update this when GitHub changes its accepted version range.
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.22.4";

/// GitHub Copilot provider.
pub struct CopilotProvider {
    config: CopilotConfig,
    client: Client,
    /// In-memory cache of the short-lived Copilot API token.
    cached_token: Mutex<Option<CachedCopilotToken>>,
}

impl CopilotProvider {
    /// Create a new Copilot provider from a config.
    pub fn new(config: CopilotConfig) -> Result<Self> {
        if config.github_token.is_empty() {
            return Err(Error::MissingApiKey("github-copilot".to_string()));
        }
        let client = create_platform_tls_client()?;
        Ok(Self {
            config,
            client,
            cached_token: Mutex::new(None),
        })
    }

    /// Fetch a fresh short-lived Copilot API token from GitHub.
    async fn fetch_token(&self) -> Result<CachedCopilotToken> {
        let resp = self
            .client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header(
                "Authorization",
                format!("token {}", self.config.github_token),
            )
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                format!("stakpak/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "Failed to obtain Copilot API token (HTTP {}): {}",
                status, body
            )));
        }

        let parsed: TokenResp = resp.json().await.map_err(|e| {
            Error::provider_error(format!("Failed to parse Copilot token response: {}", e))
        })?;

        let api_base = parsed
            .endpoints
            .map(|e| e.api.trim_end_matches('/').to_string())
            .unwrap_or_else(|| {
                self.config
                    .base_url_override
                    .clone()
                    .unwrap_or_else(|| CopilotConfig::FALLBACK_BASE_URL.to_string())
            });

        Ok(CachedCopilotToken {
            token: parsed.token,
            expires_at: parsed.expires_at,
            api_base,
        })
    }

    /// Return a valid cached token, refreshing if expired or near expiry.
    async fn get_token(&self) -> Result<CachedCopilotToken> {
        let mut guard = self.cached_token.lock().await;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::provider_error(format!("system time error: {e}")))?
            .as_secs();

        let needs_refresh = match guard.as_ref() {
            None => true,
            Some(t) => now + REFRESH_BUFFER_SECS >= t.expires_at,
        };
        if needs_refresh {
            *guard = Some(self.fetch_token().await?);
        }
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::provider_error("Copilot token unavailable after refresh"))
    }

    /// Build the full set of headers required by the Copilot chat API.
    ///
    /// Returns `(headers, api_base)` so callers can obtain both from a single
    /// token acquisition without a second `get_token()` call.
    async fn build_headers_async(
        &self,
        custom_headers: Option<&Headers>,
    ) -> Result<(Headers, String)> {
        let token = self.get_token().await?;

        let mut headers = Headers::new();
        headers.insert("Authorization", format!("Bearer {}", token.token));
        headers.insert("Content-Type", "application/json");
        headers.insert(
            "User-Agent",
            format!("stakpak/{}", env!("CARGO_PKG_VERSION")),
        );
        // Required by the Copilot backend — must be a recognised VSCode version.
        // Both values can be overridden at runtime via environment variables.
        let editor_version = std::env::var("STAKPAK_COPILOT_EDITOR_VERSION")
            .unwrap_or_else(|_| EDITOR_VERSION.to_string());
        let editor_plugin_version = std::env::var("STAKPAK_COPILOT_EDITOR_PLUGIN_VERSION")
            .unwrap_or_else(|_| EDITOR_PLUGIN_VERSION.to_string());
        headers.insert("Editor-Version", editor_version);
        headers.insert("Editor-Plugin-Version", editor_plugin_version);
        headers.insert("Openai-Intent", "conversation-edits");
        headers.insert("x-initiator", "agent");

        if let Some(custom) = custom_headers {
            headers.merge_with(custom);
        }

        Ok((headers, token.api_base))
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn provider_id(&self) -> &str {
        "github-copilot"
    }

    fn build_headers(&self, _custom_headers: Option<&Headers>) -> Headers {
        panic!(
            "CopilotProvider::build_headers is not supported; \
             use build_headers_async instead — Copilot token exchange is async"
        )
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse> {
        let (headers, api_base) = self
            .build_headers_async(request.options.headers.as_ref())
            .await?;

        let url = format!("{}/chat/completions", api_base);
        let openai_req = to_openai_request(&request, false);

        let response = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&openai_req)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::provider_error(format!(
                "GitHub Copilot API error {}: {}",
                status, error_text
            )));
        }

        let openai_resp: ChatCompletionResponse = response.json().await?;
        from_openai_response(openai_resp)
    }

    async fn stream(&self, request: GenerateRequest) -> Result<GenerateStream> {
        let (headers, api_base) = self
            .build_headers_async(request.options.headers.as_ref())
            .await?;

        let url = format!("{}/chat/completions", api_base);
        let openai_req = to_openai_request(&request, true);

        let req_builder = self
            .client
            .post(&url)
            .headers(headers.to_reqwest_headers())
            .json(&openai_req);

        let event_source = EventSource::new(req_builder)
            .map_err(|e| Error::stream_error(format!("Failed to create event source: {}", e)))?;

        create_completions_stream(event_source).await
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        crate::registry::models_dev::load_models_for_provider("github-copilot")
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let models = crate::registry::models_dev::load_models_for_provider("github-copilot")?;
        Ok(models.into_iter().find(|m| m.id == id))
    }
}
