//! Telemetry module for anonymous usage tracking
//!
//! This module provides integration for tracking local provider usage.
//! Telemetry is opt-out and collects no personal data, prompts, or session content.

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

pub fn capture_event(
    anonymous_id: &str,
    machine_name: Option<&str>,
    enabled: bool,
    event: TelemetryEvent,
) {
    if !enabled {
        return;
    }

    let payload = TelemetryPayload {
        event: event.to_string(),
        machine_name: machine_name.unwrap_or("").to_string(),
        provider: "Local".to_string(),
        user_id: anonymous_id.to_string(),
    };

    tokio::spawn(async move {
        let client = match crate::tls_client::create_tls_client(
            crate::tls_client::TlsClientConfig::default(),
        ) {
            Ok(c) => c,
            Err(_) => return,
        };
        let _ = client.post(TELEMETRY_ENDPOINT).json(&payload).send().await;
    });
}
