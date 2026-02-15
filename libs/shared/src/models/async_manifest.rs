//! Async agent manifest types for subagent JSON output parsing.
//!
//! These types represent the JSON output produced by async agent runs
//! and provide formatting for human/LLM consumption.

use crate::models::integrations::openai::ToolCall;
use crate::models::llm::LLMTokenUsage;
use serde::{Deserialize, Serialize};

/// Why an async agent paused execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum PauseReason {
    /// One or more tool calls require approval before execution.
    #[serde(rename = "tool_approval_required")]
    ToolApprovalRequired {
        pending_tool_calls: Vec<PendingToolCall>,
    },
    /// The agent responded with text only (asking a question or requesting input).
    #[serde(rename = "input_required")]
    InputRequired,
}

/// A tool call pending approval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

impl From<&ToolCall> for PendingToolCall {
    fn from(tc: &ToolCall) -> Self {
        let arguments = serde_json::from_str(&tc.function.arguments)
            .unwrap_or(serde_json::Value::String(tc.function.arguments.clone()));
        PendingToolCall {
            id: tc.id.clone(),
            name: tc.function.name.clone(),
            arguments,
        }
    }
}

/// Unified JSON output for async agent runs (both pause and completion).
/// All fields are always present for consistent parsing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AsyncManifest {
    /// "paused" or "completed"
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Model ID used for this execution (e.g., "claude-sonnet-4-5-20250929").
    #[serde(default)]
    pub model: String,
    /// The agent's text response (if any) in this execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<String>,
    /// Steps taken in this execution (current run only).
    #[serde(default)]
    pub steps: usize,
    /// Total steps across all executions in this session (including resumed runs).
    #[serde(default)]
    pub total_steps: usize,
    /// Token usage for this execution only.
    #[serde(default)]
    pub usage: LLMTokenUsage,
    /// Present when outcome is "paused" — why the agent paused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
    /// Present when outcome is "paused" — CLI command hint to resume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_hint: Option<String>,
}

impl AsyncManifest {
    /// Try to parse a string as an AsyncManifest.
    /// Returns None if the string is not valid JSON or doesn't match the manifest structure.
    pub fn try_parse(output: &str) -> Option<Self> {
        let trimmed = output.trim();

        // Direct parse attempt
        if let Ok(manifest) = serde_json::from_str::<AsyncManifest>(trimmed) {
            return Some(manifest);
        }

        // Try to find JSON object in the output
        if let Some(start) = trimmed.find('{')
            && let Some(end) = trimmed.rfind('}')
            && end > start
        {
            let json_str = &trimmed[start..=end];
            if let Ok(manifest) = serde_json::from_str::<AsyncManifest>(json_str) {
                return Some(manifest);
            }
        }

        None
    }
}

impl std::fmt::Display for AsyncManifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Status line with icon
        let (status_icon, status_text) = match self.outcome.as_str() {
            "completed" => ("✓", "Completed"),
            "paused" => ("⏸", "Paused"),
            _ => ("✗", "Failed"),
        };

        writeln!(f, "## Subagent Result: {} {}\n", status_icon, status_text)?;

        // Execution stats (compact)
        write!(f, "**Steps**: {}", self.steps)?;
        if self.total_steps > self.steps {
            write!(f, " (total: {})", self.total_steps)?;
        }
        if !self.model.is_empty() {
            write!(f, " | **Model**: {}", self.model)?;
        }
        writeln!(f, "\n")?;

        // Main content: agent message
        if let Some(ref message) = self.agent_message
            && !message.trim().is_empty()
        {
            writeln!(f, "### Response:\n{}\n", message.trim())?;
        }

        // Pause-specific information
        if let Some(ref pause_reason) = self.pause_reason {
            match pause_reason {
                PauseReason::ToolApprovalRequired { pending_tool_calls } => {
                    writeln!(f, "### Pending Tool Calls (awaiting approval):")?;
                    for tc in pending_tool_calls {
                        let display_name = tc.name.split("__").last().unwrap_or(&tc.name);
                        writeln!(f, "- {} (id: `{}`)", display_name, tc.id)?;

                        if !tc.arguments.is_null()
                            && let Some(obj) = tc.arguments.as_object()
                        {
                            for (key, value) in obj {
                                let value_str = match value {
                                    serde_json::Value::String(s) if s.len() > 100 => {
                                        // Find a valid UTF-8 boundary near 100 chars
                                        let truncate_at = s
                                            .char_indices()
                                            .take_while(|(i, _)| *i < 100)
                                            .last()
                                            .map(|(i, c)| i + c.len_utf8())
                                            .unwrap_or(0);
                                        format!("\"{}...\"", &s[..truncate_at])
                                    }
                                    serde_json::Value::String(s) => format!("\"{}\"", s),
                                    _ => value.to_string(),
                                };
                                writeln!(f, "  - {}: {}", key, value_str)?;
                            }
                        }
                    }
                    writeln!(f)?;
                }
                PauseReason::InputRequired => {
                    writeln!(f, "### Status: Awaiting Input")?;
                    writeln!(f, "The subagent is waiting for user input to continue.\n")?;
                }
            }

            if let Some(ref hint) = self.resume_hint {
                writeln!(f, "**Resume hint**: `{}`", hint)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_completed() {
        let manifest = AsyncManifest {
            outcome: "completed".to_string(),
            checkpoint_id: Some("abc123".to_string()),
            session_id: Some("sess456".to_string()),
            model: "claude-haiku-4-5".to_string(),
            agent_message: Some("Found 3 config files in /etc".to_string()),
            steps: 5,
            total_steps: 5,
            usage: LLMTokenUsage::default(),
            pause_reason: None,
            resume_hint: None,
        };

        let output = manifest.to_string();
        assert!(output.contains("✓ Completed"));
        assert!(output.contains("**Steps**: 5"));
        assert!(output.contains("claude-haiku-4-5"));
        assert!(output.contains("Found 3 config files"));
        // Should NOT contain checkpoint/session IDs (those are metadata)
        assert!(!output.contains("abc123"));
        assert!(!output.contains("sess456"));
    }

    #[test]
    fn test_display_paused() {
        let manifest = AsyncManifest {
            outcome: "paused".to_string(),
            checkpoint_id: Some("abc123".to_string()),
            session_id: None,
            model: "claude-haiku-4-5".to_string(),
            agent_message: Some("I need to run a command".to_string()),
            steps: 3,
            total_steps: 3,
            usage: LLMTokenUsage::default(),
            pause_reason: Some(PauseReason::ToolApprovalRequired {
                pending_tool_calls: vec![PendingToolCall {
                    id: "tc_001".to_string(),
                    name: "stakpak__run_command".to_string(),
                    arguments: serde_json::json!({"command": "ls -la"}),
                }],
            }),
            resume_hint: Some("stakpak -c abc123 --approve tc_001".to_string()),
        };

        let output = manifest.to_string();
        assert!(output.contains("⏸ Paused"));
        assert!(output.contains("Pending Tool Calls"));
        assert!(output.contains("run_command")); // Should strip stakpak__ prefix
        assert!(output.contains("tc_001"));
        assert!(output.contains("Resume hint"));
    }

    #[test]
    fn test_try_parse() {
        let json = r#"{
            "outcome": "completed",
            "model": "claude-haiku-4-5",
            "agent_message": "Done!",
            "steps": 2,
            "total_steps": 2,
            "usage": {"prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150}
        }"#;

        let manifest = AsyncManifest::try_parse(json).expect("Should parse valid JSON");
        assert_eq!(manifest.outcome, "completed");
        assert_eq!(manifest.steps, 2);
        assert_eq!(manifest.agent_message, Some("Done!".to_string()));
    }

    #[test]
    fn test_try_parse_with_surrounding_text() {
        let output = r#"Some log output here
{"outcome": "completed", "model": "test", "steps": 1, "total_steps": 1, "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}}
More text after"#;

        let manifest = AsyncManifest::try_parse(output).expect("Should find JSON in text");
        assert_eq!(manifest.outcome, "completed");
    }

    #[test]
    fn test_try_parse_invalid() {
        assert!(AsyncManifest::try_parse("not json").is_none());
        assert!(AsyncManifest::try_parse("{}").is_none()); // Missing required fields
    }

    #[test]
    fn test_json_structure_for_pause_reason() {
        // Verify the JSON structure matches what local_tools.rs expects to parse
        let manifest = AsyncManifest {
            outcome: "paused".to_string(),
            checkpoint_id: Some("test123".to_string()),
            session_id: None,
            model: "test".to_string(),
            agent_message: Some("Testing".to_string()),
            steps: 1,
            total_steps: 1,
            usage: LLMTokenUsage::default(),
            pause_reason: Some(PauseReason::ToolApprovalRequired {
                pending_tool_calls: vec![PendingToolCall {
                    id: "tc_001".to_string(),
                    name: "run_command".to_string(),
                    arguments: serde_json::json!({"command": "ls"}),
                }],
            }),
            resume_hint: None,
        };

        let json_str = serde_json::to_string(&manifest).unwrap();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        // Verify the structure that local_tools.rs expects
        assert_eq!(
            json.get("agent_message").unwrap().as_str().unwrap(),
            "Testing"
        );

        let pause_reason = json.get("pause_reason").unwrap();
        // With serde(tag = "type"), the type field should be present
        assert_eq!(
            pause_reason.get("type").unwrap().as_str().unwrap(),
            "tool_approval_required"
        );

        // pending_tool_calls should be accessible directly under pause_reason
        let pending = pause_reason
            .get("pending_tool_calls")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].get("id").unwrap().as_str().unwrap(), "tc_001");
        assert_eq!(
            pending[0].get("name").unwrap().as_str().unwrap(),
            "run_command"
        );
    }

    #[test]
    fn test_display_truncates_multibyte_safely() {
        // String with multi-byte UTF-8 characters (emoji are 4 bytes each)
        // This tests that truncation doesn't panic on character boundaries
        let long_value = "🎉".repeat(50); // 50 emoji = 200 bytes, but only 50 chars

        let manifest = AsyncManifest {
            outcome: "paused".to_string(),
            checkpoint_id: None,
            session_id: None,
            model: "test".to_string(),
            agent_message: None,
            steps: 1,
            total_steps: 1,
            usage: LLMTokenUsage::default(),
            pause_reason: Some(PauseReason::ToolApprovalRequired {
                pending_tool_calls: vec![PendingToolCall {
                    id: "tc_001".to_string(),
                    name: "test_tool".to_string(),
                    arguments: serde_json::json!({"data": long_value}),
                }],
            }),
            resume_hint: None,
        };

        // Should not panic
        let output = manifest.to_string();
        assert!(output.contains("data:"));
        assert!(output.contains("...")); // Should be truncated
    }
}
