//! Prompt assembly for autopilot schedules.
//!
//! Assembles the final prompt passed to the agent, combining the user's prompt
//! with context about the schedule and check script results.

use crate::commands::watch::{CheckResult, Schedule};

/// Assemble the final prompt to pass to the agent.
///
/// The prompt structure is:
/// ```text
/// {user_prompt}
///
/// ---
/// Schedule: {schedule_name}
/// [Check script: {check_path}]
/// [Check exit code: {exit_code}]
/// [Check stdout:
/// ```
/// {stdout}
/// ```]
/// [Check stderr:
/// ```
/// {stderr}
/// ```]
///
/// [Board: {board_id}
/// Track your progress and document findings on this board.]
/// ---
/// ```
///
/// # Arguments
/// * `schedule` - The schedule configuration
/// * `check_result` - Optional result from running the check script
///
/// # Returns
/// The assembled prompt string ready to pass to the agent.
pub fn assemble_prompt(schedule: &Schedule, check_result: Option<&CheckResult>) -> String {
    let mut parts = Vec::new();

    // User's prompt
    parts.push(schedule.prompt.clone());

    // Context block
    let mut context_lines = Vec::new();

    // Always include schedule name
    context_lines.push(format!("Schedule: {}", schedule.name));

    // Include check script info if check was run
    if let Some(result) = check_result
        && let Some(check_path) = &schedule.check
    {
        context_lines.push(format!("Check script: {}", check_path));
        context_lines.push(format!(
            "Check exit code: {}",
            result.exit_code.unwrap_or(-1)
        ));

        // Include check stdout if non-empty
        let stdout = result.stdout.trim();
        if !stdout.is_empty() {
            context_lines.push(format!("Check stdout:\n```\n{}\n```", stdout));
        }

        // Include check stderr if non-empty
        let stderr = result.stderr.trim();
        if !stderr.is_empty() {
            context_lines.push(format!("Check stderr:\n```\n{}\n```", stderr));
        }
    }

    // Include board section if board_id is configured
    if let Some(board_id) = &schedule.board_id {
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

    /// Create a test schedule with all fields populated.
    fn full_schedule() -> Schedule {
        Schedule {
            name: "disk-cleanup".to_string(),
            cron: "*/15 * * * *".to_string(),
            check: Some("~/.stakpak/schedules/check-disk.sh".to_string()),
            check_timeout: Some(Duration::from_secs(30)),
            trigger_on: None,
            prompt: "Analyze disk usage and safely free up space.".to_string(),
            profile: Some("infrastructure".to_string()),
            board_id: Some("board_abc123".to_string()),
            timeout: Some(Duration::from_secs(1800)),
            enable_slack_tools: None,
            enable_subagents: None,
            pause_on_approval: None,
            sandbox: None,
            notify_on: None,
            notify_channel: None,
            notify_chat_id: None,
            enabled: true,
        }
    }

    /// Create a minimal schedule with only required fields.
    fn minimal_schedule() -> Schedule {
        Schedule {
            name: "simple-task".to_string(),
            cron: "0 * * * *".to_string(),
            check: None,
            check_timeout: None,
            trigger_on: None,
            prompt: "Do something simple.".to_string(),
            profile: None,
            board_id: None,
            timeout: None,
            enable_slack_tools: None,
            enable_subagents: None,
            pause_on_approval: None,
            sandbox: None,
            notify_on: None,
            notify_channel: None,
            notify_chat_id: None,
            enabled: true,
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
        let schedule = full_schedule();
        let check_result = check_result_with_stdout("Disk usage: 92%\n/var/log: 5GB");

        let prompt = assemble_prompt(&schedule, Some(&check_result));

        // Verify user prompt is included
        assert!(prompt.contains("Analyze disk usage and safely free up space."));

        // Verify schedule name
        assert!(prompt.contains("Schedule: disk-cleanup"));

        // Verify check script path
        assert!(prompt.contains("Check script: ~/.stakpak/schedules/check-disk.sh"));

        // Verify check exit code
        assert!(prompt.contains("Check exit code: 0"));

        // Verify check stdout
        assert!(prompt.contains("Check stdout:"));
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
        let schedule = full_schedule();

        // No check result provided
        let prompt = assemble_prompt(&schedule, None);

        // Verify user prompt is included
        assert!(prompt.contains("Analyze disk usage and safely free up space."));

        // Verify schedule name
        assert!(prompt.contains("Schedule: disk-cleanup"));

        // Check script section should NOT be included
        assert!(!prompt.contains("Check script:"));
        assert!(!prompt.contains("Check exit code:"));
        assert!(!prompt.contains("Check stdout:"));

        // Board should still be included
        assert!(prompt.contains("Board: board_abc123"));
    }

    #[test]
    fn test_prompt_without_board() {
        let mut schedule = full_schedule();
        schedule.board_id = None;

        let check_result = check_result_with_stdout("All good");
        let prompt = assemble_prompt(&schedule, Some(&check_result));

        // Verify user prompt and schedule
        assert!(prompt.contains("Analyze disk usage and safely free up space."));
        assert!(prompt.contains("Schedule: disk-cleanup"));

        // Check script should be included
        assert!(prompt.contains("Check script:"));
        assert!(prompt.contains("Check exit code:"));
        assert!(prompt.contains("Check stdout:"));

        // Board section should NOT be included
        assert!(!prompt.contains("Board:"));
        assert!(!prompt.contains("track state across runs"));
    }

    #[test]
    fn test_prompt_minimal() {
        let schedule = minimal_schedule();

        // No check result, no board
        let prompt = assemble_prompt(&schedule, None);

        // Verify user prompt
        assert!(prompt.contains("Do something simple."));

        // Verify schedule name
        assert!(prompt.contains("Schedule: simple-task"));

        // No check script section
        assert!(!prompt.contains("Check script:"));
        assert!(!prompt.contains("Check exit code:"));
        assert!(!prompt.contains("Check stdout:"));

        // No board section
        assert!(!prompt.contains("Board:"));

        // Delimiters should still be present
        assert!(prompt.contains("---"));
    }

    #[test]
    fn test_multiline_check_output() {
        let schedule = full_schedule();
        let check_result = check_result_with_stdout(
            "Line 1: First item\nLine 2: Second item\nLine 3: Third item\n\nLine 5: After blank",
        );

        let prompt = assemble_prompt(&schedule, Some(&check_result));

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
        let schedule = full_schedule();
        let check_result = check_result_with_stdout("");

        let prompt = assemble_prompt(&schedule, Some(&check_result));

        // Check script path and exit code should be included
        assert!(prompt.contains("Check script: ~/.stakpak/schedules/check-disk.sh"));
        assert!(prompt.contains("Check exit code: 0"));

        // But check stdout section should be omitted when stdout is empty
        assert!(!prompt.contains("Check stdout:"));
    }

    #[test]
    fn test_whitespace_only_check_output() {
        let schedule = full_schedule();
        let check_result = check_result_with_stdout("   \n\n  \t  ");

        let prompt = assemble_prompt(&schedule, Some(&check_result));

        // Check stdout section should be omitted when stdout is only whitespace
        assert!(!prompt.contains("Check stdout:"));
    }

    #[test]
    fn test_check_stderr_included() {
        let schedule = full_schedule();
        let check_result = CheckResult {
            exit_code: Some(1),
            stdout: "stdout content".to_string(),
            stderr: "stderr warning message".to_string(),
            timed_out: false,
        };

        let prompt = assemble_prompt(&schedule, Some(&check_result));

        // Both stdout and stderr should be included
        assert!(prompt.contains("Check stdout:"));
        assert!(prompt.contains("stdout content"));
        assert!(prompt.contains("Check stderr:"));
        assert!(prompt.contains("stderr warning message"));

        // Exit code should reflect the actual value
        assert!(prompt.contains("Check exit code: 1"));
    }

    #[test]
    fn test_check_stderr_only() {
        let schedule = full_schedule();
        let check_result = CheckResult {
            exit_code: Some(2),
            stdout: "".to_string(),
            stderr: "Error: something went wrong".to_string(),
            timed_out: false,
        };

        let prompt = assemble_prompt(&schedule, Some(&check_result));

        // Only stderr should be included (stdout is empty)
        assert!(!prompt.contains("Check stdout:"));
        assert!(prompt.contains("Check stderr:"));
        assert!(prompt.contains("Error: something went wrong"));
        assert!(prompt.contains("Check exit code: 2"));
    }

    #[test]
    fn test_prompt_structure() {
        let schedule = full_schedule();
        let check_result = check_result_with_stdout("test output");

        let prompt = assemble_prompt(&schedule, Some(&check_result));

        // User prompt should come first
        let user_prompt_pos = prompt.find("Analyze disk usage").unwrap();
        let delimiter_pos = prompt.find("---").unwrap();

        assert!(
            user_prompt_pos < delimiter_pos,
            "User prompt should come before context block"
        );

        // Schedule name should be inside context block
        let schedule_pos = prompt.find("Schedule: disk-cleanup").unwrap();
        assert!(
            schedule_pos > delimiter_pos,
            "Schedule name should be inside context block"
        );
    }
}
