use reqwest::{header, Client as ReqwestClient, Error as ReqwestError};
use serde::{Deserialize, Serialize};

mod models;
use models::*;

use crate::config::AppConfig;

pub struct Client {
    client: ReqwestClient,
    base_url: String,
}

impl Client {
    pub fn new(config: &AppConfig) -> Result<Self, String> {
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

        let value: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?
            .json()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;
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

        let value: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?
            .json()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;
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

        let value: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?
            .json()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;
        match serde_json::from_value::<GetFlowResponse>(value.clone()) {
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
        owner_name: &str,
        flow_name: &str,
        version_ref: &str,
    ) -> Result<GetFlowDocumentsResponse, String> {
        let url = format!(
            "{}/flows/{}/{}/{}/documents",
            self.base_url, owner_name, flow_name, version_ref
        );

        let value: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?
            .json()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;
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

        let input = QueryCommandInput {
            query: query.to_string(),
            generate_query,
            synthesize_output,
            flow_ref: flow_ref.and_then(|flow_ref| {
                let parts: Vec<&str> = flow_ref.split('/').collect();
                if parts.len() != 3 {
                    return None;
                }
                let owner_name = parts[0].to_string();
                let flow_name = parts[1].to_string();
                let version_ref = parts[2].to_string();

                Some(FlowRef::new(owner_name, flow_name, version_ref))
            }),
        };

        let value: serde_json::Value = self
            .client
            .post(&url)
            .json(&input)
            .send()
            .await
            .map_err(|e: ReqwestError| e.to_string())?
            .json()
            .await
            .map_err(|e: ReqwestError| e.to_string())?;
        match serde_json::from_value::<QueryBlocksResponse>(value.clone()) {
            Ok(response) => Ok(response),
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
    pub fn to_text(&self) -> String {
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
                "{:<20} {:<40} {:<10} {:<20} {:<40}\n",
                flow.name,
                latest_version.id,
                format!("{:?}", flow.visibility),
                latest_version.created_at.format("\"%Y-%m-%d %H:%M UTC\""),
                if tags.is_empty() { "-" } else { &tags }
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
    pub fn to_text(&self) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "{} ({:?})\n",
            self.resource.name, self.resource.visibility
        ));
        output.push_str("\nVersions:\n");

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
                "{:<40} \"{:<20}\" {}\n",
                version.id,
                version.created_at.format("%Y-%m-%d %H:%M UTC"),
                if tags.is_empty() { "-" } else { &tags }
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
    pub semantic_query: String,
    pub output: Option<String>,
}

impl QueryBlocksResponse {
    pub fn to_text(&self) -> String {
        let mut output = String::new();
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

        if !self.semantic_query.is_empty() {
            output.push_str(&format!("\nQuery: {}\n", self.semantic_query));
        }

        if let Some(synthesized_output) = &self.output {
            output.push_str(&format!("\nOutput: {}\n", synthesized_output));
        }

        output.trim_end().to_string()
    }
}
