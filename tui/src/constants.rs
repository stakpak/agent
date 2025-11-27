// ========== Context & Token Limits ==========
pub const CONTEXT_LESS_CHARGE_LIMIT: u32 = 200_000;
pub const CONTEXT_MAX_UTIL_TOKENS: u32 = 1_000_000; // Claude Sonnect 4.5 limit
pub const CONTEXT_MAX_UTIL_TOKENS_ECO: u32 = 200_000; // Claude Haiku 4.5 limit
pub const CONTEXT_MAX_UTIL_TOKENS_RECOVERY: u32 = 400_000; // GPT5 limit
pub const CONTEXT_HIGH_UTIL_THRESHOLD: u32 = CONTEXT_LESS_CHARGE_LIMIT * 9 / 10;
pub const CONTEXT_APPROACH_PERCENT: u64 = 85;

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

#[derive(Clone, Copy)]
pub struct ContextPricingTier {
    pub tier_label: &'static str,
    pub input_cost: &'static str,
    pub output_cost: &'static str,
    /// Inclusive upper bound for the tier, if applicable.
    pub upper_bound: Option<u32>,
}

pub const CONTEXT_PRICING_TABLE: [ContextPricingTier; 2] = [
    ContextPricingTier {
        tier_label: "<200K tokens",
        input_cost: "$3/1M",
        output_cost: "$15/1M",
        upper_bound: Some(CONTEXT_LESS_CHARGE_LIMIT),
    },
    ContextPricingTier {
        tier_label: "200K-1M tokens",
        input_cost: "$6/1M",
        output_cost: "$22.5/1M",
        upper_bound: None,
    },
];

pub const CONTEXT_PRICING_TABLE_ECO: [ContextPricingTier; 1] = [ContextPricingTier {
    tier_label: "0-200K tokens",
    input_cost: "$1/1M",
    output_cost: "$5/1M",
    upper_bound: Some(CONTEXT_MAX_UTIL_TOKENS_ECO),
}];

pub const CONTEXT_PRICING_TABLE_RECOVERY: [ContextPricingTier; 1] = [ContextPricingTier {
    tier_label: "0-400K tokens",
    input_cost: "$1.25/1M",
    output_cost: "$10/1M",
    upper_bound: Some(CONTEXT_MAX_UTIL_TOKENS_RECOVERY),
}];

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
