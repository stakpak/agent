use std::collections::HashMap;

pub struct TaskBoardContextManager {
    keep_last_n_assistant_messages: usize,
    context_budget_threshold: f32,
}

impl super::ContextManager for TaskBoardContextManager {
    fn reduce_context(&self, messages: Vec<stakai::Message>) -> Vec<stakai::Message> {
        let messages = messages
            .into_iter()
            .map(Self::clean_checkpoint_tags)
            .collect();
        let messages = merge_consecutive_same_role(messages);
        dedup_tool_results(messages)
    }
}

const TRIMMED_CONTENT_PLACEHOLDER: &str = "[trimmed]";

impl TaskBoardContextManager {
    /// Remove `<checkpoint_id>...</checkpoint_id>` XML tags from message content.
    fn clean_checkpoint_tags(mut message: stakai::Message) -> stakai::Message {
        match &mut message.content {
            stakai::MessageContent::Text(text) => {
                *text = super::common::remove_xml_tag("checkpoint_id", text);
            }
            stakai::MessageContent::Parts(parts) => {
                for part in parts {
                    if let stakai::ContentPart::Text { text, .. } = part {
                        *text = super::common::remove_xml_tag("checkpoint_id", text);
                    }
                }
            }
        }
        message
    }
}

const BYTES_PER_TOKEN: f64 = 3.5; // Conservative estimate for bytes/token

impl TaskBoardContextManager {
    /// Conservative bytes-to-tokens conversion.
    ///
    /// Uses a divisor of 3.0 (instead of Anthropic's 3.5 guidance for English)
    /// to intentionally overestimate. JSON, code, and non-ASCII content tokenize
    /// at closer to 2.5-3.0 bytes/token, so 3.0 is a safer middle ground.
    /// Overestimating triggers trimming slightly early, which is far safer than
    /// underestimating and hitting context window API errors.
    fn bytes_to_tokens(bytes: usize) -> u64 {
        (bytes as f64 / BYTES_PER_TOKEN).ceil() as u64
    }

    /// Estimate token count for a single content part, including structural overhead.
    ///
    /// Each content part carries API-level structural tokens (type discriminators,
    /// JSON keys, tool_use_id fields) that aren't captured by content length alone.
    fn estimate_content_part_tokens(part: &stakai::ContentPart) -> u64 {
        match part {
            stakai::ContentPart::Text { text, .. } => Self::bytes_to_tokens(text.len()),
            stakai::ContentPart::ToolCall {
                name, arguments, ..
            } => {
                let content_bytes = name.len() + arguments.to_string().len();
                // +30 bytes for tool_use_id (~26 chars), "type":"tool_use", JSON structure
                Self::bytes_to_tokens(content_bytes + 30)
            }
            stakai::ContentPart::ToolResult { content, .. } => {
                // +30 bytes for tool_use_id, "type":"tool_result", JSON structure
                Self::bytes_to_tokens(content.to_string().len() + 30)
            }
            // Images: Anthropic charges 1600-6400+ tokens depending on resolution.
            // Use 2000 as a conservative default for typical images.
            stakai::ContentPart::Image { .. } => 2000,
        }
    }

    /// Estimate token count from LLM messages with conservative bias.
    ///
    /// Intentionally overestimates by ~15-25% compared to actual API token counts.
    /// This is by design: triggering trimming a bit early is far safer than
    /// blowing past the context window and getting 400 errors.
    ///
    /// Conservative measures applied:
    /// - Divisor of 3.0 bytes/token (vs Anthropic's 3.5 guidance for English)
    /// - 8 tokens per-message overhead (role, formatting, content block wrappers)
    /// - 3 tokens per content part in List messages (part-level structure)
    /// - 30 bytes structural overhead per ToolCall/ToolResult (IDs, type fields)
    /// - 2000 tokens per image (vs previous 1000)
    /// - 5% safety buffer on the final total
    pub fn estimate_tokens(messages: &[stakai::Message]) -> u64 {
        let raw_estimate: u64 = messages
            .iter()
            .map(|msg| {
                let content_tokens = match &msg.content {
                    stakai::MessageContent::Text(s) => Self::bytes_to_tokens(s.len()),
                    stakai::MessageContent::Parts(parts) => {
                        let part_tokens: u64 =
                            parts.iter().map(Self::estimate_content_part_tokens).sum();
                        // Per-part structural overhead: each content block in a List
                        // has type discriminator and wrapper tokens (~3 tokens each)
                        let part_overhead = parts.len() as u64 * 3;
                        part_tokens + part_overhead
                    }
                };
                // Per-message overhead: role tag, content block wrapper, formatting.
                // Anthropic's actual overhead is ~7-10 tokens; use 8 as conservative middle.
                content_tokens + 8
            })
            .sum();

        // 5% safety buffer to catch remaining estimation drift
        (raw_estimate as f64 * 1.05).ceil() as u64
    }

    /// Trim a single message's content, replacing it with a placeholder.
    /// Preserves message structure (role, tool_call_ids) for API validity.
    fn trim_message(msg: &mut stakai::Message) {
        match &mut msg.content {
            stakai::MessageContent::Text(s) => {
                *s = TRIMMED_CONTENT_PLACEHOLDER.to_string();
            }
            stakai::MessageContent::Parts(parts) => {
                for part in parts.iter_mut() {
                    match part {
                        stakai::ContentPart::Text { text, .. } => {
                            *text = TRIMMED_CONTENT_PLACEHOLDER.to_string();
                        }
                        stakai::ContentPart::ToolResult { content, .. } => {
                            *content =
                                serde_json::Value::String(TRIMMED_CONTENT_PLACEHOLDER.to_string());
                        }
                        // Preserve ToolCall structure - needed for API to match tool_use/tool_result
                        stakai::ContentPart::ToolCall { .. } => {}
                        stakai::ContentPart::Image { .. } => {}
                    }
                }
            }
        }
    }

    /// Estimate overhead tokens from tool definitions.
    ///
    /// Tool definitions are sent to the API alongside messages but are not
    /// included in the chat message list, so the trimmer needs to account
    /// for them separately. The system prompt is already part of the message
    /// list and is covered by `estimate_tokens`.
    pub fn estimate_tool_overhead(tools: Option<&[stakai::Tool]>) -> u64 {
        tools
            .map(|t| {
                t.iter()
                    .map(|tool| {
                        let schema_len = tool.function.parameters.to_string().len();
                        let tool_bytes =
                            tool.function.name.len() + tool.function.description.len() + schema_len;
                        // 1.2x multiplier accounts for JSON structural overhead not
                        // captured by content length: property names, "type":"function"
                        // wrappers, "required" arrays, enum values, nested refs, etc.
                        let adjusted_bytes = (tool_bytes as f64 * 1.2).ceil() as usize;
                        Self::bytes_to_tokens(adjusted_bytes) + 8
                    })
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Budget-aware context reduction that preserves Anthropic prompt caching.
    ///
    /// **Trimming semantics:**
    /// - Only **assistant** and **tool** messages are ever trimmed (replaced
    ///   with `[trimmed]` placeholders). User and system messages are always
    ///   preserved in full.
    /// - `keep_last_n_assistant_messages` controls how many recent assistant
    ///   messages are kept untrimmed. The trim boundary is placed just before
    ///   the Nth-from-last assistant message, so all messages after that point
    ///   (including interleaved user/tool messages) are untouched.
    ///
    /// **Lazy trimming with `trimmed_up_to_message_index`:**
    /// - On every call, previously-trimmed messages (up to the stored index)
    ///   are re-trimmed to keep the prefix stable for prompt caching.
    /// - The index only **advances** when estimated tokens exceed
    ///   `context_window × threshold` again — it never moves backward.
    /// - When under threshold and no previous trimming exists, messages are
    ///   returned as-is with no metadata changes.
    pub fn reduce_context_with_budget(
        &self,
        messages: Vec<stakai::Message>,
        context_window: u64,
        metadata: Option<serde_json::Value>,
        tools: Option<&[stakai::Tool]>,
    ) -> (Vec<stakai::Message>, Option<serde_json::Value>) {
        // Standard processing: clean, convert, merge, dedup
        let llm_messages: Vec<_> = messages
            .into_iter()
            .map(Self::clean_checkpoint_tags)
            .collect();

        let llm_messages = merge_consecutive_same_role(llm_messages);
        let mut llm_messages = dedup_tool_results(llm_messages);

        let tool_overhead = Self::estimate_tool_overhead(tools);
        let threshold = (context_window as f32 * self.context_budget_threshold) as u64;

        // Read previous trimming state from metadata
        let prev_trimmed_up_to = metadata
            .as_ref()
            .and_then(|m| m.get("trimmed_up_to_message_index"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        // Fast path: under threshold and no previous trimming → return as-is
        if prev_trimmed_up_to == 0
            && Self::estimate_tokens(&llm_messages) + tool_overhead <= threshold
        {
            return (llm_messages, metadata);
        }

        // Determine the trim boundary based on keep_last_n_assistant_messages.
        //
        // Walk backwards through messages, counting assistant-role messages.
        // Once we've counted `keep_last_n_assistant_messages` of them, the
        // current message is the first one we want to keep — set trim_end
        // there so everything before it (that isn't user/system) gets trimmed.
        //
        // When keep_last_n is 0, skip the loop entirely: trim_end stays at
        // len, meaning every assistant/tool message is eligible for trimming.
        //
        // If the loop finishes without finding enough assistants, trim_end
        // stays at 0 — meaning keep_last_n alone wouldn't trim anything.
        let len = llm_messages.len();
        let mut keep_n_trim_end = if self.keep_last_n_assistant_messages > 0 {
            0 // default: don't trim anything (not enough assistants to exceed keep_last_n)
        } else {
            len // keep_last_n is 0: trim everything
        };
        if self.keep_last_n_assistant_messages > 0 {
            let mut assistant_count = 0usize;
            for i in (0..len).rev() {
                if llm_messages[i].role == stakai::Role::Assistant {
                    assistant_count += 1;
                    if assistant_count >= self.keep_last_n_assistant_messages {
                        keep_n_trim_end = i;
                        break;
                    }
                }
            }
        }

        // Re-apply previous trimming first so we can estimate the *effective*
        // token count (what the API actually sees). This keeps the prefix
        // stable for prompt caching.
        let prev_clamped = prev_trimmed_up_to.min(len);
        for msg in &mut llm_messages[..prev_clamped] {
            if msg.role == stakai::Role::System || msg.role == stakai::Role::User {
                continue;
            }
            Self::trim_message(msg);
        }

        // Re-estimate tokens after applying previous trimming. This reflects
        // the actual token count the API will see, so we only advance the
        // trim boundary when the *effective* content exceeds the threshold —
        // not just because the raw (untrimmed) history grew.
        let effective_estimated_tokens = Self::estimate_tokens(&llm_messages) + tool_overhead;

        // Decide whether to advance the trim index or keep the old one.
        // The index only advances when the effective tokens (after re-applying
        // previous trims) still exceed the threshold.
        let effective_trim_end = if effective_estimated_tokens > threshold {
            // Over budget — we must trim. Start with keep_last_n boundary
            // if available, then fall through to budget-driven scanning if
            // that boundary alone isn't sufficient.
            //
            // Step 1: Apply keep_last_n trimming if it provides a boundary.
            let mut candidate = if keep_n_trim_end > 0 {
                // Trim up to the keep_last_n boundary first
                for msg in llm_messages
                    .iter_mut()
                    .take(keep_n_trim_end.min(len))
                    .skip(prev_clamped)
                {
                    let role = msg.role;
                    if role == stakai::Role::Assistant || role == stakai::Role::Tool {
                        Self::trim_message(msg);
                    }
                }
                keep_n_trim_end
            } else {
                prev_trimmed_up_to
            };

            // Step 2: If still over budget after keep_last_n trimming,
            // continue scanning forward — budget is the HARD constraint,
            // keep_last_n is best-effort. This handles the case where the
            // last N assistant messages themselves exceed the budget (e.g.,
            // long tool results, large file contents).
            let current_tokens = Self::estimate_tokens(&llm_messages) + tool_overhead;
            if current_tokens > threshold {
                let mut scan_idx = candidate;
                while scan_idx < len {
                    let role = llm_messages[scan_idx].role;
                    if role == stakai::Role::Assistant || role == stakai::Role::Tool {
                        Self::trim_message(&mut llm_messages[scan_idx]);
                        candidate = scan_idx + 1;

                        // Check if we're under budget now
                        let current_tokens = Self::estimate_tokens(&llm_messages) + tool_overhead;
                        if current_tokens <= threshold {
                            break;
                        }
                    }
                    scan_idx += 1;
                }
            }

            // Never go backward
            candidate.max(prev_trimmed_up_to)
        } else {
            // Under budget — keep the previous trim boundary, don't advance
            prev_trimmed_up_to
        };

        // Apply trimming for any newly-advanced range (prev_clamped..effective_trim_end).
        // The previous range (0..prev_clamped) was already trimmed above.
        let clamped_end = effective_trim_end.min(len);
        if clamped_end > prev_clamped {
            for msg in &mut llm_messages[prev_clamped..clamped_end] {
                if msg.role == stakai::Role::System || msg.role == stakai::Role::User {
                    continue;
                }
                Self::trim_message(msg);
            }
        }

        // Update metadata — only write a new index when it actually advanced
        let new_trimmed_up_to = effective_trim_end;
        let mut updated_metadata = metadata.unwrap_or(serde_json::json!({}));
        if let Some(obj) = updated_metadata.as_object_mut() {
            obj.insert(
                "trimmed_up_to_message_index".to_string(),
                serde_json::json!(new_trimmed_up_to),
            );
        }

        (llm_messages, Some(updated_metadata))
    }
}

/// Merge consecutive messages that share the same role into a single message.
///
/// When the assistant returns N tool_calls, the chat history contains N separate
/// `role=tool` messages. Provider conversion layers map `tool` → `user`, which
/// creates N consecutive `user` messages — invalid for Anthropic.  By merging
/// them here into a single `role=tool` StakAI message with multiple tool results.
/// content parts, the downstream conversion produces one `user` message with
/// all the tool_result blocks.
fn merge_consecutive_same_role(messages: Vec<stakai::Message>) -> Vec<stakai::Message> {
    if messages.is_empty() {
        return messages;
    }

    let mut result: Vec<stakai::Message> = Vec::with_capacity(messages.len());

    for msg in messages {
        let should_merge = result.last().is_some_and(|prev| prev.role == msg.role);

        if should_merge {
            let prev = result.last_mut().expect("checked above");
            let new_parts = msg.content.parts();
            prev.content = match std::mem::replace(
                &mut prev.content,
                stakai::MessageContent::Text(String::new()),
            ) {
                stakai::MessageContent::Text(s) if s.is_empty() => {
                    stakai::MessageContent::Parts(new_parts)
                }
                stakai::MessageContent::Text(s) => {
                    let mut parts = vec![stakai::ContentPart::text(s)];
                    parts.extend(new_parts);
                    stakai::MessageContent::Parts(parts)
                }
                stakai::MessageContent::Parts(mut existing) => {
                    existing.extend(new_parts);
                    stakai::MessageContent::Parts(existing)
                }
            };
        } else {
            result.push(msg);
        }
    }

    result
}

/// Remove duplicate ToolResult entries that share the same tool_use_id.
/// Keeps only the **last** occurrence (the most recent / retried result).
fn dedup_tool_results(mut messages: Vec<stakai::Message>) -> Vec<stakai::Message> {
    for msg in &mut messages {
        if msg.role != stakai::Role::Tool {
            continue;
        }
        let parts = match &mut msg.content {
            stakai::MessageContent::Parts(p) => p,
            _ => continue,
        };

        // Track last index for each tool_use_id
        let mut last_index: HashMap<String, usize> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (i, part) in parts.iter().enumerate() {
            if let stakai::ContentPart::ToolResult { tool_call_id, .. } = part {
                last_index.insert(tool_call_id.clone(), i);
                *counts.entry(tool_call_id.clone()).or_insert(0) += 1;
            }
        }

        // Only filter if there are actual duplicates
        let has_dups = counts.values().any(|&c| c > 1);
        if !has_dups {
            continue;
        }

        let mut idx = 0;
        parts.retain(|part| {
            let keep = if let stakai::ContentPart::ToolResult { tool_call_id, .. } = part {
                if counts.get(tool_call_id).copied().unwrap_or(0) > 1 {
                    // Duplicate — keep only the last one
                    last_index.get(tool_call_id).copied() == Some(idx)
                } else {
                    true
                }
            } else {
                true
            };
            idx += 1;
            keep
        });
    }

    messages
}

pub struct TaskBoardContextManagerOptions {
    /// How many recent **assistant** messages to keep untrimmed when context
    /// trimming is triggered. The trim boundary is placed just before the
    /// Nth-from-last assistant message — all messages after that point
    /// (including interleaved user/tool messages) are left untouched.
    /// User and system messages are never trimmed regardless of position.
    pub keep_last_n_assistant_messages: usize,
    /// Fraction of context window at which trimming triggers (e.g., 0.8 = 80%)
    pub context_budget_threshold: f32,
}

impl TaskBoardContextManager {
    pub fn new(options: TaskBoardContextManagerOptions) -> Self {
        Self {
            keep_last_n_assistant_messages: options.keep_last_n_assistant_messages,
            context_budget_threshold: options.context_budget_threshold,
        }
    }
}
