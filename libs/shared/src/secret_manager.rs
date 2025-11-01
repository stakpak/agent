use crate::local_store::LocalStore;
use crate::secrets::{redact_password, redact_secrets, restore_secrets};
use serde_json;
use std::collections::HashMap;
use tracing::{error, warn};

/// Handles secret redaction and restoration across different tool types
#[derive(Clone)]
pub struct SecretManager {
    redact_secrets: bool,
    privacy_mode: bool,
}

impl SecretManager {
    pub fn new(redact_secrets: bool, privacy_mode: bool) -> Self {
        Self {
            redact_secrets,
            privacy_mode,
        }
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
                eprintln!("[DEBUG] Attempting to write session secrets file with {} entries", redaction_map.len());
                match LocalStore::write_session_data("secrets.json", &json_content) {
                    Ok(path) => {
                        eprintln!("[DEBUG] Successfully wrote secrets to: {}", path);
                    }
                    Err(e) => {
                        eprintln!("[ERROR] Failed to save session redaction map: {}", e);
                        error!("Failed to save session redaction map: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("[ERROR] Failed to serialize session redaction map to JSON: {}", e);
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
        eprintln!("[DEBUG] redact_and_store_secrets called with content: '{}'", content);
        eprintln!("[DEBUG] Loaded session map with {} entries", existing_redaction_map.len());
        for (key, val) in &existing_redaction_map {
            eprintln!("[DEBUG]   Map entry: {} -> {}", key, val);
        }
        let redaction_result =
            redact_secrets(content, path, &existing_redaction_map, self.privacy_mode);
        eprintln!("[DEBUG] After redact_secrets, result: '{}'", redaction_result.redacted_string);

        // Add new redactions to session map
        self.add_to_session_redaction_map(&redaction_result.redaction_map);

        redaction_result.redacted_string
    }

    pub fn redact_and_store_password(&self, content: &str, password: &str) -> String {
        if !self.redact_secrets {
            return content.to_string();
        }

        // TODO: this is not thread safe, we need to use a mutex or an actor to protect the redaction map
        let existing_redaction_map = self.load_session_redaction_map();
        eprintln!("[DEBUG] redact_and_store_password called with password: '{}'", password);
        eprintln!("[DEBUG] Existing redaction map has {} entries", existing_redaction_map.len());
        let redaction_result = redact_password(content, password, &existing_redaction_map);
        eprintln!("[DEBUG] After redact_password, map has {} entries", redaction_result.redaction_map.len());

        // Add new redactions to session map
        self.add_to_session_redaction_map(&redaction_result.redaction_map);
        eprintln!("[DEBUG] Saved redaction map to session file");

        redaction_result.redacted_string
    }
}
