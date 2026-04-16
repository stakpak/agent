//! Shared OpenAI runtime profile types.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialBackendProfile {
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibleBackendProfile {
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexBackendProfile {
    pub base_url: String,
    pub originator: String,
    pub chatgpt_account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAIBackendProfile {
    Official(OfficialBackendProfile),
    Codex(CodexBackendProfile),
    Compatible(CompatibleBackendProfile),
}

impl OpenAIBackendProfile {
    pub fn base_url(&self) -> &str {
        match self {
            Self::Official(profile) => &profile.base_url,
            Self::Codex(profile) => &profile.base_url,
            Self::Compatible(profile) => &profile.base_url,
        }
    }
}
