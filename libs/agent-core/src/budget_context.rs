use crate::context::{
    ContextReducer, dedup_tool_results, merge_consecutive_same_role, remove_orphaned_tool_results,
    strip_dangling_tool_calls,
};
use stakai::{ContentPart, Message, MessageContent, Model, Role, Tool};

const TRIMMED_CONTENT_PLACEHOLDER: &str = "[trimmed]";
const BYTES_PER_TOKEN: f64 = 3.5;
const SAFETY_BUFFER_FACTOR: f64 = 1.05;
/// Headroom factor applied to the trim target so the trim boundary stays frozen
/// across multiple turns, preserving Anthropic prompt cache stability. Without this,
/// trim_end advances every turn, invalidating cache on every request.
const TRIM_HEADROOM_FACTOR: f64 = 0.75;

#[derive(Debug, Clone)]
pub struct BudgetAwareContextReducer {
    keep_last_n_assistant_messages: usize,
    context_budget_threshold: f32,
}

impl BudgetAwareContextReducer {
    pub fn new(keep_last_n_assistant_messages: usize, context_budget_threshold: f32) -> Self {
        Self {
            keep_last_n_assistant_messages,
            context_budget_threshold,
        }
    }

    fn bytes_to_tokens(bytes: usize) -> u64 {
        (bytes as f64 / BYTES_PER_TOKEN).ceil() as u64
    }

    fn estimate_content_part_tokens(part: &ContentPart) -> u64 {
        match part {
            ContentPart::Text { text, .. } => Self::bytes_to_tokens(text.len()),
            ContentPart::ToolCall {
                name, arguments, ..
            } => {
                let content_bytes = name.len() + arguments.to_string().len();
                Self::bytes_to_tokens(content_bytes + 30)
            }
            ContentPart::ToolResult { content, .. } => {
                let content_bytes = content
                    .as_str()
                    .map(|value| value.len())
                    .unwrap_or_else(|| content.to_string().len());
                Self::bytes_to_tokens(content_bytes + 30)
            }
            ContentPart::Image { .. } => 2000,
        }
    }

    fn estimate_message_tokens_raw(msg: &Message) -> u64 {
        let content_tokens = match &msg.content {
            MessageContent::Text(text) => Self::bytes_to_tokens(text.len()),
            MessageContent::Parts(parts) => {
                let part_tokens: u64 = parts.iter().map(Self::estimate_content_part_tokens).sum();
                let part_overhead = parts.len() as u64 * 3;
                part_tokens + part_overhead
            }
        };

        content_tokens + 8
    }

    fn estimate_tokens_raw(messages: &[Message]) -> u64 {
        messages.iter().map(Self::estimate_message_tokens_raw).sum()
    }

    fn add_safety_buffer(raw_tokens: u64) -> u64 {
        (raw_tokens as f64 * SAFETY_BUFFER_FACTOR).ceil() as u64
    }

    pub fn estimate_tokens(messages: &[Message]) -> u64 {
        Self::add_safety_buffer(Self::estimate_tokens_raw(messages))
    }

    pub fn estimate_tool_overhead(tools: &[Tool]) -> u64 {
        tools
            .iter()
            .map(|tool| {
                let schema_len = tool.function.parameters.to_string().len();
                let tool_bytes =
                    tool.function.name.len() + tool.function.description.len() + schema_len;
                let adjusted_bytes = (tool_bytes as f64 * 1.2).ceil() as usize;
                Self::bytes_to_tokens(adjusted_bytes) + 8
            })
            .sum()
    }

    fn trim_message(msg: &mut Message) {
        match &mut msg.content {
            MessageContent::Text(text) => {
                *text = TRIMMED_CONTENT_PLACEHOLDER.to_string();
            }
            MessageContent::Parts(parts) => {
                for part in parts.iter_mut() {
                    match part {
                        ContentPart::Text { text, .. } => {
                            *text = TRIMMED_CONTENT_PLACEHOLDER.to_string();
                        }
                        ContentPart::ToolResult { content, .. } => {
                            *content = serde_json::json!(TRIMMED_CONTENT_PLACEHOLDER);
                        }
                        ContentPart::ToolCall { .. } | ContentPart::Image { .. } => {}
                    }
                }
            }
        }
    }

    fn trim_message_with_delta(msg: &mut Message) -> i64 {
        let before = Self::estimate_message_tokens_raw(msg);
        Self::trim_message(msg);
        let after = Self::estimate_message_tokens_raw(msg);
        after as i64 - before as i64
    }

    fn metadata_trimmed_up_to(metadata: &serde_json::Value) -> usize {
        metadata
            .get("trimmed_up_to_message_index")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize
    }

    fn ensure_metadata_object(metadata: &mut serde_json::Value) {
        if !metadata.is_object() {
            *metadata = serde_json::json!({});
        }
    }
}

impl ContextReducer for BudgetAwareContextReducer {
    fn reduce(
        &self,
        messages: Vec<Message>,
        model: &Model,
        max_output_tokens: u32,
        tools: &[Tool],
        metadata: &mut serde_json::Value,
    ) -> Vec<Message> {
        let messages = merge_consecutive_same_role(messages);
        let messages = dedup_tool_results(messages);
        let messages = strip_dangling_tool_calls(messages);
        let mut messages = remove_orphaned_tool_results(messages);

        let available_context_window = model.limit.context.saturating_sub(max_output_tokens as u64);
        let threshold = (available_context_window as f32 * self.context_budget_threshold) as u64;
        let trim_target = (threshold as f64 * TRIM_HEADROOM_FACTOR) as u64;
        let tool_overhead = Self::estimate_tool_overhead(tools);

        let prev_trimmed_up_to = Self::metadata_trimmed_up_to(metadata);
        let mut raw_tokens = Self::estimate_tokens_raw(&messages);

        if prev_trimmed_up_to == 0
            && Self::add_safety_buffer(raw_tokens) + tool_overhead <= threshold
        {
            return messages;
        }

        let len = messages.len();
        let mut keep_n_trim_end = if self.keep_last_n_assistant_messages > 0 {
            0
        } else {
            len
        };

        if self.keep_last_n_assistant_messages > 0 {
            let mut assistant_count = 0usize;
            for i in (0..len).rev() {
                if messages[i].role == Role::Assistant {
                    assistant_count += 1;
                    if assistant_count >= self.keep_last_n_assistant_messages {
                        keep_n_trim_end = i;
                        break;
                    }
                }
            }
        }

        let prev_clamped = prev_trimmed_up_to.min(len);
        for msg in &mut messages[..prev_clamped] {
            if msg.role == Role::Assistant || msg.role == Role::Tool {
                let delta = Self::trim_message_with_delta(msg);
                raw_tokens = (raw_tokens as i64 + delta).max(0) as u64;
            }
        }

        let effective_estimated_tokens = Self::add_safety_buffer(raw_tokens) + tool_overhead;

        let effective_trim_end = if effective_estimated_tokens > threshold {
            let mut candidate = if keep_n_trim_end > 0 {
                for msg in messages
                    .iter_mut()
                    .take(keep_n_trim_end.min(len))
                    .skip(prev_clamped)
                {
                    if msg.role == Role::Assistant || msg.role == Role::Tool {
                        let delta = Self::trim_message_with_delta(msg);
                        raw_tokens = (raw_tokens as i64 + delta).max(0) as u64;
                    }
                }
                keep_n_trim_end
            } else {
                prev_trimmed_up_to
            };

            let mut current_tokens = Self::add_safety_buffer(raw_tokens) + tool_overhead;
            if current_tokens > trim_target {
                let mut scan_idx = candidate;
                while scan_idx < len {
                    if messages[scan_idx].role == Role::Assistant
                        || messages[scan_idx].role == Role::Tool
                    {
                        let delta = Self::trim_message_with_delta(&mut messages[scan_idx]);
                        raw_tokens = (raw_tokens as i64 + delta).max(0) as u64;
                        candidate = scan_idx + 1;

                        current_tokens = Self::add_safety_buffer(raw_tokens) + tool_overhead;
                        if current_tokens <= trim_target {
                            break;
                        }
                    }
                    scan_idx += 1;
                }
            }

            candidate.max(prev_trimmed_up_to)
        } else {
            prev_trimmed_up_to
        };

        // Final pass: ensure every message in [prev_clamped..effective_trim_end] is trimmed.
        // Some of these may have been trimmed already by Phase 2 above — that's harmless
        // because trim_message_with_delta is idempotent (delta=0 on already-trimmed content).
        let clamped_end = effective_trim_end.min(len);
        for msg in messages.iter_mut().take(clamped_end).skip(prev_clamped) {
            if msg.role == Role::Assistant || msg.role == Role::Tool {
                let delta = Self::trim_message_with_delta(msg);
                raw_tokens = (raw_tokens as i64 + delta).max(0) as u64;
            }
        }

        Self::ensure_metadata_object(metadata);
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "trimmed_up_to_message_index".to_string(),
                serde_json::json!(effective_trim_end),
            );
        }

        messages
    }
}
