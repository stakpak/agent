use crate::utils::local_context::LocalContext;
use stakpak_api::ListRuleBook;
use stakpak_shared::models::integrations::openai::{
    ChatMessage, FunctionDefinition, MessageContent, Role, Tool, ToolCallResult,
};
use stakpak_shared::models::subagent::SubagentConfigs;

pub fn convert_tools_map_with_filter(
    tools_map: &std::collections::HashMap<String, Vec<rmcp::model::Tool>>,
    allowed_tools: Option<&Vec<String>>,
) -> Vec<Tool> {
    tools_map
        .iter()
        .flat_map(|(_name, tools)| {
            tools.iter().filter_map(|tool| {
                let tool_name = tool.name.as_ref();

                // Filter tools based on allowed_tools if specified
                if let Some(allowed) = allowed_tools
                    && !allowed.is_empty()
                    && !allowed.contains(&tool_name.to_string())
                {
                    return None;
                }

                Some(Tool {
                    r#type: "function".to_string(),
                    function: FunctionDefinition {
                        name: tool_name.to_owned(),
                        description: tool.description.clone().map(|d| d.to_string()),
                        parameters: serde_json::Value::Object((*tool.input_schema).clone()),
                    },
                })
            })
        })
        .collect()
}

pub fn user_message(user_input: String) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: Some(MessageContent::String(user_input)),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

pub fn system_message(system_prompt: String) -> ChatMessage {
    ChatMessage {
        role: Role::System,
        content: Some(MessageContent::String(system_prompt)),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

pub fn tool_result(tool_call_id: String, result: String) -> ChatMessage {
    ChatMessage {
        role: Role::Tool,
        content: Some(MessageContent::String(result)),
        name: None,
        tool_calls: None,
        tool_call_id: Some(tool_call_id),
    }
}

pub async fn add_local_context<'a>(
    messages: &'a [ChatMessage],
    user_input: &'a str,
    local_context: &'a Option<LocalContext>,
) -> Result<(String, Option<&'a LocalContext>), Box<dyn std::error::Error>> {
    if let Some(local_context) = local_context {
        // only add local context if this is the first message
        if messages
            .iter()
            .filter(|m: &&ChatMessage| m.role != Role::System)
            .count()
            == 0
        {
            let context_display = local_context.format_display().await?;
            let formatted_input = format!(
                "{}\n\n<local_context>\n{}\n</local_context>",
                user_input, context_display
            );
            Ok((formatted_input, Some(local_context)))
        } else {
            Ok((user_input.to_string(), None))
        }
    } else {
        Ok((user_input.to_string(), None))
    }
}

pub fn add_rulebooks(
    messages: &[ChatMessage],
    user_input: &str,
    rulebooks: &Option<Vec<ListRuleBook>>,
) -> (String, Option<String>) {
    add_rulebooks_with_force(messages, user_input, rulebooks, false)
}

pub fn add_rulebooks_with_force(
    messages: &[ChatMessage],
    user_input: &str,
    rulebooks: &Option<Vec<ListRuleBook>>,
    force_add: bool,
) -> (String, Option<String>) {
    if let Some(rulebooks) = rulebooks {
        let rulebooks_text = if !rulebooks.is_empty() {
            format!(
                "\n\n# My Rule Books:\n\n{}",
                rulebooks
                    .iter()
                    .map(|rulebook| {
                        let text = rulebook.to_text();
                        let mut lines = text.lines();
                        let mut result = String::new();
                        if let Some(first) = lines.next() {
                            result.push_str(&format!("  - {}", first));
                            for line in lines {
                                result.push_str(&format!("\n    {}", line));
                            }
                        }
                        result
                    })
                    .collect::<Vec<String>>()
                    .join("\n")
            )
        } else {
            "# No Rule Books Available".to_string()
        };

        // Add rulebooks if this is the first message OR if force_add is true
        if messages.is_empty() || force_add {
            let formatted_input = format!(
                "{}\n\n<rulebooks>\n{}\n</rulebooks>",
                user_input, rulebooks_text
            );
            (formatted_input, Some(rulebooks_text))
        } else {
            (user_input.to_string(), None)
        }
    } else {
        (user_input.to_string(), None)
    }
}

pub fn add_subagents(
    messages: &[ChatMessage],
    user_input: &str,
    subagent_configs: &Option<SubagentConfigs>,
) -> (String, Option<String>) {
    if let Some(subagent_configs) = subagent_configs {
        let subagents_text = subagent_configs.format_for_context();

        if messages.is_empty() {
            let formatted_input = format!(
                "{}\n\n<subagents>\n{}\n</subagents>",
                user_input, subagents_text
            );
            (formatted_input, Some(subagents_text))
        } else {
            (user_input.to_string(), None)
        }
    } else {
        (user_input.to_string(), None)
    }
}

pub fn tool_call_history_string(tool_calls: &[ToolCallResult]) -> Option<String> {
    if tool_calls.is_empty() {
        return None;
    }
    let history = tool_calls
        .iter()
        .map(|tc| {
            let command = if let Ok(json) =
                serde_json::from_str::<serde_json::Value>(&tc.call.function.arguments)
            {
                json.get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&tc.call.function.arguments)
                    .to_string()
            } else {
                tc.call.function.arguments.clone()
            };

            let output = if tc.result.trim().is_empty() {
                "No output".to_string()
            } else {
                tc.result.clone()
            };
            format!("```shell\n$ {}\n{}\n```", command, output)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!("Here's my shell history:\n{}", history))
}
