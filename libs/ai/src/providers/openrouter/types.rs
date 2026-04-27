use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub base_url: String,
    pub http_referer: Option<String>,
    pub site_title: Option<String>,
}

impl OpenRouterConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            http_referer: None,
            site_title: None,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_http_referer(mut self, referer: impl Into<String>) -> Self {
        self.http_referer = Some(referer.into());
        self
    }

    pub fn with_site_title(mut self, title: impl Into<String>) -> Self {
        self.site_title = Some(title.into());
        self
    }
}

impl Default for OpenRouterConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| String::new()),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            http_referer: None,
            site_title: None,
        }
    }
}