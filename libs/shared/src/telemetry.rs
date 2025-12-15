//! Telemetry module for anonymous usage tracking
//!
//! This module provides PostHog integration for tracking local provider usage.
//! Telemetry is opt-out and collects no personal data, prompts, or session content.

use serde::Serialize;
use std::fmt;

const POSTHOG_API_KEY: &str = "phc_QA5vkh1LnITsEmIhDeSZ2cE8veaBdpUKceWa3b9X3K9";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TelemetryEvent {
    FirstOpen,
    UserPrompted,
}

impl fmt::Display for TelemetryEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TelemetryEvent::FirstOpen => write!(f, "local_provider_first_open"),
            TelemetryEvent::UserPrompted => write!(f, "local_provider_user_prompted"),
        }
    }
}

pub fn capture_event(
    user_id: &str,
    machine_name: Option<&str>,
    enabled: bool,
    event: TelemetryEvent,
) {
    if !enabled {
        return;
    }

    let user_id = user_id.to_string();
    let machine_name = machine_name.map(|s| s.to_string());
    let event_name = event.to_string();

    tokio::spawn(async move {
        let client = posthog_rs::client(POSTHOG_API_KEY).await;
        let mut posthog_event = posthog_rs::Event::new(event_name, user_id);
        posthog_event.insert_prop("provider", "local").unwrap();

        if let Some(name) = machine_name
            && posthog_event.insert_prop("machine_name", name).is_err()
        {
            return;
        }

        let _ = client.capture(posthog_event).await;
    });
}
