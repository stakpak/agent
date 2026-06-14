use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AutoApproveOverride {
    /// "all" | "none"
    Mode(String),
    /// Explicit allowlist for auto-approval.
    AllowList(Vec<String>),
}

/// Per-request run overrides merged with runtime defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<AutoApproveOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<usize>,
    /// Override the model's context window size (in tokens).
    /// When set, this value replaces the model's default context window
    /// for budget and trimming calculations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Fraction of the context window at which context trimming triggers.
    /// Range: 0.0–1.0 (e.g. 0.8 = start trimming at 80% of context window).
    /// Default when not set: 0.8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_budget_threshold: Option<f32>,
    /// Number of most recent assistant messages to keep untrimmed when
    /// context trimming is triggered. Only assistant and tool messages are
    /// trimmed; user and system messages are always preserved in full.
    /// Default when not set: 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_last_n_assistant_messages: Option<usize>,
}

impl RunOverrides {
    pub fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.auto_approve.is_none()
            && self.system_prompt.is_none()
            && self.max_turns.is_none()
            && self.context_window.is_none()
            && self.context_budget_threshold.is_none()
            && self.keep_last_n_assistant_messages.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::{AutoApproveOverride, RunOverrides};

    #[test]
    fn run_overrides_is_empty_only_when_all_fields_absent() {
        let empty = RunOverrides::default();
        assert!(empty.is_empty());

        let with_model = RunOverrides {
            model: Some("openai/gpt-4o-mini".to_string()),
            ..RunOverrides::default()
        };
        assert!(!with_model.is_empty());

        let with_allowlist = RunOverrides {
            auto_approve: Some(AutoApproveOverride::AllowList(vec!["view".to_string()])),
            ..RunOverrides::default()
        };
        assert!(!with_allowlist.is_empty());
    }

    #[test]
    fn run_overrides_serde_round_trip() {
        let overrides = RunOverrides {
            model: Some("anthropic/claude-sonnet-4-5".to_string()),
            auto_approve: Some(AutoApproveOverride::AllowList(vec![
                "view".to_string(),
                "search_docs".to_string(),
            ])),
            system_prompt: Some("hello".to_string()),
            max_turns: Some(24),
            context_window: Some(200_000),
            context_budget_threshold: Some(0.7),
            keep_last_n_assistant_messages: Some(10),
        };

        let encoded = serde_json::to_string(&overrides).expect("serialize overrides");
        let decoded: RunOverrides = serde_json::from_str(&encoded).expect("deserialize overrides");
        assert_eq!(decoded, overrides);
    }

    #[test]
    fn run_overrides_partial_context_fields() {
        // Verify PartialEq works correctly when only some fields are set
        let a = RunOverrides {
            context_window: Some(100_000),
            ..RunOverrides::default()
        };
        let b = RunOverrides {
            context_window: Some(100_000),
            ..RunOverrides::default()
        };
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }
}
