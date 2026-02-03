//! Local storage and hook infrastructure
//!
//! This module provides:
//! - Database operations for local session storage
//! - Lifecycle hooks for context management
//! - Model configuration types

use stakpak_shared::models::integrations::openai::AgentModel;
use stakpak_shared::models::llm::LLMModel;

// Sub-modules
pub(crate) mod context_managers;
pub mod hooks;
pub mod migrations;
pub mod storage;

#[cfg(test)]
mod tests;

/// Model options for the agent
#[derive(Clone, Debug, Default)]
pub struct ModelOptions {
    pub smart_model: Option<LLMModel>,
    pub eco_model: Option<LLMModel>,
    pub recovery_model: Option<LLMModel>,
}

/// Resolved model set with default fallbacks
#[derive(Clone, Debug)]
pub struct ModelSet {
    pub smart_model: LLMModel,
    pub eco_model: LLMModel,
    pub recovery_model: LLMModel,
}

impl ModelSet {
    /// Get the model for a given agent model type
    pub fn get_model(&self, agent_model: &AgentModel) -> LLMModel {
        match agent_model {
            AgentModel::Smart => self.smart_model.clone(),
            AgentModel::Eco => self.eco_model.clone(),
            AgentModel::Recovery => self.recovery_model.clone(),
        }
    }
}

impl From<ModelOptions> for ModelSet {
    fn from(value: ModelOptions) -> Self {
        // Default models route through Stakpak provider
        let smart_model = value
            .smart_model
            .unwrap_or_else(|| LLMModel::from("stakpak/anthropic/claude-opus-4-5".to_string()));
        let eco_model = value
            .eco_model
            .unwrap_or_else(|| LLMModel::from("stakpak/anthropic/claude-haiku-4-5".to_string()));
        let recovery_model = value
            .recovery_model
            .unwrap_or_else(|| LLMModel::from("stakpak/openai/gpt-4o".to_string()));

        Self {
            smart_model,
            eco_model,
            recovery_model,
        }
    }
}
