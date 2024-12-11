use chrono::{DateTime, Utc};
use reqwest::{header, Client as ReqwestClient, Error as ReqwestError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
            base_url: "https://apiv2.stakpak.dev/v1".to_string(),
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

#[derive(Serialize, Deserialize, Debug)]
pub struct GetFlowPermission {
    pub read: bool,
    pub write: bool,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Flow {
    pub id: Uuid,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub name: String,
    pub visibility: FlowVisibility,
    pub versions: Vec<FlowVersion>,
}
#[derive(Deserialize, Serialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum FlowVisibility {
    #[default]
    Public,
    Private,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FlowVersion {
    pub id: Uuid,
    pub immutable: bool,
    pub created_at: DateTime<Utc>,
    pub tags: Vec<FlowTag>,
    pub parent: Option<FlowVersionRelation>,
    pub children: Vec<FlowVersionRelation>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FlowTag {
    pub name: String,
    pub description: String,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FlowVersionRelation {
    pub id: Uuid,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct GetFlowDocumentsResponse {
    pub documents: Vec<Document>,
    pub additional_documents: Vec<Document>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Document {
    pub content: String,
    pub uri: String,
}
