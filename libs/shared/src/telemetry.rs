//! Telemetry module for anonymous usage tracking
//!
//! This module provides integration for tracking local provider usage.
//! Telemetry is OPT-IN by default and collects no personal data, prompts, or session content.
//! 
//! To enable telemetry, users must explicitly set `collect_telemetry = true` in their configuration.
//! This is a security-by-default design to protect user privacy and sovereignty.

use serde::Serialize;
use std::fmt;

const TELEMETRY_ENDPOINT: &str = "https://apiv2.stakpak.dev/v1/telemetry";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TelemetryEvent {
    FirstOpen,
    UserPrompted,
}

impl fmt::Display for TelemetryEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TelemetryEvent::FirstOpen => write!(f, "FirstOpen"),
            TelemetryEvent::UserPrompted => write!(f, "UserPrompted"),
        }
    }
}

#[derive(Serialize)]
struct TelemetryPayload {
    event: String,
    machine_name: String,
    provider: String,
    user_id: String,
}

/// Captures a telemetry event ONLY if explicitly enabled by the user.
/// 
/// # Privacy Guarantee
/// This function acts as a final gatekeeper - even if called, it will NOT send data
/// unless ALL of the following are true:
/// 1. `enabled` flag is true (user opted in via config)
/// 2. STAKPAK_ENABLE_TELEMETRY=1 (environment variable opt-in)
/// 3. STAKPAK_SEND_MACHINE_ID=1 (optional machine fingerprint opt-in)
/// 4. STAKPAK_SEND_ANON_ID=1 (optional anonymous_id opt-in)
/// 
/// # Parameters
/// - `anonymous_id`: Unique identifier for the session (only sent if STAKPAK_SEND_ANON_ID=1)
/// - `machine_name`: Optional machine identifier (only sent if STAKPAK_SEND_MACHINE_ID=1)
/// - `enabled`: **MUST be explicitly set to true by user** to allow collection
/// - `event`: The telemetry event to capture
/// 
/// # Sovereignty Protection
/// Returns early without any network call if any guard is false, ensuring
/// no data leaves the local machine unless the user has explicitly opted in via BOTH
/// configuration AND environment variable.
pub fn capture_event(
    anonymous_id: &str,
    machine_name: Option<&str>,
    enabled: bool,
    event: TelemetryEvent,
) {
    // ENV VAR GATE: Must be explicitly enabled via environment variable
    if !is_telemetry_env_enabled() {
        tracing::debug!("Telemetry blocked by STAKPAK_ENABLE_TELEMETRY guard");
        return;
    }

    // CONFIG GATE: User must have opted in via config
    if !enabled {
        tracing::debug!("Telemetry blocked by config.collect_telemetry=false");
        return;
    }

    // BUILD PAYLOAD WITH OPTIONAL FIELDS
    let machine_name_value = if std::env::var("STAKPAK_SEND_MACHINE_ID")
        .unwrap_or_else(|_| "0".to_string())
        .eq_ignore_ascii_case("1")
    {
        machine_name.unwrap_or("anonymous").to_string()
    } else {
        "anonymous".to_string() // Always send "anonymous" if not opted in
    };

    let user_id_value = if std::env::var("STAKPAK_SEND_ANON_ID")
        .unwrap_or_else(|_| "0".to_string())
        .eq_ignore_ascii_case("1")
    {
        anonymous_id.to_string()
    } else {
        "anonymous".to_string() // Always send "anonymous" if not opted in
    };

    let payload = TelemetryPayload {
        event: event.to_string(),
        machine_name: machine_name_value,
        provider: "Local".to_string(),
        user_id: user_id_value,
    };

    // Async fire-and-forget - but only if user explicitly enabled telemetry
    tokio::spawn(async move {
        let client = match crate::tls_client::create_tls_client(
            crate::tls_client::TlsClientConfig::default(),
        ) {
            Ok(c) => c,
            Err(_) => return, // Silently fail if TLS client creation fails
        };
        let _ = client.post(TELEMETRY_ENDPOINT).json(&payload).send().await;
    });
}

/// Check if telemetry is enabled (for internal use)
/// Returns false by default unless explicitly configured
pub fn is_telemetry_enabled(config_collect_telemetry: Option<bool>) -> bool {
    // DEFAULT: Opt-out is false, opt-in must be explicit
    config_collect_telemetry.unwrap_or(false)
}

/// Check if telemetry is enabled via environment variable
pub fn is_telemetry_env_enabled() -> bool {
    std::env::var("STAKPAK_ENABLE_TELEMETRY")
        .unwrap_or_else(|_| "0".to_string())
        .eq_ignore_ascii_case("1")
}