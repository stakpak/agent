use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentError {
    BadRequest(BadRequestErrorMessage),
    InternalError,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::BadRequest(e) => write!(f, "Bad Request: {:?}", e),
            AgentError::InternalError => write!(f, "Internal Error"),
        }
    }
}

impl std::error::Error for AgentError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BadRequestErrorMessage {
    ApiError(String),
    InvalidAgentInput(String),
}
