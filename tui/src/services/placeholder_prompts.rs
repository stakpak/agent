use crate::services::textarea::TextArea;

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

pub fn get_placeholder_prompt(textarea: &TextArea) -> &'static str {
    if textarea.is_shell_mode() {
        // Return a random shell prompt
        let index = (textarea.text().len() + textarea.cursor()) % SHELL_PROMPTS.len();
        SHELL_PROMPTS[index]
    } else {
        // Return a random Stakpak prompt
        let index = (textarea.text().len() + textarea.cursor()) % STAKPAK_PROMPTS.len();
        STAKPAK_PROMPTS[index]
    }
}
