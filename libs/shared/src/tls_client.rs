use reqwest::{Client, header::HeaderMap};
use rustls_platform_verifier::BuilderVerifierExt;

pub fn create_tls_client(headers: HeaderMap) -> Result<Client, String> {
    // needed to use OS-provided CA certificates with Rustls
    let arc_crypto_provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let tls_config = rustls::ClientConfig::builder_with_provider(arc_crypto_provider)
        .with_safe_default_protocol_versions()
        .expect("Failed to build client TLS config")
        .with_platform_verifier()
        .with_no_client_auth();

    let client = Client::builder()
        .use_preconfigured_tls(tls_config)
        .default_headers(headers)
        .build()
        .expect("Failed to create HTTP client");

    Ok(client)
}
