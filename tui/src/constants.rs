// ========== Input & Shell Constants ==========
pub const INTERACTIVE_COMMANDS: [&str; 2] = ["ssh", "sudo"];

// ========== UI & Scrolling Constants ==========
pub const SCROLL_LINES: usize = 1;
pub const SCROLL_BUFFER_LINES: usize = 2;
pub const DROPDOWN_MAX_HEIGHT: usize = 8;
pub const MAX_PASTE_CHAR_COUNT: usize = 1000;
pub const APPROVAL_POPUP_WIDTH_PERCENT: f32 = 0.8;

// ========== Error Messages ==========
pub const EXCEEDED_API_LIMIT_ERROR: &str = "Exceeded API limit";
pub const EXCEEDED_API_LIMIT_ERROR_MESSAGE: &str = "Exceeded credits plan limit. Please top up your account at https://stakpak.dev/settings/billing to keep Stakpaking.";

// ========== File Paths ==========
pub const AUTO_APPROVE_CONFIG_PATH: &str = ".stakpak/session/auto_approve.json";

pub const SUMMARIZE_PROMPT_BASE: &str = "\
You are the Stakpak session summarizer. You have full context of the session, including workspace state, current working directory, and file activity.\n\
\n\
Goal:\n\
- Create a concise yet complete markdown summary for another engineer to resume work seamlessly.\n\
\n\
Instructions:\n\
1. Use the `create_file` tool to write the summary as `summary.md` in the current working directory.\n\
   - If `summary.md` exists, create `summary-1.md`, `summary-2.md`, etc.\n\
2. Add local context, CWD and current profile name, DON'T add token usage information.
3. Include these sections (in order):\n\
   Overview; Key Accomplishments; Key Decisions & Rationale; Commands & Tools; Files Modified/Created; Tests & Verification; Issues/Blockers; Next Steps.\n\
4. Use bullet lists and short paragraphs; include fenced code blocks for critical commands or snippets.\n\
5. The summary must be self-contained and not reference prior messages.\n\
6. Respond only with the tool result or acknowledgement—no extra commentary.\n";
