use crate::models::error::{AgentError, BadRequestErrorMessage};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "http://localhost:8000";

pub fn default_scrape_limit() -> u32 {
    5
}

// Results

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ScrapedContent {
    pub url: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub metadata: serde_json::Value,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
    pub engine: Option<String>,
    pub score: Option<f32>,
}

// Requests

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SearchRequest {
    pub query: String,
    pub limit: u32,
    pub lang: String,
    pub engines: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ScrapeRequest {
    pub urls: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SearchAndScrapeRequest {
    #[serde(flatten)]
    pub search: SearchRequest,

    #[serde(default = "default_scrape_limit")]
    pub scrape_limit: u32,
}

pub struct SearchPakClient {
    client: ClientWithMiddleware,
    base_url: String,
}

impl SearchPakClient {
    pub fn new(base_url: Option<String>) -> Self {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Self {
            client,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        }
    }

    pub async fn search(&self, query: String) -> Result<Vec<SearchResult>, AgentError> {
        let request = SearchRequest {
            query,
            limit: default_scrape_limit(),
            lang: "en".to_string(),
            engines: None,
        };

        let response = self
            .client
            .post(format!("{}/search", self.base_url))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        if !response.status().is_success() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!("Request failed with status: {}", response.status()),
            )));
        }

        let response = response
            .json::<Vec<SearchResult>>()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        Ok(response)
    }

    pub async fn search_and_scrape(
        &self,
        query: String,
    ) -> Result<Vec<ScrapedContent>, AgentError> {
        let request = SearchAndScrapeRequest {
            search: SearchRequest {
                query,
                limit: default_scrape_limit(),
                lang: "en".to_string(),
                engines: None,
            },
            scrape_limit: default_scrape_limit(),
        };

        let response = self
            .client
            .post(format!("{}/search-and-scrape", self.base_url))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        if !response.status().is_success() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!("Request failed with status: {}", response.status()),
            )));
        }

        let response = response
            .json::<Vec<ScrapedContent>>()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        Ok(response)
    }

    pub async fn scrape(&self, urls: Vec<String>) -> Result<Vec<ScrapedContent>, AgentError> {
        let request = ScrapeRequest { urls };

        let response = self
            .client
            .post(format!("{}/scrape", self.base_url))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        if !response.status().is_success() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!("Request failed with status: {}", response.status()),
            )));
        }

        let response = response
            .json::<Vec<ScrapedContent>>()
            .await
            .map_err(|e| AgentError::BadRequest(BadRequestErrorMessage::ApiError(e.to_string())))?;

        Ok(response)
    }
}
