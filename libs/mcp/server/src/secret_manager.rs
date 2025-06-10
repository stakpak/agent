use serde_json;
use stakpak_shared::local_store::LocalStore;
use stakpak_shared::secrets::{redact_secrets, restore_secrets};
use std::collections::HashMap;
use tracing::{error, warn};

/// Handles secret redaction and restoration across different tool types
#[derive(Clone)]
pub struct SecretManager {
    redact_secrets: bool,
}

impl SecretManager {
    pub fn new(redact_secrets: bool) -> Self {
        Self { redact_secrets }
    }

    /// Load the redaction map from the session file
    pub fn load_session_redaction_map(&self) -> HashMap<String, String> {
        match LocalStore::read_session_data("secrets.json") {
            Ok(content) => {
                if content.trim().is_empty() {
                    return HashMap::new();
                }

                match serde_json::from_str::<HashMap<String, String>>(&content) {
                    Ok(map) => map,
                    Err(e) => {
                        error!("Failed to parse session redaction map JSON: {}", e);
                        HashMap::new()
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read session redaction map file: {}", e);
                HashMap::new()
            }
        }
    }

    /// Save the redaction map to the session file
    pub fn save_session_redaction_map(&self, redaction_map: &HashMap<String, String>) {
        match serde_json::to_string_pretty(redaction_map) {
            Ok(json_content) => {
                if let Err(e) = LocalStore::write_session_data("secrets.json", &json_content) {
                    error!("Failed to save session redaction map: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to serialize session redaction map to JSON: {}", e);
            }
        }
    }

    /// Add new redactions to the session map
    pub fn add_to_session_redaction_map(&self, new_redactions: &HashMap<String, String>) {
        if new_redactions.is_empty() {
            return;
        }

        let mut existing_map = self.load_session_redaction_map();
        existing_map.extend(new_redactions.clone());
        self.save_session_redaction_map(&existing_map);
    }

    /// Restore secrets in a string using the session redaction map
    pub fn restore_secrets_in_string(&self, input: &str) -> String {
        let redaction_map = self.load_session_redaction_map();
        if redaction_map.is_empty() {
            return input.to_string();
        }
        restore_secrets(input, &redaction_map)
    }

    /// Redact secrets and add to session map
    pub fn redact_and_store_secrets(&self, content: &str, path: Option<&str>) -> String {
        if !self.redact_secrets {
            return content.to_string();
        }

        // TODO: this is not thread safe, we need to use a mutex or an actor to protect the redaction map
        let existing_redaction_map = self.load_session_redaction_map();
        let redaction_result = redact_secrets(content, path, &existing_redaction_map);

        // Add new redactions to session map
        self.add_to_session_redaction_map(&redaction_result.redaction_map);

        redaction_result.redacted_string
    }
}
