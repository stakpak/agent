//! Prompt assembly for watch triggers.
//!
//! Assembles the final prompt passed to the agent, combining the user's prompt
//! with context about the trigger and check script results.

use crate::commands::watch::{CheckResult, Trigger};

/// Assemble the final prompt to pass to the agent.
///
/// The prompt structure is:
/// ```text
/// {user_prompt}
///
/// ---
/// Trigger: {trigger_name}
/// [Check script: {check_path}]
/// [Check output:
/// ```
/// {stdout}
/// ```]
///
/// [Board: {board_id}
/// Track your progress and document findings on this board.]
/// ---
/// ```
///
/// # Arguments
/// * `trigger` - The trigger configuration
/// * `check_result` - Optional result from running the check script
///
/// # Returns
/// The assembled prompt string ready to pass to the agent.
pub fn assemble_prompt(trigger: &Trigger, check_result: Option<&CheckResult>) -> String {
    let mut parts = Vec::new();

    // User's prompt
    parts.push(trigger.prompt.clone());

    // Context block
    let mut context_lines = Vec::new();

    // Always include trigger name
    context_lines.push(format!("Trigger: {}", trigger.name));

    // Include check script info if check was run
    if let Some(result) = check_result
        && let Some(check_path) = &trigger.check
    {
        context_lines.push(format!("Check script: {}", check_path));

        // Include check output if non-empty
        let stdout = result.stdout.trim();
        if !stdout.is_empty() {
            context_lines.push(format!("Check output:\n```\n{}\n```", stdout));
        }
    }

    // Include board section if board_id is configured
    if let Some(board_id) = &trigger.board_id {
        context_lines.push(format!(
            "Board: {}\n\
            Use this board to track state across runs:\n\
            - Check existing cards before starting new work\n\
            - Create/update cards for ongoing tasks\n\
            - Add comments to document findings and decisions\n\
            - Tag cards `blocked` or `needs-human` when user input required",
            board_id
        ));
    }

    // Build context block with delimiters
    let context_block = format!("---\n{}\n---", context_lines.join("\n\n"));

    parts.push(context_block);

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Create a test trigger with all fields populated.
    fn full_trigger() -> Trigger {
        Trigger {
            name: "disk-cleanup".to_string(),
            schedule: "*/15 * * * *".to_string(),
            check: Some("~/.stakpak/triggers/check-disk.sh".to_string()),
            check_timeout: Some(Duration::from_secs(30)),
            prompt: "Analyze disk usage and safely free up space.".to_string(),
            profile: Some("infrastructure".to_string()),
            board_id: Some("board_abc123".to_string()),
            timeout: Some(Duration::from_secs(1800)),
            enable_slack_tools: None,
            enable_subagents: None,
            pause_on_approval: None,
        }
    }

    /// Create a minimal trigger with only required fields.
    fn minimal_trigger() -> Trigger {
        Trigger {
            name: "simple-task".to_string(),
            schedule: "0 * * * *".to_string(),
            check: None,
            check_timeout: None,
            prompt: "Do something simple.".to_string(),
            profile: None,
            board_id: None,
            timeout: None,
            enable_slack_tools: None,
            enable_subagents: None,
            pause_on_approval: None,
        }
    }

    /// Create a check result with the given stdout.
    fn check_result_with_stdout(stdout: &str) -> CheckResult {
        CheckResult {
            exit_code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
            timed_out: false,
        }
    }

    #[test]
    fn test_prompt_with_check_and_board() {
        let trigger = full_trigger();
        let check_result = check_result_with_stdout("Disk usage: 92%\n/var/log: 5GB");

        let prompt = assemble_prompt(&trigger, Some(&check_result));

        // Verify user prompt is included
        assert!(prompt.contains("Analyze disk usage and safely free up space."));

        // Verify trigger name
        assert!(prompt.contains("Trigger: disk-cleanup"));

        // Verify check script path
        assert!(prompt.contains("Check script: ~/.stakpak/triggers/check-disk.sh"));

        // Verify check output
        assert!(prompt.contains("Check output:"));
        assert!(prompt.contains("Disk usage: 92%"));
        assert!(prompt.contains("/var/log: 5GB"));

        // Verify board section
        assert!(prompt.contains("Board: board_abc123"));
        assert!(prompt.contains("Use this board to track state across runs:"));

        // Verify delimiters
        assert!(prompt.contains("---"));
    }

    #[test]
    fn test_prompt_without_check() {
        let trigger = full_trigger();

        // No check result provided
        let prompt = assemble_prompt(&trigger, None);

        // Verify user prompt is included
        assert!(prompt.contains("Analyze disk usage and safely free up space."));

        // Verify trigger name
        assert!(prompt.contains("Trigger: disk-cleanup"));

        // Check script section should NOT be included
        assert!(!prompt.contains("Check script:"));
        assert!(!prompt.contains("Check output:"));

        // Board should still be included
        assert!(prompt.contains("Board: board_abc123"));
    }

    #[test]
    fn test_prompt_without_board() {
        let mut trigger = full_trigger();
        trigger.board_id = None;

        let check_result = check_result_with_stdout("All good");
        let prompt = assemble_prompt(&trigger, Some(&check_result));

        // Verify user prompt and trigger
        assert!(prompt.contains("Analyze disk usage and safely free up space."));
        assert!(prompt.contains("Trigger: disk-cleanup"));

        // Check script should be included
        assert!(prompt.contains("Check script:"));
        assert!(prompt.contains("Check output:"));

        // Board section should NOT be included
        assert!(!prompt.contains("Board:"));
        assert!(!prompt.contains("track state across runs"));
    }

    #[test]
    fn test_prompt_minimal() {
        let trigger = minimal_trigger();

        // No check result, no board
        let prompt = assemble_prompt(&trigger, None);

        // Verify user prompt
        assert!(prompt.contains("Do something simple."));

        // Verify trigger name
        assert!(prompt.contains("Trigger: simple-task"));

        // No check script section
        assert!(!prompt.contains("Check script:"));
        assert!(!prompt.contains("Check output:"));

        // No board section
        assert!(!prompt.contains("Board:"));

        // Delimiters should still be present
        assert!(prompt.contains("---"));
    }

    #[test]
    fn test_multiline_check_output() {
        let trigger = full_trigger();
        let check_result = check_result_with_stdout(
            "Line 1: First item\nLine 2: Second item\nLine 3: Third item\n\nLine 5: After blank",
        );

        let prompt = assemble_prompt(&trigger, Some(&check_result));

        // All lines should be preserved
        assert!(prompt.contains("Line 1: First item"));
        assert!(prompt.contains("Line 2: Second item"));
        assert!(prompt.contains("Line 3: Third item"));
        assert!(prompt.contains("Line 5: After blank"));

        // Output should be in code block
        assert!(prompt.contains("```\nLine 1:"));
    }

    #[test]
    fn test_empty_check_output() {
        let trigger = full_trigger();
        let check_result = check_result_with_stdout("");

        let prompt = assemble_prompt(&trigger, Some(&check_result));

        // Check script path should be included
        assert!(prompt.contains("Check script: ~/.stakpak/triggers/check-disk.sh"));

        // But check output section should be omitted when stdout is empty
        assert!(!prompt.contains("Check output:"));
    }

    #[test]
    fn test_whitespace_only_check_output() {
        let trigger = full_trigger();
        let check_result = check_result_with_stdout("   \n\n  \t  ");

        let prompt = assemble_prompt(&trigger, Some(&check_result));

        // Check output section should be omitted when stdout is only whitespace
        assert!(!prompt.contains("Check output:"));
    }

    #[test]
    fn test_prompt_structure() {
        let trigger = full_trigger();
        let check_result = check_result_with_stdout("test output");

        let prompt = assemble_prompt(&trigger, Some(&check_result));

        // User prompt should come first
        let user_prompt_pos = prompt.find("Analyze disk usage").unwrap();
        let delimiter_pos = prompt.find("---").unwrap();

        assert!(
            user_prompt_pos < delimiter_pos,
            "User prompt should come before context block"
        );

        // Trigger name should be inside context block
        let trigger_pos = prompt.find("Trigger: disk-cleanup").unwrap();
        assert!(
            trigger_pos > delimiter_pos,
            "Trigger name should be inside context block"
        );
    }
}
