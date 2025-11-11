pub const CONTEXT_LESS_CHARGE_LIMIT: u32 = 200_000;
pub const CONTEXT_MAX_UTIL_TOKENS: u32 = 1_000_000;
pub const CONTEXT_HIGH_UTIL_THRESHOLD: u32 = CONTEXT_LESS_CHARGE_LIMIT * 9 / 10;
pub const CONTEXT_APPROACH_PERCENT: u64 = 85;

#[derive(Clone, Copy)]
pub struct ContextPricingTier {
    pub tier_label: &'static str,
    pub range_label: &'static str,
    pub input_cost: &'static str,
    pub output_cost: &'static str,
    /// Inclusive upper bound for the tier, if applicable.
    pub upper_bound: Option<u32>,
}

pub const CONTEXT_PRICING_TABLE: [ContextPricingTier; 2] = [
    ContextPricingTier {
        tier_label: "<200k tokens",
        range_label: "0 - 200k",
        input_cost: "$3 / MTok",
        output_cost: "$15 / MTok",
        upper_bound: Some(CONTEXT_LESS_CHARGE_LIMIT),
    },
    ContextPricingTier {
        tier_label: "200k-1M tokens",
        range_label: "200k - 1M",
        input_cost: "$6 / MTok",
        output_cost: "$22.50 / MTok",
        upper_bound: None,
    },
];
