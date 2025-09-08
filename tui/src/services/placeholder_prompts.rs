use crate::services::textarea::TextArea;
use std::sync::OnceLock;

/// Example prompts that showcase Stakpak's strengths
const STAKPAK_PROMPTS: &[&str] = &[
    "Create an eks cluster",
    "Create a deployment and service for an app",
    "Dockerize my app and deploy it to a kubernetes cluster",
    "Set up a CI/CD pipeline for this Node.js application with automated testing",
    "Help me deploy my python app with github actions on aws lambd",
];

/// Example shell commands for shell mode
const SHELL_PROMPTS: &[&str] = &[" "];

// Generate a random index once per session
static STAKPAK_INDEX: OnceLock<usize> = OnceLock::new();
static SHELL_INDEX: OnceLock<usize> = OnceLock::new();

pub fn get_placeholder_prompt(textarea: &TextArea) -> &'static str {
    if textarea.is_shell_mode() {
        // Return a random shell prompt (selected once per session)
        let index = SHELL_INDEX.get_or_init(|| {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            use std::time::{SystemTime, UNIX_EPOCH};

            let mut hasher = DefaultHasher::new();
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut hasher);
            hasher.finish() as usize % SHELL_PROMPTS.len()
        });
        SHELL_PROMPTS[*index]
    } else {
        // Return a random Stakpak prompt (selected once per session)
        let index = STAKPAK_INDEX.get_or_init(|| {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            use std::time::{SystemTime, UNIX_EPOCH};

            let mut hasher = DefaultHasher::new();
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut hasher);
            hasher.finish() as usize % STAKPAK_PROMPTS.len()
        });
        STAKPAK_PROMPTS[*index]
    }
}
