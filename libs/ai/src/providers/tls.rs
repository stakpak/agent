//! Shared TLS client configuration for all providers.
//!
//! Uses `rustls` with `rustls-platform-verifier` to create HTTP clients
//! that validate server certificates against the OS-provided CA certificate
//! store. This is important for enterprise environments with custom CA certs
//! (e.g., corporate proxies, private PKI).

use crate::error::{Error, Result};
use reqwest::Client;
use rustls_platform_verifier::BuilderVerifierExt;

/// Create an HTTP client configured with platform-verified TLS.
///
/// Uses `rustls` with the OS-provided CA certificate store via
/// `rustls-platform-verifier`, ensuring proper certificate validation
/// when calling provider APIs.
pub fn create_platform_tls_client() -> Result<Client> {
    let arc_crypto_provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let tls_config = rustls::ClientConfig::builder_with_provider(arc_crypto_provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::provider_error(format!("Failed to build TLS config: {}", e)))?
        .with_platform_verifier()
        .with_no_client_auth();

    Client::builder()
        .use_preconfigured_tls(tls_config)
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| Error::provider_error(format!("Failed to create TLS HTTP client: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_platform_tls_client_succeeds() {
        let client = create_platform_tls_client();
        assert!(client.is_ok(), "TLS client creation should succeed");
    }

    #[test]
    fn test_create_platform_tls_client_returns_usable_client() {
        let client = create_platform_tls_client().unwrap();
        // Client should be able to build a request without panicking
        let _request = client.get("https://example.com").build().unwrap();
    }

    #[test]
    fn test_multiple_tls_clients_can_be_created() {
        // Ensure creating multiple clients doesn't cause issues with
        // the crypto provider or platform verifier
        let client1 = create_platform_tls_client();
        let client2 = create_platform_tls_client();
        let client3 = create_platform_tls_client();
        assert!(client1.is_ok());
        assert!(client2.is_ok());
        assert!(client3.is_ok());
    }

    #[tokio::test]
    async fn test_tls_client_validates_valid_certificate() {
        let client = create_platform_tls_client().unwrap();
        // A GET to a well-known HTTPS site should succeed with proper TLS validation
        let response = client.get("https://example.com").send().await;
        assert!(
            response.is_ok(),
            "Request to a valid HTTPS endpoint should succeed: {:?}",
            response.err()
        );
    }

    #[tokio::test]
    async fn test_tls_client_rejects_self_signed_certificate() {
        let client = create_platform_tls_client().unwrap();
        // self-signed.badssl.com uses a self-signed certificate that should be rejected
        let response = client.get("https://self-signed.badssl.com/").send().await;
        assert!(
            response.is_err(),
            "Request to a self-signed HTTPS endpoint should fail TLS validation"
        );
    }

    #[tokio::test]
    async fn test_tls_client_rejects_expired_certificate() {
        let client = create_platform_tls_client().unwrap();
        // expired.badssl.com uses an expired certificate that should be rejected
        let response = client.get("https://expired.badssl.com/").send().await;
        assert!(
            response.is_err(),
            "Request to an expired HTTPS endpoint should fail TLS validation"
        );
    }

    #[tokio::test]
    async fn test_tls_client_rejects_wrong_host_certificate() {
        let client = create_platform_tls_client().unwrap();
        // wrong.host.badssl.com uses a certificate for a different host
        let response = client.get("https://wrong.host.badssl.com/").send().await;
        assert!(
            response.is_err(),
            "Request to a wrong-host HTTPS endpoint should fail TLS validation"
        );
    }
}
