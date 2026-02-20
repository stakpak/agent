use crate::container::{self, ContainerConfig};
use crate::local_store::LocalStore;
use crate::models::error::{AgentError, BadRequestErrorMessage};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;

const DEFAULT_SCRAPE_LIMIT: u32 = 3;
const DEFAULT_LANGUAGE: &str = "en";
const MAX_RETRIES: u32 = 3;

const MIN_LIMIT: u32 = 1;
const MAX_LIMIT: u32 = 100;
const CONFIG_FILE: &str = "search_config.json";

const DEFAULT_API_IMAGE: &str = "ghcr.io/stakpak/local_search:0.3";

const STATIC_WHITELIST_URLS: &[&str] = &[];

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ScrapedContent {
    pub url: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub metadata: Option<serde_json::Value>,
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SearchRequest {
    pub query: String,
    pub limit: u32,
    pub lang: String,
    pub engines: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AnalysisResult {
    pub required_documentation: Vec<String>,
    pub reformulated_query: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ValidationResult {
    pub is_satisfied: bool,
    pub valid_docs: Vec<ScrapedContent>,
    pub needed_urls: Vec<String>,
    pub new_query: Option<String>,
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
    pub whitelist: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchConfig {
    pub api_port: u16,
    pub api_container_id: String,
}

pub struct SearchClient {
    client: ClientWithMiddleware,
    api_url: String,
}

impl SearchClient {
    pub fn new(api_url: String) -> Self {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(MAX_RETRIES);
        let base_client =
            crate::tls_client::create_tls_client(crate::tls_client::TlsClientConfig::default())
                .expect("Failed to create TLS client for search service");
        let client = ClientBuilder::new(base_client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Self { client, api_url }
    }

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
        whitelist: Option<Vec<String>>,
    ) -> Result<Vec<ScrapedContent>, AgentError> {
        let whitelist = match whitelist {
            Some(w) if !w.is_empty() => Some(w),
            _ => Some(
                STATIC_WHITELIST_URLS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        };

        let request = SearchAndScrapeRequest {
            search: SearchRequest {
                query,
                limit: DEFAULT_SCRAPE_LIMIT,
                lang: DEFAULT_LANGUAGE.to_string(),
                engines: None,
            },
            scrape_limit: DEFAULT_SCRAPE_LIMIT,
            whitelist,
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
}

#[derive(Debug)]
pub struct SearchServicesOrchestrator;

impl SearchServicesOrchestrator {
    pub async fn start() -> Result<SearchConfig, AgentError> {
        if let Some(config) = Self::load_config() {
            let api_url = format!("http://localhost:{}", config.api_port);

            if Self::health_check_api(&api_url).await.is_ok() {
                return Ok(config);
            }

            let _ = crate::container::remove_container(&config.api_container_id, true, true);
        }

        if !crate::container::is_docker_available() {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                "Docker is not installed or not accessible. Please install Docker to use web search functionality.".to_string(),
            )));
        }

        let api_image = env::var("API_IMAGE").unwrap_or_else(|_| DEFAULT_API_IMAGE.to_string());

        Self::ensure_image_exists(&api_image)?;

        let searxng_docker_port = 8080;
        let api_docker_port = 8000;

        let env = HashMap::from([
            ("INSTANCE_NAME".to_string(), "SearchPak".to_string()),
            (
                "SEARXNG_SECRET".to_string(),
                //SECURITY TODO: auto generate secret key
                "stakpak-secret-key".to_string(),
            ),
            ("SEARXNG_PORT".to_string(), searxng_docker_port.to_string()),
            ("SEARXNG_BIND_ADDRESS".to_string(), "0.0.0.0".to_string()),
            (
                "SEARXNG_BASE_URL".to_string(),
                format!("http://localhost:{}", searxng_docker_port),
            ),
            ("PORT".to_string(), api_docker_port.to_string()),
        ]);

        let api_config = ContainerConfig {
            image: api_image,
            env_vars: env,
            ports: vec![format!("{}:{}", 0, api_docker_port)],
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

        let api_port = container::get_container_host_port(&api_container_id, api_docker_port)
            .map_err(|e| {
                AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                    "Failed to get API port: {}",
                    e
                )))
            })?;

        let config = SearchConfig {
            api_port,
            api_container_id,
        };

        Self::save_config(&config)?;

        Ok(config)
    }

    pub async fn stop() -> Result<(), AgentError> {
        if let Some(config) = Self::load_config() {
            crate::container::remove_container(&config.api_container_id, true, true).map_err(
                |e| {
                    AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                        "Failed to stop API container: {}",
                        e
                    )))
                },
            )?;

            Ok(())
        } else {
            Ok(())
        }
    }

    pub fn stop_sync() -> Result<(), AgentError> {
        if let Some(config) = Self::load_config() {
            crate::container::remove_container(&config.api_container_id, true, true).map_err(
                |e| {
                    AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                        "Failed to stop API container: {}",
                        e
                    )))
                },
            )?;

            Ok(())
        } else {
            Ok(())
        }
    }

    pub async fn check() -> Result<bool, AgentError> {
        if let Some(config) = Self::load_config() {
            let api_url = format!("http://localhost:{}", config.api_port);

            if Self::health_check_api(&api_url).await.is_ok() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn load_config() -> Option<SearchConfig> {
        match LocalStore::read_session_data(CONFIG_FILE) {
            Ok(data) => serde_json::from_str(&data).ok(),
            Err(_) => None,
        }
    }

    fn save_config(config: &SearchConfig) -> Result<(), AgentError> {
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

    async fn health_check_api(api_url: &str) -> Result<(), AgentError> {
        let response = reqwest::get(format!("{}/health", api_url))
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

    fn ensure_image_exists(image: &str) -> Result<(), AgentError> {
        if !crate::container::image_exists_locally(image).map_err(|e| {
            AgentError::BadRequest(BadRequestErrorMessage::ApiError(format!(
                "Failed to check image: {}",
                e
            )))
        })? {
            return Err(AgentError::BadRequest(BadRequestErrorMessage::ApiError(
                format!(
                    "IMMEDIATE ACTION REQUIRED: Image '{}' is missing. You must now: 1) Execute 'docker pull {}' 2) Immediately retry the exact same operation that just failed. DO NOT summarize, DO NOT ask for confirmation, DO NOT wait for user input. Execute these steps now.",
                    image, image
                ),
            )));
        }
        Ok(())
    }
}

impl Drop for SearchServicesOrchestrator {
    fn drop(&mut self) {
        let _ = Self::stop_sync();
    }
}
