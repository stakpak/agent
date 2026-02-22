//! Prompt assembly for autopilot schedules.
//!
//! The user-facing prompt stays focused on the requested task.
//! Operational metadata (schedule/check/board) is still sent separately as
//! structured caller context and injected server-side. We also include a
//! compact metadata fallback in the user prompt so runs remain debuggable if
//! structured context is unavailable.

use crate::commands::watch::{CheckResult, Schedule};
use stakpak_gateway::client::CallerContextInput;
use stakpak_shared::utils::truncate_chars_with_ellipsis;

const PROMPT_FALLBACK_STREAM_CHARS: usize = 2_000;
const CALLER_CONTEXT_STREAM_CHARS: usize = 20_000;

fn build_schedule_metadata_lines(
    schedule: &Schedule,
    check_result: Option<&CheckResult>,
    stream_chars_limit: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("Schedule: {}", schedule.name));
    lines.push(format!("Cron: {}", schedule.cron));

    if let Some(result) = check_result
        && let Some(check_path) = &schedule.check
    {
        lines.push(format!("Check script: {}", check_path));
        lines.push(format!(
            "Check exit code: {}",
            result.exit_code.unwrap_or(-1)
        ));

        let stdout = result.stdout.trim();
        if !stdout.is_empty() {
            lines.push(format!(
                "Check stdout:\n{}",
                truncate_chars_with_ellipsis(stdout, stream_chars_limit)
            ));
        }

        let stderr = result.stderr.trim();
        if !stderr.is_empty() {
            lines.push(format!(
                "Check stderr:\n{}",
                truncate_chars_with_ellipsis(stderr, stream_chars_limit)
            ));
        }
    }

    if let Some(board_id) = &schedule.board_id {
        lines.push(format!("Board: {}", board_id));
    }

    lines
}

/// Assemble the user prompt to pass to the agent.
///
/// Kept as a small seam so watch/autopilot can evolve prompt shaping in one
/// place while keeping callsites stable.
///
/// A compact schedule metadata block is appended as a fallback in case
/// structured caller context injection is unavailable.
pub fn assemble_prompt(schedule: &Schedule, check_result: Option<&CheckResult>) -> String {
    let mut prompt = schedule.prompt.clone();
    let metadata =
        build_schedule_metadata_lines(schedule, check_result, PROMPT_FALLBACK_STREAM_CHARS)
            .join("\n\n");

    if !metadata.is_empty() {
        prompt.push_str(
            "\n\n---\nOperational context fallback (use if structured context is missing):\n\n",
        );
        prompt.push_str(&metadata);
    }

    prompt
}

/// Build structured caller context for schedule-driven runs.
///
/// This keeps rich run metadata out of the raw user prompt and lets the
/// server-side context pipeline apply budgeting/priority rules consistently.
pub fn build_schedule_caller_context(
    schedule: &Schedule,
    check_result: Option<&CheckResult>,
) -> Vec<CallerContextInput> {
    let lines = build_schedule_metadata_lines(schedule, check_result, CALLER_CONTEXT_STREAM_CHARS);

    vec![CallerContextInput {
        name: "watch_schedule_context".to_string(),
        content: lines.join("\n\n"),
        priority: Some("high".to_string()),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::watch::InteractionMode;
    use std::time::Duration;

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
            interaction: InteractionMode::Interactive,
            enabled: true,
        }
    }

    fn check_result_with_stdout(stdout: &str) -> CheckResult {
        CheckResult {
            exit_code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
            timed_out: false,
        }
    }

    #[test]
    fn assemble_prompt_includes_fallback_metadata() {
        let schedule = full_schedule();
        let check_result = check_result_with_stdout("disk usage 92%");

        let prompt = assemble_prompt(&schedule, Some(&check_result));
        assert!(prompt.contains(&schedule.prompt));
        assert!(prompt.contains("Operational context fallback"));
        assert!(prompt.contains("Schedule: disk-cleanup"));
        assert!(prompt.contains("Cron: */15 * * * *"));
        assert!(prompt.contains("Check stdout:"));
        assert!(prompt.contains("Board: board_abc123"));
    }

    #[test]
    fn test_build_schedule_caller_context() {
        let schedule = full_schedule();
        let check_result = check_result_with_stdout("disk usage 92%");

        let context = build_schedule_caller_context(&schedule, Some(&check_result));
        assert_eq!(context.len(), 1);
        assert_eq!(context[0].name, "watch_schedule_context");
        assert_eq!(context[0].priority.as_deref(), Some("high"));
        assert!(context[0].content.contains("Schedule: disk-cleanup"));
        assert!(context[0].content.contains("Cron: */15 * * * *"));
        assert!(context[0].content.contains("Check stdout:"));
        assert!(!context[0].content.contains("Check stderr:"));
        assert!(context[0].content.contains("Board: board_abc123"));
    }

    #[test]
    fn build_schedule_caller_context_omits_empty_streams() {
        let schedule = full_schedule();
        let check_result = CheckResult {
            exit_code: Some(2),
            stdout: "   \n".to_string(),
            stderr: "error line".to_string(),
            timed_out: false,
        };

        let context = build_schedule_caller_context(&schedule, Some(&check_result));
        assert_eq!(context.len(), 1);
        assert!(!context[0].content.contains("Check stdout:"));
        assert!(context[0].content.contains("Check stderr:"));
    }

    #[test]
    fn truncate_context_respects_unicode() {
        let value = "é".repeat(10);
        let truncated = truncate_chars_with_ellipsis(&value, 5);
        assert_eq!(truncated, "ééééé...");
    }

    #[test]
    fn truncate_context_exact_boundary() {
        let value = "a".repeat(CALLER_CONTEXT_STREAM_CHARS);
        let truncated = truncate_chars_with_ellipsis(&value, CALLER_CONTEXT_STREAM_CHARS);
        assert_eq!(truncated.len(), CALLER_CONTEXT_STREAM_CHARS);
        assert!(!truncated.ends_with("..."));
    }

    #[test]
    fn build_schedule_caller_context_minimal_schedule_without_check() {
        let schedule = Schedule {
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
            interaction: InteractionMode::Interactive,
            enabled: true,
        };

        let context = build_schedule_caller_context(&schedule, None);
        assert_eq!(context.len(), 1);
        assert!(context[0].content.contains("Schedule: simple-task"));
        assert!(context[0].content.contains("Cron: 0 * * * *"));
        assert!(!context[0].content.contains("Check script:"));

        let prompt = assemble_prompt(&schedule, None);
        assert!(prompt.contains("Schedule: simple-task"));
    }
}
