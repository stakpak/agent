use std::time::Duration;

use reqwest::{Client, header::HeaderMap, redirect::Policy};
use rustls_platform_verifier::BuilderVerifierExt;

pub struct TlsClientConfig {
    pub headers: HeaderMap,
    pub timeout: Duration,
    pub redirect_policy: Policy,
}

impl Default for TlsClientConfig {
    fn default() -> Self {
        Self {
            headers: HeaderMap::new(),
            timeout: Duration::from_secs(30),
            redirect_policy: Policy::limited(10),
        }
    }
}

impl TlsClientConfig {
    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_redirect_policy(mut self, redirect_policy: Policy) -> Self {
        self.redirect_policy = redirect_policy;
        self
    }
}

pub fn create_tls_client(config: TlsClientConfig) -> Result<Client, String> {
    // needed to use OS-provided CA certificates with Rustls
    let arc_crypto_provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let tls_config = rustls::ClientConfig::builder_with_provider(arc_crypto_provider)
        .with_safe_default_protocol_versions()
        .expect("Failed to build client TLS config")
        .with_platform_verifier()
        .with_no_client_auth();

    let client = Client::builder()
        .use_preconfigured_tls(tls_config)
        .default_headers(config.headers)
        .timeout(config.timeout)
        .redirect(config.redirect_policy)
        .build()
        .expect("Failed to create HTTP client");

    Ok(client)
}
