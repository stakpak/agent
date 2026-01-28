//! StakpakApiClient implementation
//!
//! Provides access to Stakpak's non-inference APIs.

use super::{
    CheckpointState, CreateCheckpointRequest, CreateCheckpointResponse, CreateSessionRequest,
    CreateSessionResponse, GetCheckpointResponse, GetSessionResponse, ListCheckpointsQuery,
    ListCheckpointsResponse, ListSessionsQuery, ListSessionsResponse, SessionVisibility,
    StakpakApiConfig, UpdateSessionRequest, UpdateSessionResponse, models::*,
};
use crate::models::{
    CreateRuleBookInput, CreateRuleBookResponse, GetMyAccountResponse, ListRuleBook,
    ListRulebooksResponse, RuleBook,
};
use reqwest::{Response, header};
use rmcp::model::Content;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use stakpak_shared::models::billing::BillingResponse;
use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
use uuid::Uuid;

/// Client for Stakpak's non-inference APIs
#[derive(Clone, Debug)]
pub struct StakpakApiClient {
    client: reqwest::Client,
    base_url: String,
}

/// API error response format
#[derive(Debug, serde::Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Debug, serde::Deserialize)]
struct ApiErrorDetail {
    key: String,
    message: String,
}

impl StakpakApiClient {
    /// Create a new StakpakApiClient
    pub fn new(config: &StakpakApiConfig) -> Result<Self, String> {
        if config.api_key.is_empty() {
            return Err("Stakpak API key is required".to_string());
        }

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", config.api_key))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&format!("Stakpak/{}", env!("CARGO_PKG_VERSION")))
                .map_err(|e| e.to_string())?,
        );

        let client = create_tls_client(
            TlsClientConfig::default()
                .with_headers(headers)
                .with_timeout(std::time::Duration::from_secs(300)),
        )?;

        Ok(Self {
            client,
            base_url: config.api_endpoint.clone(),
        })
    }

    // =========================================================================
    // Session APIs - New /v1/sessions endpoints
    // =========================================================================

    /// Create a new session
    pub async fn create_session(
        &self,
        req: &CreateSessionRequest,
    ) -> Result<CreateSessionResponse, String> {
        let url = format!("{}/v1/sessions", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(req)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Create a checkpoint for a session
    pub async fn create_checkpoint(
        &self,
        session_id: Uuid,
        req: &CreateCheckpointRequest,
    ) -> Result<CreateCheckpointResponse, String> {
        let url = format!("{}/v1/sessions/{}/checkpoints", self.base_url, session_id);
        let response = self
            .client
            .post(&url)
            .json(req)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// List sessions
    pub async fn list_sessions(
        &self,
        query: &ListSessionsQuery,
    ) -> Result<ListSessionsResponse, String> {
        let url = format!("{}/v1/sessions", self.base_url);
        let response = self
            .client
            .get(&url)
            .query(query)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: Uuid) -> Result<GetSessionResponse, String> {
        let url = format!("{}/v1/sessions/{}", self.base_url, id);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Update a session
    pub async fn update_session(
        &self,
        id: Uuid,
        req: &UpdateSessionRequest,
    ) -> Result<UpdateSessionResponse, String> {
        let url = format!("{}/v1/sessions/{}", self.base_url, id);
        let response = self
            .client
            .patch(&url)
            .json(req)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Delete a session
    pub async fn delete_session(&self, id: Uuid) -> Result<(), String> {
        let url = format!("{}/v1/sessions/{}", self.base_url, id);
        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response_no_body(response).await
    }

    /// List checkpoints for a session
    pub async fn list_checkpoints(
        &self,
        session_id: Uuid,
        query: &ListCheckpointsQuery,
    ) -> Result<ListCheckpointsResponse, String> {
        let url = format!("{}/v1/sessions/{}/checkpoints", self.base_url, session_id);
        let response = self
            .client
            .get(&url)
            .query(query)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Get a checkpoint by ID
    pub async fn get_checkpoint(&self, id: Uuid) -> Result<GetCheckpointResponse, String> {
        let url = format!("{}/v1/sessions/checkpoints/{}", self.base_url, id);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    // =========================================================================
    // Cancel API
    // =========================================================================

    /// Cancel an active inference request
    pub async fn cancel_request(&self, request_id: &str) -> Result<(), String> {
        let url = format!("{}/v1/chat/requests/{}/cancel", self.base_url, request_id);
        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response_no_body(response).await
    }

    // =========================================================================
    // Account APIs
    // =========================================================================

    /// Get the current user's account info
    pub async fn get_account(&self) -> Result<GetMyAccountResponse, String> {
        let url = format!("{}/v1/account", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Get billing info for a user
    pub async fn get_billing(&self, username: &str) -> Result<BillingResponse, String> {
        let url = format!("{}/v2/{}/billing", self.base_url, username);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    // =========================================================================
    // Rulebook APIs
    // =========================================================================

    /// List all rulebooks
    pub async fn list_rulebooks(&self) -> Result<Vec<ListRuleBook>, String> {
        let url = format!("{}/v1/rules", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let response = self.handle_response_error(response).await?;
        let value: Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<ListRulebooksResponse>(value) {
            Ok(response) => Ok(response.results),
            Err(e) => Err(format!("Failed to deserialize rulebooks response: {}", e)),
        }
    }

    /// Get a rulebook by URI
    pub async fn get_rulebook_by_uri(&self, uri: &str) -> Result<RuleBook, String> {
        let encoded_uri = urlencoding::encode(uri);
        let url = format!("{}/v1/rules/{}", self.base_url, encoded_uri);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Create a new rulebook
    pub async fn create_rulebook(
        &self,
        input: &CreateRuleBookInput,
    ) -> Result<CreateRuleBookResponse, String> {
        let url = format!("{}/v1/rules", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(input)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response(response).await
    }

    /// Delete a rulebook
    pub async fn delete_rulebook(&self, uri: &str) -> Result<(), String> {
        let encoded_uri = urlencoding::encode(uri);
        let url = format!("{}/v1/rules/{}", self.base_url, encoded_uri);
        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response_no_body(response).await
    }

    // =========================================================================
    // MCP Tool APIs
    // =========================================================================

    /// Search documentation
    pub async fn search_docs(&self, req: &SearchDocsRequest) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "search_docs".to_string(),
            arguments: serde_json::to_value(req).map_err(|e| e.to_string())?,
        })
        .await
    }

    /// Search memory
    pub async fn search_memory(&self, req: &SearchMemoryRequest) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "search_memory".to_string(),
            arguments: serde_json::to_value(req).map_err(|e| e.to_string())?,
        })
        .await
    }

    /// Memorize a session checkpoint (extract memory)
    pub async fn memorize_session(&self, checkpoint_id: Uuid) -> Result<(), String> {
        let url = format!(
            "{}/v1/agents/sessions/checkpoints/{}/extract-memory",
            self.base_url, checkpoint_id
        );
        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        self.handle_response_no_body(response).await
    }

    /// Read Slack messages from a channel
    pub async fn slack_read_messages(
        &self,
        req: &SlackReadMessagesRequest,
    ) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "slack_read_messages".to_string(),
            arguments: serde_json::to_value(req).map_err(|e| e.to_string())?,
        })
        .await
    }

    /// Read Slack thread replies
    pub async fn slack_read_replies(
        &self,
        req: &SlackReadRepliesRequest,
    ) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "slack_read_replies".to_string(),
            arguments: serde_json::to_value(req).map_err(|e| e.to_string())?,
        })
        .await
    }

    /// Send a Slack message
    pub async fn slack_send_message(
        &self,
        req: &SlackSendMessageRequest,
    ) -> Result<Vec<Content>, String> {
        self.call_mcp_tool(&ToolsCallParams {
            name: "slack_send_message".to_string(),
            arguments: serde_json::to_value(req).map_err(|e| e.to_string())?,
        })
        .await
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// Call an MCP tool via JSON-RPC
    async fn call_mcp_tool(&self, params: &ToolsCallParams) -> Result<Vec<Content>, String> {
        let url = format!("{}/v1/mcp", self.base_url);
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": params
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let resp: Value = self.handle_response(response).await?;

        // Extract result.content from JSON-RPC response
        if let Some(result) = resp.get("result")
            && let Some(content) = result.get("content")
        {
            let content: Vec<Content> =
                serde_json::from_value(content.clone()).map_err(|e| e.to_string())?;
            return Ok(content);
        }

        // Check for error
        if let Some(error) = resp.get("error") {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(msg.to_string());
        }

        Err("Invalid MCP response format".to_string())
    }

    /// Handle response and parse JSON
    async fn handle_response<T: DeserializeOwned>(&self, response: Response) -> Result<T, String> {
        let response = self.handle_response_error(response).await?;
        response.json().await.map_err(|e| e.to_string())
    }

    /// Handle response without body
    async fn handle_response_no_body(&self, response: Response) -> Result<(), String> {
        self.handle_response_error(response).await?;
        Ok(())
    }

    /// Handle response errors
    async fn handle_response_error(&self, response: Response) -> Result<Response, String> {
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();

        // Try to parse as API error
        if let Ok(api_error) = serde_json::from_str::<ApiError>(&error_body) {
            // Special handling for API limit exceeded
            if api_error.error.key == "EXCEEDED_API_LIMIT" {
                return Err(format!(
                    "{}. You can top up your billing at https://stakpak.dev/settings/billing",
                    api_error.error.message
                ));
            }
            return Err(api_error.error.message);
        }

        Err(format!("API error {}: {}", status, error_body))
    }
}

// =============================================================================
// Builder helpers for creating sessions and checkpoints
// =============================================================================

impl CreateSessionRequest {
    /// Create a new session request with initial state
    pub fn new(title: impl Into<String>, state: CheckpointState) -> Self {
        Self {
            title: title.into(),
            visibility: Some(SessionVisibility::Private),
            cwd: None,
            state,
        }
    }

    /// Set the working directory
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set visibility
    pub fn with_visibility(mut self, visibility: SessionVisibility) -> Self {
        self.visibility = Some(visibility);
        self
    }
}

impl CreateCheckpointRequest {
    /// Create a new checkpoint request
    pub fn new(state: CheckpointState) -> Self {
        Self {
            state,
            parent_id: None,
        }
    }

    /// Set the parent checkpoint ID (for branching)
    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent_id = Some(parent_id);
        self
    }
}
