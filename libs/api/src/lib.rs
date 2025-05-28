use chrono::{DateTime, Utc};
use reqwest::{Client as ReqwestClient, Error as ReqwestError, header};
use rmcp::model::Content;
use rmcp::model::JsonRpcResponse;
use serde::{Deserialize, Serialize};
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
    // key: String,
    message: String,
}

impl Client {
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

        let client = ReqwestClient::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create HTTP client");

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

    pub async fn list_flows(&self, owner_name: &str) -> Result<GetFlowsResponse, String> {
        let url = format!("{}/flows/{}", self.base_url, owner_name);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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
    ) -> Result<impl Stream<Item = Result<Vec<ChatCompletionStreamResponse>, String>>, String> {
        let url = format!("{}/agents/openai/v1/chat/completions", self.base_url);

        let input = ChatCompletionRequest::new(messages, tools, Some(true));

        let response = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

        let stream = response.bytes_stream().map(|chunk| {
            chunk
                .map_err(|_| "Failed to read response".to_string())
                .and_then(|bytes| {
                    std::str::from_utf8(&bytes)
                        .map_err(|_| "Failed to parse UTF-8 from Anthropic response".to_string())
                        .map(|text| {
                            text.split("\n\n")
                                .filter(|event| event.starts_with("data: "))
                                .filter_map(|event| {
                                    event.strip_prefix("data: ").and_then(|json_str| {
                                        serde_json::from_str::<ChatCompletionStreamResponse>(
                                            json_str,
                                        )
                                        .ok()
                                    })
                                })
                                .collect::<Vec<ChatCompletionStreamResponse>>()
                        })
                })
        });

        Ok(stream)
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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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

        if !response.status().is_success() {
            let error: ApiError = response.json().await.map_err(|e| e.to_string())?;
            return Err(error.error.message);
        }

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
                    result.block.document_uri.strip_prefix("file:///").unwrap(),
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
    pub created_blocks: Vec<Block>,
    pub modified_blocks: Vec<Block>,
    pub removed_blocks: Vec<Block>,
    pub score: i32,
    pub selected_blocks: Vec<Block>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub delta: Option<GenerationDelta>,
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
