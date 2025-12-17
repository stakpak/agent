use crate::container::ContainerConfig;
use crate::local_store::LocalStore;
use crate::models::error::{AgentError, BadRequestErrorMessage};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "http://localhost:8000";
const DEFAULT_SCRAPE_LIMIT: u32 = 5;
const MIN_LIMIT: u32 = 1;
const MAX_LIMIT: u32 = 100;
const DEFAULT_LANGUAGE: &str = "en";
const MAX_RETRIES: u32 = 3;
const HEALTH_CHECK_TIMEOUT_SECS: u64 = 30;
const HEALTH_CHECK_INTERVAL_SECS: u64 = 1;
const CONFIG_FILE: &str = "searchpak_config.json";

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

impl SearchRequest {
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.query.trim().is_empty() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                "Search query cannot be empty".to_string(),
            )));
        }

        if self.limit < MIN_LIMIT || self.limit > MAX_LIMIT {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!("Limit must be between {} and {}", MIN_LIMIT, MAX_LIMIT),
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ScrapeRequest {
    pub urls: Vec<String>,
}

impl ScrapeRequest {
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.urls.is_empty() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                "URLs list cannot be empty".to_string(),
            )));
        }

        for url in &self.urls {
            if url.trim().is_empty() {
                return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                    "URL cannot be empty".to_string(),
                )));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SearchAndScrapeRequest {
    #[serde(flatten)]
    pub search: SearchRequest,
    pub scrape_limit: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchPakConfig {
    pub searxng_port: u16,
    pub api_port: u16,
    pub searxng_container_id: String,
    pub api_container_id: String,
}

pub struct SearchPakClient {
    client: ClientWithMiddleware,
    api_url: String,
    searxng_url: String,
}

impl SearchPakClient {
    /// Creates a new SearchPak client with optional custom URLs
    pub fn new(api_url: Option<String>, searxng_url: Option<String>) -> Self {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(MAX_RETRIES);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Self {
            client,
            api_url: api_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            searxng_url: searxng_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        }
    }

    /// Performs a web search with the given query
    pub async fn search(&self, query: String) -> Result<Vec<SearchResult>, AgentError> {
        let request = SearchRequest {
            query,
            limit: DEFAULT_SCRAPE_LIMIT,
            lang: DEFAULT_LANGUAGE.to_string(),
            engines: None,
        };

        request.validate()?;

        self.execute_request("/search", &request).await
    }

    /// Searches the web and scrapes the top results
    pub async fn search_and_scrape(
        &self,
        query: String,
    ) -> Result<Vec<ScrapedContent>, AgentError> {
        let request = SearchAndScrapeRequest {
            search: SearchRequest {
                query,
                limit: DEFAULT_SCRAPE_LIMIT,
                lang: DEFAULT_LANGUAGE.to_string(),
                engines: None,
            },
            scrape_limit: DEFAULT_SCRAPE_LIMIT,
        };

        request.search.validate()?;

        self.execute_request("/search-and-scrape", &request).await
    }

    /// Scrapes content from the provided URLs
    pub async fn scrape(&self, urls: Vec<String>) -> Result<Vec<ScrapedContent>, AgentError> {
        let request = ScrapeRequest { urls };
        request.validate()?;

        self.execute_request("/scrape", &request).await
    }

    /// Generic method to execute API requests with proper error handling
    async fn execute_request<T, R>(&self, endpoint: &str, request: &T) -> Result<R, AgentError>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let response = self
            .client
            .post(format!("{}{}", self.api_url, endpoint))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to send request to {}: {}",
                    endpoint, e
                )))
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!(
                    "Request to {} failed with status {}: {}",
                    endpoint, status, error_text
                ),
            )));
        }

        response.json::<R>().await.map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                "Failed to parse response from {}: {}",
                endpoint, e
            )))
        })
    }

    /// Checks if the SearchPak API is healthy
    pub async fn health_check_api(&self) -> Result<(), AgentError> {
        let response = self
            .client
            .get(format!("{}/health", self.api_url))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "API health check failed: {}",
                    e
                )))
            })?;

        if !response.status().is_success() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!("API health check returned status: {}", response.status()),
            )));
        }

        Ok(())
    }

    /// Checks if the SearXNG service is healthy
    pub async fn health_check_searxng(&self) -> Result<(), AgentError> {
        let response = self
            .client
            .get(format!("{}/healthz", self.searxng_url))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "SearXNG health check failed: {}",
                    e
                )))
            })?;

        if !response.status().is_success() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!(
                    "SearXNG health check returned status: {}",
                    response.status()
                ),
            )));
        }

        Ok(())
    }

    fn load_config() -> Option<SearchPakConfig> {
        match LocalStore::read_session_data(CONFIG_FILE) {
            Ok(data) => serde_json::from_str(&data).ok(),
            Err(_) => None,
        }
    }

    fn save_config(config: &SearchPakConfig) -> Result<(), AgentError> {
        let data = serde_json::to_string_pretty(config).map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                "Failed to serialize config: {}",
                e
            )))
        })?;

        LocalStore::write_session_data(CONFIG_FILE, &data)
            .map(|_| ())
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to save config: {}",
                    e
                )))
            })
    }

    /// Finds two available ports, ensuring they're different
    fn find_two_available_ports() -> Result<(u16, u16), AgentError> {
        let port1 = crate::container::find_available_port().ok_or_else(|| {
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                "Failed to find first available port".to_string(),
            ))
        })?;

        // Try up to 5 times to find a second different port
        for _ in 0..5 {
            if let Some(port2) = crate::container::find_available_port() {
                if port2 != port1 {
                    return Ok((port1, port2));
                }
            }
        }

        Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
            "Failed to find two different available ports".to_string(),
        )))
    }

    /// Waits for both services to become healthy
    async fn wait_for_health(&self) -> Result<(), AgentError> {
        let max_attempts = HEALTH_CHECK_TIMEOUT_SECS / HEALTH_CHECK_INTERVAL_SECS;
        let mut api_healthy = false;
        let mut searxng_healthy = false;

        for attempt in 1..=max_attempts {
            if !api_healthy {
                api_healthy = self.health_check_api().await.is_ok();
            }

            if !searxng_healthy {
                searxng_healthy = self.health_check_searxng().await.is_ok();
            }

            if api_healthy && searxng_healthy {
                return Ok(());
            }

            if attempt < max_attempts {
                tokio::time::sleep(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS)).await;
            }
        }

        let mut errors = Vec::new();
        if !api_healthy {
            errors.push("API");
        }
        if !searxng_healthy {
            errors.push("SearXNG");
        }

        Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
            format!(
                "Health check timed out after {}s. Unhealthy services: {}",
                HEALTH_CHECK_TIMEOUT_SECS,
                errors.join(", ")
            ),
        )))
    }

    async fn start_containers(&mut self) -> Result<(), AgentError> {
        let searxng_image = env::var("SEARXNG_IMAGE")
            .unwrap_or_else(|_| "299shehab299/searchpak:searxng".to_string());
        let api_image =
            env::var("API_IMAGE").unwrap_or_else(|_| "299shehab299/searchpak:api".to_string());

        let (searxng_port, api_port) = Self::find_two_available_ports()?;

        let searxng_env = HashMap::from([
            (
                "BASE_URL".to_string(),
                format!("http://localhost:{}", searxng_port),
            ),
            ("INSTANCE_NAME".to_string(), "SearchPak".to_string()),
            ("BIND_ADDRESS".to_string(), "0.0.0.0:8080".to_string()),
            ("SEARXNG_LIMITER".to_string(), "false".to_string()),
            ("SEARXNG_SEARCH_FORMATS".to_string(), "json".to_string()),
            ("SEARXNG_IMAGE_PROXY".to_string(), "true".to_string()),
        ]);

        let searxng_config = ContainerConfig {
            image: searxng_image,
            env_vars: searxng_env,
            ports: vec![format!("{}:8080", searxng_port)],
            extra_hosts: vec!["host.docker.internal:host-gateway".to_string()],
            volumes: vec![],
        };

        let searxng_container_id = crate::container::run_container_detached(searxng_config)
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to start SearXNG container: {}",
                    e
                )))
            })?;

        let api_env = HashMap::from([
            (
                "SEARXNG_BASE_URL".to_string(),
                format!("http://host.docker.internal:{}", searxng_port),
            ),
            ("PORT".to_string(), "8000".to_string()),
        ]);

        let api_config = ContainerConfig {
            image: api_image,
            env_vars: api_env,
            ports: vec![format!("{}:8000", api_port)],
            extra_hosts: vec!["host.docker.internal:host-gateway".to_string()],
            volumes: vec![],
        };

        let api_container_id =
            crate::container::run_container_detached(api_config).map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to start API container: {}",
                    e
                )))
            })?;

        self.api_url = format!("http://localhost:{}", api_port);
        self.searxng_url = format!("http://localhost:{}", searxng_port);

        self.wait_for_health().await?;

        let config = SearchPakConfig {
            searxng_port,
            api_port,
            searxng_container_id,
            api_container_id,
        };

        Self::save_config(&config)?;

        Ok(())
    }

    pub async fn ensure_running(&mut self) -> Result<(), AgentError> {
        // Try to load existing configuration
        if let Some(config) = Self::load_config() {
            self.api_url = format!("http://localhost:{}", config.api_port);
            self.searxng_url = format!("http://localhost:{}", config.searxng_port);

            // Check if services are already running
            if self.health_check_api().await.is_ok() && self.health_check_searxng().await.is_ok() {
                return Ok(());
            }
        }

        self.start_containers().await
    }

    pub async fn stop(&mut self) -> Result<(), AgentError> {
        if let Some(config) = Self::load_config() {
            crate::container::stop_container(&config.searxng_container_id).map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to stop SearXNG container: {}",
                    e
                )))
            })?;

            crate::container::stop_container(&config.api_container_id).map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to stop API container: {}",
                    e
                )))
            })?;

            Self::save_config(&SearchPakConfig {
                searxng_port: 0,
                api_port: 0,
                searxng_container_id: "".to_string(),
                api_container_id: "".to_string(),
            })?;

            Ok(())
        } else {
            Ok(())
        }
    }
}
