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

    pub async fn get_my_account(&self) -> Result<GetMyAccountResponse, ReqwestError> {
        let url = format!("{}/account", self.base_url);
        self.client.get(&url).send().await?.json().await
    }

    pub async fn list_flows(&self, owner_name: &str) -> Result<GetFlowsResponse, ReqwestError> {
        let url = format!("{}/flows/{}", self.base_url, owner_name);
        self.client.get(&url).send().await?.json().await
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GetMyAccountResponse {
    pub username: String,
    pub id: String,
    pub first_name: String,
    pub last_name: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct GetFlowsResponse {
    pub results: Vec<Flow>,
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
