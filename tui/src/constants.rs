pub const CONTEXT_LESS_CHARGE_LIMIT: u32 = 200_000;
pub const CONTEXT_MAX_UTIL_TOKENS: u32 = 1_000_000;
pub const CONTEXT_HIGH_UTIL_THRESHOLD: u32 = CONTEXT_LESS_CHARGE_LIMIT * 9 / 10;
pub const CONTEXT_APPROACH_PERCENT: u64 = 85;

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
        tier_label: "<200k tokens",
        input_cost: "$3 / MTok",
        output_cost: "$15 / MTok",
        upper_bound: Some(CONTEXT_LESS_CHARGE_LIMIT),
    },
    ContextPricingTier {
        tier_label: "200k-1M tokens",
        input_cost: "$6 / MTok",
        output_cost: "$22.50 / MTok",
        upper_bound: None,
    },
];

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
