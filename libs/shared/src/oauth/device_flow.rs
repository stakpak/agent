//! Device Authorization Grant (RFC 8628)
//!
//! Generic implementation of the OAuth 2.0 Device Authorization Grant.  The
//! flow has two stages:
//!
//! 1. **Request** — POST to the provider's device-code endpoint.  Returns a
//!    `device_code`, `user_code`, `verification_uri` and a polling `interval`.
//! 2. **Poll** — Repeatedly POST to the token endpoint until the user has
//!    authorised the device, then store the returned access token.
//!
//! Reference: <https://www.rfc-editor.org/rfc/rfc8628>

use super::error::{OAuthError, OAuthResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The initial response from the device-code endpoint
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    /// Opaque code used in the polling phase
    pub device_code: String,
    /// Short code the user types on the verification page
    pub user_code: String,
    /// URL the user should visit
    pub verification_uri: String,
    /// Total time (seconds) the device code is valid
    pub expires_in: u64,
    /// Minimum seconds between poll attempts
    pub interval: u64,
}

/// Successful token response from the polling endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTokenResponse {
    /// GitHub OAuth access token
    pub access_token: String,
    /// Token type (usually "bearer")
    pub token_type: String,
    /// Space-separated list of granted scopes
    #[serde(default)]
    pub scope: String,
}

/// Current state of an in-progress device flow
#[derive(Debug, Clone)]
pub enum DeviceFlowState {
    /// Waiting for the user to complete authorisation
    Pending {
        user_code: String,
        verification_uri: String,
    },
    /// Authorisation complete; contains the access token
    Completed(DeviceTokenResponse),
}

#[derive(Debug, Deserialize)]
struct PollRaw {
    // success fields
    access_token: Option<String>,
    token_type: Option<String>,
    #[serde(default)]
    scope: String,
    // error fields
    error: Option<String>,
    error_description: Option<String>,
}

/// Manages a complete RFC 8628 device-flow OAuth session.
///
/// Provider-agnostic: supply the device-code URL and token URL for any
/// provider that supports the Device Authorization Grant.
pub struct DeviceFlow {
    client_id: String,
    scopes: Vec<String>,
    device_code_url: String,
    token_url: String,
    /// Reused across all HTTP calls in the flow (avoids re-creating TLS on every poll)
    client: Client,
}

impl DeviceFlow {
    /// Create a new device flow for the given OAuth app and provider endpoints.
    ///
    /// - `device_code_url`: the provider's device-authorization endpoint
    /// - `token_url`: the provider's token endpoint used during polling
    ///
    /// Returns an error if the underlying TLS client cannot be constructed.
    pub fn new(
        client_id: impl Into<String>,
        scopes: Vec<String>,
        device_code_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> OAuthResult<Self> {
        let client =
            crate::tls_client::create_tls_client(crate::tls_client::TlsClientConfig::default())
                .map_err(OAuthError::token_exchange_failed)?;
        Ok(Self {
            client_id: client_id.into(),
            scopes,
            device_code_url: device_code_url.into(),
            token_url: token_url.into(),
            client,
        })
    }

    /// Step 1 — request a device code from the provider.
    ///
    /// Returns the `DeviceCodeResponse` that you should present to the user
    /// (display `user_code` and `verification_uri`).
    pub async fn request_device_code(&self) -> OAuthResult<DeviceCodeResponse> {
        let scope = self.scopes.join(" ");

        let response = self
            .client
            .post(&self.device_code_url)
            .header("Accept", "application/json")
            .form(&[("client_id", self.client_id.as_str()), ("scope", &scope)])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(OAuthError::token_exchange_failed(format!(
                "Device code request failed: HTTP {} — {}",
                status, body
            )));
        }

        response.json::<DeviceCodeResponse>().await.map_err(|e| {
            OAuthError::token_exchange_failed(format!(
                "Failed to parse device code response: {}",
                e
            ))
        })
    }

    /// Step 2 — poll the provider until the user has authorised the device.
    ///
    /// Automatically respects the `interval` returned by step 1 and handles
    /// `slow_down` responses (which add 5 s to the current interval per spec).
    ///
    /// Returns `Ok(DeviceTokenResponse)` once the user approves the request.
    pub async fn poll_for_token(
        &self,
        device_code: &DeviceCodeResponse,
    ) -> OAuthResult<DeviceTokenResponse> {
        let mut interval_secs = device_code.interval;
        let expires_at = std::time::Instant::now() + Duration::from_secs(device_code.expires_in);

        loop {
            if std::time::Instant::now() >= expires_at {
                return Err(OAuthError::token_exchange_failed(
                    "Device code expired before the user completed authorisation",
                ));
            }

            let response = self
                .client
                .post(&self.token_url)
                .header("Accept", "application/json")
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("device_code", device_code.device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(OAuthError::token_exchange_failed(format!(
                    "Token polling failed: HTTP {} — {}",
                    status, body
                )));
            }

            let poll_raw: PollRaw = response.json().await.map_err(|e| {
                OAuthError::token_exchange_failed(format!(
                    "Failed to parse token poll response: {}",
                    e
                ))
            })?;

            // Error field takes priority — check it before looking at token fields.
            if let Some(ref err) = poll_raw.error {
                match err.as_str() {
                    // Normal — user hasn't approved yet; wait then retry
                    "authorization_pending" => {}
                    // Provider asked us to back off — add 5 s per RFC 8628 §3.5
                    "slow_down" => {
                        interval_secs += 5;
                    }
                    // Terminal errors — stop immediately
                    "access_denied" => {
                        return Err(OAuthError::token_exchange_failed(
                            "User denied the authorisation request",
                        ));
                    }
                    "expired_token" | "token_expired" => {
                        return Err(OAuthError::token_exchange_failed("Device code expired"));
                    }
                    "unsupported_grant_type" => {
                        return Err(OAuthError::token_exchange_failed(
                            "Unsupported grant type — grant_type must be \
                             urn:ietf:params:oauth:grant-type:device_code",
                        ));
                    }
                    "incorrect_client_credentials" => {
                        return Err(OAuthError::token_exchange_failed(
                            "Incorrect client credentials — check the client_id",
                        ));
                    }
                    "incorrect_device_code" => {
                        return Err(OAuthError::token_exchange_failed(
                            "The device_code provided is not valid",
                        ));
                    }
                    "device_flow_disabled" => {
                        return Err(OAuthError::token_exchange_failed(
                            "Device flow is not enabled for this OAuth app",
                        ));
                    }
                    other => {
                        return Err(OAuthError::token_exchange_failed(format!(
                            "Unexpected error from provider: {} — {}",
                            other,
                            poll_raw.error_description.as_deref().unwrap_or("")
                        )));
                    }
                }
            } else if let Some(access_token) = poll_raw.access_token {
                let token_type = poll_raw.token_type.unwrap_or_default();
                return Ok(DeviceTokenResponse {
                    access_token,
                    token_type,
                    scope: poll_raw.scope,
                });
            } else {
                return Err(OAuthError::token_exchange_failed(
                    "Token poll response contained neither an error nor an access token",
                ));
            }

            // RFC 8628 only requires the minimum gap between requests.
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
    }
}
