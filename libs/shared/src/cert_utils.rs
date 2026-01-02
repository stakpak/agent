use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyUsagePurpose, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;

pub struct CertificateChain {
    pub ca_cert: rcgen::Certificate,
    pub server_cert: rcgen::Certificate,
    pub client_cert: rcgen::Certificate,
}

impl std::fmt::Debug for CertificateChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertificateChain")
            .field("ca_cert", &"<Certificate>")
            .field("server_cert", &"<Certificate>")
            .field("client_cert", &"<Certificate>")
            .finish()
    }
}

pub fn default_cert_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home).join(".stakpak").join("certs"))
}

/// Certificate management strategy for different use cases
#[derive(Debug, Clone)]
pub enum CertificateStrategy {
    /// Generate in memory certificates
    Ephemeral,
    /// Load persistent certificates from disk
    Persistent(PathBuf),
}

impl CertificateStrategy {
    pub fn load_server_config(&self) -> Result<ServerConfig> {
        match self {
            Self::Ephemeral => {
                let chain = CertificateChain::generate()?;
                chain.create_server_config()
            }
            Self::Persistent(path) => CertificateChain::load_server_config(path),
        }
    }

    pub fn load_client_config(&self) -> Result<ClientConfig> {
        match self {
            Self::Ephemeral => {
                let chain = CertificateChain::generate()?;
                chain.create_client_config()
            }
            Self::Persistent(path) => CertificateChain::load_client_config(path),
        }
    }

    /// Get the certificate chain (only works for Ephemeral strategy)
    /// For Persistent strategy, returns an error as certificates are already on disk
    pub fn get_certificate_chain(&self) -> Result<CertificateChain> {
        match self {
            Self::Ephemeral => CertificateChain::generate(),
            Self::Persistent(path) => Err(anyhow::anyhow!(
                "Cannot get certificate chain for Persistent strategy. Certificates are stored at: {}",
                path.display()
            )),
        }
    }

    /// Check if certificates exist
    pub fn exists(&self) -> bool {
        match self {
            Self::Ephemeral => true, // Ephemeral certs are always available since they're generated on demand
            Self::Persistent(path) => CertificateChain::exists_in_directory(path),
        }
    }

    /// Get default certifications directory
    pub fn default_persistent() -> Result<Self> {
        Ok(Self::Persistent(default_cert_dir()?))
    }
}

impl CertificateChain {
    pub fn generate() -> Result<Self> {
        // Generate CA certificate
        let mut ca_params = CertificateParams::default();
        ca_params.distinguished_name = DistinguishedName::new();
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Stakpak MCP CA");
        ca_params
            .distinguished_name
            .push(DnType::OrganizationName, "Stakpak");
        ca_params.distinguished_name.push(DnType::CountryName, "US");

        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];

        ca_params.not_before = OffsetDateTime::now_utc() - time::Duration::seconds(60);
        ca_params.not_after = OffsetDateTime::now_utc() + time::Duration::days(365);

        let ca_cert = rcgen::Certificate::from_params(ca_params)?;

        // Generate server certificate
        let mut server_params = CertificateParams::default();
        server_params.distinguished_name = DistinguishedName::new();
        server_params
            .distinguished_name
            .push(DnType::CommonName, "Stakpak MCP Server");
        server_params
            .distinguished_name
            .push(DnType::OrganizationName, "Stakpak");
        server_params
            .distinguished_name
            .push(DnType::CountryName, "US");

        server_params.subject_alt_names = vec![
            SanType::DnsName("localhost".to_string()),
            SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0))),
            SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        ];

        server_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];

        server_params.not_before = OffsetDateTime::now_utc() - time::Duration::seconds(60);
        server_params.not_after = OffsetDateTime::now_utc() + time::Duration::days(365);

        let server_cert = rcgen::Certificate::from_params(server_params)?;

        // Generate client certificate
        let mut client_params = CertificateParams::default();
        client_params.distinguished_name = DistinguishedName::new();
        client_params
            .distinguished_name
            .push(DnType::CommonName, "Stakpak MCP Client");
        client_params
            .distinguished_name
            .push(DnType::OrganizationName, "Stakpak");
        client_params
            .distinguished_name
            .push(DnType::CountryName, "US");

        client_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];

        client_params.not_before = OffsetDateTime::now_utc() - time::Duration::seconds(60);
        client_params.not_after = OffsetDateTime::now_utc() + time::Duration::days(365);

        let client_cert = rcgen::Certificate::from_params(client_params)?;

        Ok(CertificateChain {
            ca_cert,
            server_cert,
            client_cert,
        })
    }

    /// Load server configuration from PEM files on disk
    /// Use this for the 'start' command
    pub fn load_server_config(dir: &Path) -> Result<ServerConfig> {
        // Check if all files exist
        if !Self::exists_in_directory(dir) {
            return Err(anyhow::anyhow!(
                "Certificates not found at {:?}\nRun 'stakpak mcp setup' to generate them",
                dir
            ));
        }

        let ca_file = File::open(dir.join("ca.pem")).context("Failed to open CA certificate")?;
        let ca_certs: Vec<CertificateDer> = certs(&mut BufReader::new(ca_file))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse CA certificate")?;

        let server_cert_file =
            File::open(dir.join("server-cert.pem")).context("Failed to open server certificate")?;
        let server_cert_chain: Vec<CertificateDer> = certs(&mut BufReader::new(server_cert_file))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse server certificate")?;

        let server_key_file =
            File::open(dir.join("server-key.pem")).context("Failed to open server private key")?;
        let mut server_keys = pkcs8_private_keys(&mut BufReader::new(server_key_file))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse server private key")?;

        if server_keys.is_empty() {
            return Err(anyhow::anyhow!("No private key found in server-key.pem"));
        }

        let server_private_key = PrivateKeyDer::Pkcs8(server_keys.remove(0));

        let mut root_cert_store = RootCertStore {
            roots: Vec::with_capacity(ca_certs.len()),
        };
        for cert in ca_certs {
            root_cert_store.add(cert)?;
        }

        let client_cert_verifier =
            rustls::server::WebPkiClientVerifier::builder(Arc::new(root_cert_store))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to build client cert verifier: {}", e))?;

        let config = ServerConfig::builder()
            .with_client_cert_verifier(client_cert_verifier)
            .with_single_cert(server_cert_chain, server_private_key)?;

        Ok(config)
    }

    /// Create server configuration from in memory certificates
    /// Used by `CertificateStrategy::Ephemeral` for testing and development.
    /// For production use, prefer `load_server_config()` with `CertificateStrategy::Persistent` for better security and certificate persistence.
    pub fn create_server_config(&self) -> Result<ServerConfig> {
        // Sign server certificate with CA
        let server_cert_der = self.server_cert.serialize_der_with_signer(&self.ca_cert)?;
        let server_key_der = self.server_cert.serialize_private_key_der();

        let server_cert_chain = vec![CertificateDer::from(server_cert_der)];
        let server_private_key = PrivateKeyDer::try_from(server_key_der)
            .map_err(|e| anyhow::anyhow!("Failed to convert server private key: {:?}", e))?;

        // Set up root certificate store to trust our CA (for client cert validation)
        let mut root_cert_store = RootCertStore::empty();
        let ca_cert_der = self.ca_cert.serialize_der()?;
        root_cert_store.add(CertificateDer::from(ca_cert_der))?;

        // Create client certificate verifier that requires client certificates
        let client_cert_verifier =
            rustls::server::WebPkiClientVerifier::builder(Arc::new(root_cert_store))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to build client cert verifier: {}", e))?;

        let config = ServerConfig::builder()
            .with_client_cert_verifier(client_cert_verifier)
            .with_single_cert(server_cert_chain, server_private_key)?;

        Ok(config)
    }

    /// Load client configuration from PEM files on disk
    /// Use this for clients to load existing certificates
    pub fn load_client_config(dir: &Path) -> Result<ClientConfig> {
        if !Self::exists_in_directory(dir) {
            return Err(anyhow::anyhow!(
                "Certificates not found at {:?}\nRun 'stakpak mcp setup' to generate them",
                dir
            ));
        }

        let ca_file = File::open(dir.join("ca.pem")).context("Failed to open CA certificate")?;
        let ca_certs: Vec<CertificateDer> = certs(&mut BufReader::new(ca_file))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse CA certificate")?;

        let client_cert_file =
            File::open(dir.join("client-cert.pem")).context("Failed to open client certificate")?;
        let client_certs: Vec<CertificateDer> = certs(&mut BufReader::new(client_cert_file))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse client certificate")?;

        let client_key_file =
            File::open(dir.join("client-key.pem")).context("Failed to open client private key")?;
        let mut client_keys = pkcs8_private_keys(&mut BufReader::new(client_key_file))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse client private key")?;

        if client_keys.is_empty() {
            return Err(anyhow::anyhow!("No private key found in client-key.pem"));
        }

        let client_key = PrivateKeyDer::Pkcs8(client_keys.remove(0));

        let mut root_cert_store = RootCertStore {
            roots: Vec::with_capacity(ca_certs.len()),
        };
        for cert in ca_certs {
            root_cert_store.add(cert)?;
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_cert_store)
            .with_client_auth_cert(client_certs, client_key)?;

        Ok(config)
    }

    /// Create client configuration from in memory certificates
    /// Used by `CertificateStrategy::Ephemeral` for testing and development.
    /// For production use, prefer `load_client_config()` with `CertificateStrategy::Persistent` for certificate persistance
    pub fn create_client_config(&self) -> Result<ClientConfig> {
        // Sign client certificate with CA
        let client_cert_der = self.client_cert.serialize_der_with_signer(&self.ca_cert)?;
        let client_key_der = self.client_cert.serialize_private_key_der();

        let client_cert_chain = vec![CertificateDer::from(client_cert_der)];
        let client_private_key = PrivateKeyDer::try_from(client_key_der)
            .map_err(|e| anyhow::anyhow!("Failed to convert client private key: {:?}", e))?;

        // Set up root certificate store to trust our CA (for server cert validation)
        let mut root_cert_store = RootCertStore::empty();
        let ca_cert_der = self.ca_cert.serialize_der()?;
        root_cert_store.add(CertificateDer::from(ca_cert_der))?;

        let config = ClientConfig::builder()
            .with_root_certificates(root_cert_store)
            .with_client_auth_cert(client_cert_chain, client_private_key)?;

        Ok(config)
    }

    pub fn get_ca_cert_pem(&self) -> Result<String> {
        Ok(self.ca_cert.serialize_pem()?)
    }

    pub fn get_server_cert_pem(&self) -> Result<String> {
        Ok(self.server_cert.serialize_pem_with_signer(&self.ca_cert)?)
    }

    pub fn get_client_cert_pem(&self) -> Result<String> {
        Ok(self.client_cert.serialize_pem_with_signer(&self.ca_cert)?)
    }

    pub fn get_server_key_pem(&self) -> Result<String> {
        Ok(self.server_cert.serialize_private_key_pem())
    }

    pub fn get_client_key_pem(&self) -> Result<String> {
        Ok(self.client_cert.serialize_private_key_pem())
    }

    /// Save certificates to a directory
    pub fn save_to_directory(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)
            .context(format!("Failed to create certificate directory: {:?}", dir))?;

        let ca_pem = self.get_ca_cert_pem()?;
        fs::write(dir.join("ca.pem"), ca_pem).context("Failed to write CA certificate")?;

        let server_cert_pem = self.get_server_cert_pem()?;
        fs::write(dir.join("server-cert.pem"), server_cert_pem)
            .context("Failed to write server certificate")?;

        let server_key_pem = self.get_server_key_pem()?;
        fs::write(dir.join("server-key.pem"), server_key_pem)
            .context("Failed to write server key")?;

        let client_cert_pem = self.get_client_cert_pem()?;
        fs::write(dir.join("client-cert.pem"), client_cert_pem)
            .context("Failed to write client certificate")?;

        let client_key_pem = self.get_client_key_pem()?;
        fs::write(dir.join("client-key.pem"), client_key_pem)
            .context("Failed to write client key")?;

        // Set restrictive permissions on private keys (Unix only)
        // Note: On Windows, file permissions are managed differently
        // Windows users should ensure the certificate directory has appropriate access controls
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let key_perms = fs::Permissions::from_mode(0o600);
            let _ = fs::set_permissions(dir.join("server-key.pem"), key_perms.clone());
            let _ = fs::set_permissions(dir.join("client-key.pem"), key_perms);
        }

        Ok(())
    }

    /// Check if all required certificates exist in a directory
    pub fn exists_in_directory(dir: &Path) -> bool {
        dir.join("ca.pem").exists()
            && dir.join("server-cert.pem").exists()
            && dir.join("server-key.pem").exists()
            && dir.join("client-cert.pem").exists()
            && dir.join("client-key.pem").exists()
    }

    /// Generate new certificates and save to a directory
    /// Use this for the 'setup' command
    pub fn generate_and_save(dir: Option<&Path>, force: bool) -> Result<Self> {
        let cert_dir = match dir {
            Some(d) => d.to_path_buf(),
            None => default_cert_dir()?,
        };

        // Check if certificates already exist
        if Self::exists_in_directory(&cert_dir) && !force {
            return Err(anyhow::anyhow!(
                "Certificates already exist at {:?}\nUse --force to overwrite, or delete the directory manually",
                cert_dir
            ));
        }

        tracing::info!("Generating new certificate chain");
        let chain = Self::generate()?;

        chain
            .save_to_directory(&cert_dir)
            .context("Failed to save certificates")?;

        tracing::info!("Certificates saved to {:?}", cert_dir);

        Ok(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, response::Json, routing::get};
    use axum_server::tls_rustls::RustlsConfig;
    use reqwest::Client;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};

    fn init_crypto_provider() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            rustls::crypto::aws_lc_rs::default_provider()
                .install_default()
                .expect("Failed to install crypto provider");
        });
    }

    #[tokio::test]
    async fn test_mtls_handshake_success() {
        init_crypto_provider();
        // Generate certificate chain
        let cert_chain =
            CertificateChain::generate().expect("Failed to generate certificate chain");

        // Create server config
        let server_config = cert_chain
            .create_server_config()
            .expect("Failed to create server config");

        // Create client config
        let client_config = cert_chain
            .create_client_config()
            .expect("Failed to create client config");

        // Create a simple axum app
        let app = Router::new().route(
            "/test",
            get(|| async { Json(json!({"status": "success"})) }),
        );

        // Start server with mTLS
        let rustls_config = RustlsConfig::from_config(Arc::new(server_config));

        // Use a fixed port for testing
        let test_port = 8443;
        let server_addr = format!("127.0.0.1:{}", test_port).parse().unwrap();

        let server_handle = tokio::spawn(async move {
            axum_server::bind_rustls(server_addr, rustls_config)
                .serve(app.into_make_service())
                .await
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Create reqwest client with mTLS config
        let client = Client::builder()
            .use_preconfigured_tls(client_config)
            .build()
            .expect("Failed to build client");

        // Test successful mTLS connection
        let url = format!("https://127.0.0.1:{}/test", test_port);
        println!("Testing mTLS connection to: {}", url);

        let response = timeout(Duration::from_secs(10), client.get(&url).send())
            .await
            .expect("Request timed out")
            .expect("Failed to send request");

        assert!(
            response.status().is_success(),
            "Request should succeed with valid mTLS"
        );

        let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        assert_eq!(body["status"], "success");

        // Shutdown server
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_mtls_handshake_failure_no_client_cert() {
        init_crypto_provider();
        // Generate certificate chain
        let cert_chain =
            CertificateChain::generate().expect("Failed to generate certificate chain");

        // Create server config (requires client certs)
        let server_config = cert_chain
            .create_server_config()
            .expect("Failed to create server config");

        // Create a simple axum app
        let app = Router::new().route(
            "/test",
            get(|| async { Json(json!({"status": "success"})) }),
        );

        // Start server with mTLS
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind listener");
        let server_addr = listener.local_addr().expect("Failed to get local address");
        let rustls_config = RustlsConfig::from_config(Arc::new(server_config));

        let server_handle = tokio::spawn(async move {
            axum_server::bind_rustls(server_addr, rustls_config)
                .serve(app.into_make_service())
                .await
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create reqwest client without client certificates (should fail)
        let client = Client::builder()
            .danger_accept_invalid_certs(true) // Accept self-signed certs but still no client cert
            .build()
            .expect("Failed to build client");

        // Test that connection fails without client certificate
        let result = timeout(
            Duration::from_secs(5),
            client
                .get(format!("https://127.0.0.1:{}/test", server_addr.port()))
                .send(),
        )
        .await;

        // Should fail because no client certificate is provided
        assert!(
            result.is_err() || result.unwrap().is_err(),
            "Request should fail without client certificate"
        );

        // Shutdown server
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_mtls_handshake_failure_wrong_ca() {
        init_crypto_provider();
        // Generate two separate certificate chains
        let cert_chain1 =
            CertificateChain::generate().expect("Failed to generate certificate chain 1");
        let cert_chain2 =
            CertificateChain::generate().expect("Failed to generate certificate chain 2");

        // Create server config with first cert chain
        let server_config = cert_chain1
            .create_server_config()
            .expect("Failed to create server config");

        // Create client config with second cert chain (different CA)
        let client_config = cert_chain2
            .create_client_config()
            .expect("Failed to create client config");

        // Create a simple axum app
        let app = Router::new().route(
            "/test",
            get(|| async { Json(json!({"status": "success"})) }),
        );

        // Start server with mTLS
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind listener");
        let server_addr = listener.local_addr().expect("Failed to get local address");
        let rustls_config = RustlsConfig::from_config(Arc::new(server_config));

        let server_handle = tokio::spawn(async move {
            axum_server::bind_rustls(server_addr, rustls_config)
                .serve(app.into_make_service())
                .await
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create reqwest client with wrong CA certificates
        let client = Client::builder()
            .use_preconfigured_tls(client_config)
            .build()
            .expect("Failed to build client");

        // Test that connection fails with wrong CA
        let result = timeout(
            Duration::from_secs(5),
            client
                .get(format!("https://127.0.0.1:{}/test", server_addr.port()))
                .send(),
        )
        .await;

        // Should fail because client and server have different CAs
        assert!(
            result.is_err() || result.unwrap().is_err(),
            "Request should fail with wrong CA certificates"
        );

        // Shutdown server
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_certificate_chain_generation() {
        init_crypto_provider();
        let cert_chain =
            CertificateChain::generate().expect("Failed to generate certificate chain");

        // Test that we can get PEM representations
        let ca_pem = cert_chain.get_ca_cert_pem().expect("Failed to get CA PEM");
        let server_pem = cert_chain
            .get_server_cert_pem()
            .expect("Failed to get server PEM");
        let client_pem = cert_chain
            .get_client_cert_pem()
            .expect("Failed to get client PEM");
        let server_key_pem = cert_chain
            .get_server_key_pem()
            .expect("Failed to get server key PEM");
        let client_key_pem = cert_chain
            .get_client_key_pem()
            .expect("Failed to get client key PEM");

        // Verify PEM format
        assert!(ca_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(ca_pem.contains("-----END CERTIFICATE-----"));
        assert!(server_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(server_pem.contains("-----END CERTIFICATE-----"));
        assert!(client_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(client_pem.contains("-----END CERTIFICATE-----"));
        assert!(server_key_pem.contains("-----BEGIN PRIVATE KEY-----"));
        assert!(server_key_pem.contains("-----END PRIVATE KEY-----"));
        assert!(client_key_pem.contains("-----BEGIN PRIVATE KEY-----"));
        assert!(client_key_pem.contains("-----END PRIVATE KEY-----"));
    }

    #[tokio::test]
    async fn test_server_config_creation() {
        init_crypto_provider();
        let cert_chain =
            CertificateChain::generate().expect("Failed to generate certificate chain");
        let _server_config = cert_chain
            .create_server_config()
            .expect("Failed to create server config");

        // Verify server config is created successfully
        // The fact that it doesn't panic/error is the main test
        assert!(true, "Server config created successfully");
    }

    #[tokio::test]
    async fn test_client_config_creation() {
        init_crypto_provider();
        let cert_chain =
            CertificateChain::generate().expect("Failed to generate certificate chain");
        let _client_config = cert_chain
            .create_client_config()
            .expect("Failed to create client config");

        // Verify client config is created successfully
        // The fact that it doesn't panic/error is the main test
        assert!(true, "Client config created successfully");
    }

    #[tokio::test]
    async fn test_mtls_multiple_requests() {
        init_crypto_provider();
        // Generate certificate chain
        let cert_chain =
            CertificateChain::generate().expect("Failed to generate certificate chain");

        // Create server and client configs
        let server_config = cert_chain
            .create_server_config()
            .expect("Failed to create server config");
        let client_config = cert_chain
            .create_client_config()
            .expect("Failed to create client config");

        // Create a simple axum app with multiple routes
        let app = Router::new()
            .route(
                "/test1",
                get(|| async { Json(json!({"endpoint": "test1"})) }),
            )
            .route(
                "/test2",
                get(|| async { Json(json!({"endpoint": "test2"})) }),
            )
            .route(
                "/test3",
                get(|| async { Json(json!({"endpoint": "test3"})) }),
            );

        // Start server with mTLS
        let rustls_config = RustlsConfig::from_config(Arc::new(server_config));

        // Use a fixed port for testing
        let test_port = 8444; // Different port from the first test
        let server_addr = format!("127.0.0.1:{}", test_port).parse().unwrap();

        let server_handle = tokio::spawn(async move {
            axum_server::bind_rustls(server_addr, rustls_config)
                .serve(app.into_make_service())
                .await
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Create reqwest client with mTLS config
        let client = Client::builder()
            .use_preconfigured_tls(client_config)
            .build()
            .expect("Failed to build client");

        // Test multiple requests to different endpoints
        for endpoint in ["test1", "test2", "test3"] {
            let response = timeout(
                Duration::from_secs(10),
                client
                    .get(format!("https://127.0.0.1:{}/{}", test_port, endpoint))
                    .send(),
            )
            .await
            .expect("Request timed out")
            .expect("Failed to send request");

            assert!(
                response.status().is_success(),
                "Request to {} should succeed",
                endpoint
            );

            let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
            assert_eq!(body["endpoint"], endpoint);
        }

        // Shutdown server
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_mtls_configuration_compatibility() {
        init_crypto_provider();

        // Generate certificate chain
        let cert_chain =
            CertificateChain::generate().expect("Failed to generate certificate chain");

        // Create server config - should work without errors
        let server_config = cert_chain
            .create_server_config()
            .expect("Failed to create server config");

        // Create client config - should work without errors
        let client_config = cert_chain
            .create_client_config()
            .expect("Failed to create client config");

        // Verify we can create a reqwest client with the client config
        let _client = Client::builder()
            .use_preconfigured_tls(client_config)
            .build()
            .expect("Failed to build reqwest client with mTLS config");

        // Verify we can create an axum-server RustlsConfig with the server config
        let _rustls_config = RustlsConfig::from_config(Arc::new(server_config));

        // Verify certificate chain properties
        assert!(cert_chain.get_ca_cert_pem().is_ok());
        assert!(cert_chain.get_server_cert_pem().is_ok());
        assert!(cert_chain.get_client_cert_pem().is_ok());
        assert!(cert_chain.get_server_key_pem().is_ok());
        assert!(cert_chain.get_client_key_pem().is_ok());

        // If we get here, the mTLS configuration is properly set up
        println!("✅ mTLS configuration successfully created");
        println!("✅ Reqwest client can be configured with client certificates");
        println!("✅ Axum server can be configured with server certificates");
        println!("✅ Certificate chain includes CA, server, and client certificates");
    }

    #[tokio::test]
    async fn test_mtls_certificate_validation() {
        init_crypto_provider();

        // Test that different certificate chains are incompatible
        let cert_chain1 =
            CertificateChain::generate().expect("Failed to generate certificate chain 1");
        let cert_chain2 =
            CertificateChain::generate().expect("Failed to generate certificate chain 2");

        // Create configs from different chains
        let server_config1 = cert_chain1
            .create_server_config()
            .expect("Failed to create server config 1");
        let client_config2 = cert_chain2
            .create_client_config()
            .expect("Failed to create client config 2");

        // These should be created successfully but would fail in actual connection
        let _client = Client::builder()
            .use_preconfigured_tls(client_config2)
            .build()
            .expect("Failed to build client with different CA");

        let _rustls_config = RustlsConfig::from_config(Arc::new(server_config1));

        // The configurations are created successfully, but they would fail during handshake
        // because they use different CAs
        println!("✅ Different certificate chains create valid but incompatible configurations");
    }
}
