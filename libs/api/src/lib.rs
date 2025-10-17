use chrono::{DateTime, Utc};
use eventsource_stream::Eventsource;
use reqwest::header::HeaderMap;
use reqwest::{Client as ReqwestClient, Error as ReqwestError, Response, header};
use rmcp::model::Content;
use rmcp::model::JsonRpcResponse;
use serde::{Deserialize, Serialize};
use stakpak_shared::tls_client::TlsClientConfig;
use stakpak_shared::tls_client::create_tls_client;
pub mod models;
use futures_util::Stream;
use futures_util::StreamExt;
use models::*;
use serde_json::Value;
use serde_json::json;
use stakpak_shared::models::integrations::openai::{
    ChatCompletionRequest, ChatCompletionResponse, ChatCompletionStreamResponse, ChatMessage, Tool,
};
use uuid::Uuid;
pub mod dave_v1;
pub mod kevin_v1;
pub mod norbert_v1;
pub mod stuart_v1;
pub use models::Block;

#[derive(Clone, Debug)]

pub struct Client {
    client: ReqwestClient,
    base_url: String,
}

#[derive(Clone, Debug)]

pub struct ClientConfig {
    pub api_key: Option<String>,
    pub api_endpoint: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    key: String,
    message: String,
}

impl Client {
    async fn handle_response_error(&self, response: Response) -> Result<Response, String> {
        if response.status().is_success() {
            Ok(response)
        } else {
            match response.json::<ApiError>().await {
                Ok(response) => {
                    if response.error.key == "EXCEEDED_API_LIMIT" {
                        Err(format!(
                            "{}.\n\nPlease top up your account at https://stakpak.dev/settings/billing to keep Stakpaking.",
                            response.error.message
                        ))
                    } else {
                        Err(response.error.message)
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }
    }

    pub fn new(config: &ClientConfig) -> Result<Self, String> {
        if config.api_key.is_none() {
            return Err("API Key not found, please login".into());
        }

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", config.api_key.clone().unwrap()))
                .expect("Invalid API key format"),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&format!("Stakpak/{}", env!("CARGO_PKG_VERSION")))
                .expect("Invalid user agent format"),
        );

        let client = create_tls_client(
            TlsClientConfig::default()
                .with_headers(headers)
                .with_timeout(std::time::Duration::from_secs(300)),
        )?;

        Ok(Self {
            client,
            base_url: config.api_endpoint.clone() + "/v1",
        })
    }

    pub async fn get_my_account(&self) -> Result<GetMyAccountResponse, String> {
        let url = format!("{}/account", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<GetMyAccountResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn list_rulebooks(&self) -> Result<Vec<ListRuleBook>, String> {
        let url = format!("{}/rules", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<ListRulebooksResponse>(value.clone()) {
            Ok(response) => Ok(response.results),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_rulebook_by_uri(&self, uri: &str) -> Result<RuleBook, String> {
        // URL encode the URI to handle special characters
        let encoded_uri = urlencoding::encode(uri);
        let url = format!("{}/rules/{}", self.base_url, encoded_uri);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<RuleBook>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn create_rulebook(
        &self,
        uri: &str,
        description: &str,
        content: &str,
        tags: Vec<String>,
        visibility: Option<RuleBookVisibility>,
    ) -> Result<CreateRuleBookResponse, String> {
        let url = format!("{}/rules", self.base_url);

        let input = CreateRuleBookInput {
            uri: uri.to_string(),
            description: description.to_string(),
            content: content.to_string(),
            tags,
            visibility,
        };

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        // Check status before consuming body
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("API error ({}): {}", status, error_text));
        }

        // Get response as text first to handle non-JSON responses
        let response_text = response.text().await.map_err(|e| e.to_string())?;
        
        // Try to parse as JSON first
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&response_text) {
            match serde_json::from_value::<CreateRuleBookResponse>(value.clone()) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    eprintln!("Failed to deserialize JSON response: {}", e);
                    eprintln!("Raw response: {}", value);
                }
            }
        }
        
        // If JSON parsing failed, try to parse as plain text "id: <uuid>"
        if response_text.starts_with("id: ") {
            let id = response_text.trim_start_matches("id: ").trim().to_string();
            return Ok(CreateRuleBookResponse { id });
        }
        
        Err(format!("Unexpected response format: {}", response_text))
    }

    pub async fn delete_rulebook(&self, uri: &str) -> Result<(), String> {
        let encoded_uri = urlencoding::encode(uri);
        let url = format!("{}/rules/{}", self.base_url, encoded_uri);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let _response = self.handle_response_error(response).await?;

        Ok(())
    }

    pub async fn list_flows(&self, owner_name: &str) -> Result<GetFlowsResponse, String> {
        let url = format!("{}/flows/{}", self.base_url, owner_name);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<GetFlowsResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_flow(
        &self,
        owner_name: &str,
        flow_name: &str,
    ) -> Result<GetFlowResponse, String> {
        let url = format!("{}/flows/{}/{}", self.base_url, owner_name, flow_name);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<GetFlowResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn create_flow(
        &self,
        flow_name: &str,
        visibility: Option<FlowVisibility>,
    ) -> Result<CreateFlowResponse, String> {
        let url = format!("{}/flows", self.base_url);

        let input = CreateFlowInput {
            name: flow_name.to_string(),
            visibility,
        };

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<CreateFlowResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn save_edits(
        &self,
        flow_ref: &FlowRef,
        edits: Vec<Edit>,
    ) -> Result<SaveEditsResponse, String> {
        let url = format!("{}/flows/{}/save", self.base_url, flow_ref);

        let input = SaveEditsInput { edits };

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<SaveEditsResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_flow_documents(
        &self,
        flow_ref: &FlowRef,
    ) -> Result<GetFlowDocumentsResponse, String> {
        let url = format!("{}/flows/{}/documents", self.base_url, flow_ref);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<GetFlowDocumentsResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn query_blocks(
        &self,
        query: &str,
        generate_query: bool,
        synthesize_output: bool,
        flow_ref: Option<&str>,
    ) -> Result<QueryBlocksResponse, String> {
        let url = format!("{}/commands/query", self.base_url);

        let flow_ref = if let Some(flow_ref) = flow_ref {
            let flow_ref: FlowRef = FlowRef::new(flow_ref.to_string())?;
            Some(flow_ref)
        } else {
            None
        };

        let input = QueryCommandInput {
            query: query.to_string(),
            generate_query,
            synthesize_output,
            flow_ref,
        };

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<QueryBlocksResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn list_agent_sessions(&self) -> Result<Vec<AgentSession>, String> {
        let url = format!("{}/agents/sessions", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<Vec<AgentSession>>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_session(&self, session_id: Uuid) -> Result<AgentSession, String> {
        let url = format!("{}/agents/sessions/{}", self.base_url, session_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<AgentSession>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_session_stats(
        &self,
        session_id: Uuid,
    ) -> Result<AgentSessionStats, String> {
        let url = format!("{}/agents/sessions/{}/stats", self.base_url, session_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<AgentSessionStats>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn create_agent_session(
        &self,
        agent_id: AgentID,
        visibility: AgentSessionVisibility,
        input: Option<AgentInput>,
    ) -> Result<AgentSession, String> {
        let url = format!("{}/agents/sessions", self.base_url);

        let input = serde_json::json!({
            "agent_id": agent_id,
            "visibility": visibility,
            "input": input,
        });

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<AgentSession>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn run_agent(&self, input: &RunAgentInput) -> Result<RunAgentOutput, String> {
        let url = format!("{}/agents/run", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<RunAgentOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_checkpoint(
        &self,
        checkpoint_id: Uuid,
    ) -> Result<RunAgentOutput, String> {
        let url = format!("{}/agents/checkpoints/{}", self.base_url, checkpoint_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<RunAgentOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_session_latest_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<RunAgentOutput, String> {
        let url = format!(
            "{}/agents/sessions/{}/checkpoints/latest",
            self.base_url, session_id
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<RunAgentOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn transpile(
        &self,
        content: Vec<Document>,
        source_provisioner: ProvisionerType,
        target_provisioner: TranspileTargetProvisionerType,
    ) -> Result<TranspileOutput, String> {
        let url = format!(
            "{}/commands/{}/transpile",
            self.base_url,
            serde_json::to_value(&source_provisioner)
                .unwrap()
                .as_str()
                .unwrap()
        );

        let input = TranspileInput {
            content,
            output: target_provisioner.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<TranspileOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn get_agent_tasks(
        &self,
        provisioner: &ProvisionerType,
        dir: Option<String>,
    ) -> Result<Vec<AgentTask>, String> {
        let url = format!(
            "{}/agents/tasks?provisioner={}{}",
            self.base_url,
            serde_json::to_value(provisioner).unwrap().as_str().unwrap(),
            dir.map(|d| format!("&dir={}", d)).unwrap_or_default(),
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<AgentTaskOutput>(value.clone()) {
            Ok(response) => Ok(response.results),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ChatCompletionResponse, String> {
        let url = format!("{}/agents/openai/v1/chat/completions", self.base_url);

        let input = ChatCompletionRequest::new(messages, tools, None);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<ChatCompletionResponse>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn chat_completion_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
        headers: Option<HeaderMap>,
    ) -> Result<
        (
            impl Stream<Item = Result<ChatCompletionStreamResponse, ApiStreamError>>,
            Option<String>,
        ),
        String,
    > {
        let url = format!("{}/agents/openai/v1/chat/completions", self.base_url);

        let input = ChatCompletionRequest::new(messages, tools, Some(true));

        let response = self
            .client
            .post(&url)
            .headers(headers.unwrap_or_default())
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        // Extract x-request-id from headers
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let response = self.handle_response_error(response).await?;
        let stream = response.bytes_stream().eventsource().map(|event| {
            event
                .map_err(|_| ApiStreamError::Unknown("Failed to read response".to_string()))
                .and_then(|event| match event.event.as_str() {
                    "error" => Err(ApiStreamError::from(event.data)),
                    _ => serde_json::from_str::<ChatCompletionStreamResponse>(&event.data).map_err(
                        |_| {
                            ApiStreamError::Unknown(
                                "Failed to parse JSON from Anthropic response".to_string(),
                            )
                        },
                    ),
                })
        });

        Ok((stream, request_id))
    }

    pub async fn cancel_stream(&self, request_id: String) -> Result<(), String> {
        let url = format!("{}/agents/requests/{}/cancel", self.base_url, request_id);
        self.client
            .post(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        Ok(())
    }

    pub async fn generate_code(
        &self,
        input: &GenerateCodeInput,
    ) -> Result<GenerateCodeOutput, String> {
        let url = format!("{}/commands/{}/generate", self.base_url, input.provisioner);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<GenerateCodeOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn build_code_index(
        &self,
        input: &BuildCodeIndexInput,
    ) -> Result<BuildCodeIndexOutput, String> {
        let url = format!("{}/commands/build_code_index", self.base_url,);

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        match serde_json::from_value::<BuildCodeIndexOutput>(value.clone()) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn call_mcp_tool(&self, input: &ToolsCallParams) -> Result<Vec<Content>, String> {
        let url = format!("{}/mcp", self.base_url);

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": input.name,
                "arguments": input.arguments,
            },
            "id": Uuid::new_v4().to_string(),
        });

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let response = self.handle_response_error(response).await?;

        let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        match serde_json::from_value::<JsonRpcResponse<ToolsCallResponse>>(value.clone()) {
            Ok(response) => Ok(response.result.content),
            Err(e) => {
                eprintln!("Failed to deserialize response: {}", e);
                eprintln!("Raw response: {}", value);
                Err("Failed to deserialize response:".into())
            }
        }
    }

    pub async fn memorize_session(&self, checkpoint_id: Uuid) -> Result<(), String> {
        let url = format!(
            "{}/agents/sessions/checkpoints/{}/extract-memory",
            self.base_url, checkpoint_id
        );

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        let _ = self.handle_response_error(response).await?;
        Ok(())
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GetMyAccountResponse {
    pub username: String,
    pub id: String,
    pub first_name: String,
    pub last_name: String,
}

impl GetMyAccountResponse {
    pub fn to_text(&self) -> String {
        format!(
            "ID: {}\nUsername: {}\nName: {} {}",
            self.id, self.username, self.first_name, self.last_name
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum RuleBookVisibility {
    #[default]
    Public,
    Private,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListRuleBook {
    pub id: String,
    pub uri: String,
    pub description: String,
    pub visibility: RuleBookVisibility,
    pub tags: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ListRulebooksResponse {
    pub results: Vec<ListRuleBook>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuleBook {
    pub id: String,
    pub uri: String,
    pub description: String,
    pub content: String,
    pub visibility: RuleBookVisibility,
    pub tags: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateRuleBookInput {
    pub uri: String,
    pub description: String,
    pub content: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<RuleBookVisibility>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateRuleBookResponse {
    pub id: String,
}

impl ListRuleBook {
    pub fn to_text(&self) -> String {
        format!(
            "URI: {}\nDescription: {}\nTags: {}\n",
            self.uri,
            self.description,
            self.tags.join(", ")
        )
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct GetFlowsResponse {
    pub results: Vec<Flow>,
}

impl GetFlowsResponse {
    pub fn to_text(&self, owner_name: &str) -> String {
        let mut output = String::new();

        for flow in &self.results {
            let latest_version = flow
                .versions
                .iter()
                .max_by_key(|v| v.created_at)
                .unwrap_or_else(|| &flow.versions[0]);
            let tags = latest_version
                .tags
                .iter()
                .map(|t| t.name.clone())
                .collect::<Vec<_>>()
                .join(", ");

            output.push_str(&format!(
                "{} ({:7}) {:<10} {}/{}/{}\n",
                latest_version.created_at.format("\"%Y-%m-%d %H:%M UTC\""),
                format!("{:?}", flow.visibility),
                if tags.is_empty() { "-" } else { &tags },
                owner_name,
                flow.name,
                latest_version.id,
            ));
        }

        output
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetFlowResponse {
    pub permission: GetFlowPermission,
    pub resource: Flow,
}
impl GetFlowResponse {
    pub fn to_text(&self, owner_name: &str) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "Flow: {}/{} ({:?})\n\n",
            owner_name, self.resource.name, self.resource.visibility
        ));

        let mut versions = self.resource.versions.clone();
        versions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        for version in versions {
            let tags = version
                .tags
                .iter()
                .map(|t| t.name.clone())
                .collect::<Vec<_>>()
                .join(", ");

            output.push_str(&format!(
                "\"{:<20}\" {} {}/{}/{} \n",
                version.created_at.format("%Y-%m-%d %H:%M UTC"),
                if tags.is_empty() { "-" } else { &tags },
                owner_name,
                self.resource.name,
                version.id,
            ));
        }

        output
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct QueryCommandInput {
    query: String,
    #[serde(default)]
    generate_query: bool,
    #[serde(default)]
    synthesize_output: bool,
    #[serde(default)]
    flow_ref: Option<FlowRef>,
}

#[derive(Deserialize, Debug)]
pub struct QueryBlocksResponse {
    pub query_results: Vec<QueryBlockResult>,
    // not used
    // pub semantic_query: String,
    pub output: Option<String>,
}

impl QueryBlocksResponse {
    pub fn to_text(&self, output_only: bool) -> String {
        let mut output = String::new();

        if !output_only {
            for result in &self.query_results {
                output.push_str(&format!(
                    r#"
-------------------------------------------------------
Flow: {} ({})
Document: {}:{}:{}
Score: {:.2}%
-------------------------------------------------------
{}

            "#,
                    result.flow_version.flow_name,
                    result.flow_version.version_id,
                    result
                        .block
                        .document_uri
                        .strip_prefix("file://")
                        .unwrap_or(&result.block.document_uri),
                    result.block.start_point.row,
                    result.block.start_point.column,
                    result.similarity * 100.0,
                    result.block.code
                ));
            }

            // if !self.semantic_query.is_empty() {
            //     output.push_str(&format!("\nQuery: {}\n", self.semantic_query));
            // }
        }

        if let Some(synthesized_output) = &self.output {
            output.push_str(&format!("{}\n", synthesized_output));
        }

        output.trim_end().to_string()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateFlowResponse {
    pub flow_name: String,
    pub owner_name: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub version_id: Uuid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateFlowInput {
    pub name: String,
    pub visibility: Option<FlowVisibility>,
}

#[derive(Serialize)]
pub struct SaveEditsInput {
    pub edits: Vec<Edit>,
}

#[derive(Serialize, Debug)]
pub struct Edit {
    pub content: String,
    pub document_uri: String,
    pub end_byte: usize,
    pub end_column: usize,
    pub end_row: usize,
    pub language: String,
    pub operation: String,
    pub start_byte: usize,
    pub start_column: usize,
    pub start_row: usize,
    pub timestamp: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
pub struct SaveEditsResponse {
    pub created_blocks: Vec<Block>,
    pub modified_blocks: Vec<Block>,
    // pub removed_blocks: Vec<Block>,
    pub errors: Vec<EditError>,
    // pub flow_ref: FlowRef,
}

#[derive(Deserialize, Debug)]
pub struct EditError {
    pub details: Option<String>,
    pub message: String,
    pub uri: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SimpleLLMMessage {
    #[serde(rename = "role")]
    pub role: SimpleLLMRole,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SimpleLLMRole {
    User,
    Assistant,
}

impl std::fmt::Display for SimpleLLMRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimpleLLMRole::User => write!(f, "user"),
            SimpleLLMRole::Assistant => write!(f, "assistant"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GenerateCodeInput {
    pub prompt: String,
    pub provisioner: ProvisionerType,
    pub resolve_validation_errors: bool,
    pub stream: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GenerateCodeOutput {
    pub prompt: String,
    pub result: GenerationResult,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenerationResult {
    // pub created_blocks: Vec<Block>,
    // pub modified_blocks: Vec<Block>,
    // pub removed_blocks: Vec<Block>,
    pub score: i32,
    pub selected_blocks: Vec<Block>,
    pub edits: Option<Vec<EditInfo>>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub delta: Option<GenerationDelta>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EditInfo {
    pub reasoning: String,
    pub document_uri: String,
    pub old_str: String,
    pub new_str: String,
}

impl std::fmt::Display for EditInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.old_str.is_empty(), self.new_str.is_empty()) {
            // replace
            (false, false) => {
                write!(
                    f,
                    r#"# {}
{}
```
<<<<<<< SEARCH
{}
=======
{}
>>>>>>> REPLACE
```"#,
                    self.reasoning,
                    self.document_uri
                        .strip_prefix("file://")
                        .unwrap_or(&self.document_uri),
                    self.old_str,
                    self.new_str
                )
            }
            // append
            (true, false) => {
                write!(
                    f,
                    r#"# {}
{}
```
{}
```"#,
                    self.reasoning,
                    self.document_uri
                        .strip_prefix("file://")
                        .unwrap_or(&self.document_uri),
                    self.new_str
                )
            }
            // remove
            (false, true) => {
                write!(
                    f,
                    r#"# {}
{}
```
<<<<<<< SEARCH
{}
=======
>>>>>>> REPLACE
```"#,
                    self.reasoning,
                    self.document_uri
                        .strip_prefix("file://")
                        .unwrap_or(&self.document_uri),
                    self.old_str
                )
            }
            _ => Ok(()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolsCallParams {
    pub name: String,
    pub arguments: Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolsCallResponse {
    pub content: Vec<Content>,
}
