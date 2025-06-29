use crate::utils::local_context::LocalContext;
use stakpak_api::ListRuleBook;
use stakpak_shared::models::integrations::openai::{
    ChatMessage, FunctionDefinition, MessageContent, Role, Tool, ToolCallResult,
};

pub fn convert_tools_map(
    tools_map: &std::collections::HashMap<String, Vec<rmcp::model::Tool>>,
) -> Vec<Tool> {
    tools_map
        .iter()
        .flat_map(|(_name, tools)| {
            tools.iter().map(|tool| Tool {
                r#type: "function".to_string(),
                function: FunctionDefinition {
                    name: tool.name.clone().into_owned(),
                    description: tool.description.clone().map(|d| d.to_string()),
                    parameters: serde_json::Value::Object((*tool.input_schema).clone()),
                },
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

pub fn tool_result(tool_call_id: String, result: String) -> ChatMessage {
    ChatMessage {
        role: Role::Tool,
        content: Some(MessageContent::String(result)),
        name: None,
        tool_calls: None,
        tool_call_id: Some(tool_call_id),
    }
}

pub fn add_local_context<'a>(
    messages: &'a [ChatMessage],
    user_input: &'a str,
    local_context: &'a Option<LocalContext>,
) -> (String, Option<&'a LocalContext>) {
    if let Some(local_context) = local_context {
        // only add local context if this is the first message
        if messages.is_empty() {
            let formatted_input = format!(
                "{}\n\n<local_context>\n{}\n</local_context>",
                user_input, local_context
            );
            (formatted_input, Some(local_context))
        } else {
            (user_input.to_string(), None)
        }
    } else {
        (user_input.to_string(), None)
    }
}

pub fn add_rulebooks(
    messages: &[ChatMessage],
    user_input: &str,
    rulebooks: &Option<Vec<ListRuleBook>>,
) -> (String, Option<String>) {
    if let Some(rulebooks) = rulebooks {
        let rulebooks_text = if !rulebooks.is_empty() {
            format!(
                "# User Rule Books:\n{}",
                rulebooks
                    .iter()
                    .map(|rulebook| format!("  - {}", rulebook.to_text().replace('\n', "\n    ")))
                    .collect::<Vec<String>>()
                    .join("\n")
            )
        } else {
            "# No User Rule Books available".to_string()
        };

        // only add local context if this is the first message
        if messages.is_empty() {
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
    Some(format!(
        "Here is the history of commands run before this message:\n{}",
        history
    ))
}
