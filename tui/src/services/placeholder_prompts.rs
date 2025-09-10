use crate::services::textarea::TextArea;
use std::sync::OnceLock;

/// Example prompts that showcase Stakpak's strengths
const STAKPAK_PROMPTS: &[&str] = &[
    "Dockerize my app",
    "Create github actions workflow to automate building and deploying my app on ECS",
    "Load test my service to right-size the resources needed",
    "Analyze the costs of my cloud account",
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
