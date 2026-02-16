use crate::utils::agents_md::{AgentsMdInfo, format_agents_md_for_context};
use crate::utils::apps_md::{AppsMdInfo, format_apps_md_for_context};
use crate::utils::local_context::LocalContext;
use stakpak_api::models::ListRuleBook;
use stakpak_shared::models::integrations::openai::{
    ChatMessage, FunctionDefinition, MessageContent, Role, Tool, ToolCallResult,
};
use uuid::Uuid;

/// Build a CLI resume command string, preferring session ID over checkpoint ID.
pub fn build_resume_command(
    session_id: Option<Uuid>,
    checkpoint_id: Option<Uuid>,
) -> Option<String> {
    if let Some(session_id) = session_id {
        return Some(format!("stakpak -s {}", session_id));
    }
    checkpoint_id.map(|checkpoint_id| format!("stakpak -c {}", checkpoint_id))
}

/// Extract the checkpoint ID from the last assistant message that contains one.
pub fn extract_last_checkpoint_id(messages: &[ChatMessage]) -> Option<Uuid> {
    messages
        .iter()
        .rev()
        .filter(|m| m.role == Role::Assistant)
        .find_map(|m| {
            m.content
                .as_ref()
                .and_then(MessageContent::extract_checkpoint_id)
        })
}

pub fn convert_tools_with_filter(
    tools: &[rmcp::model::Tool],
    allowed_tools: Option<&Vec<String>>,
) -> Vec<Tool> {
    tools
        .iter()
        .filter_map(|tool| {
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
        .collect()
}

pub fn user_message(user_input: String) -> ChatMessage {
    ChatMessage {
        role: Role::User,
        content: Some(MessageContent::String(user_input)),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
        ..Default::default()
    }
}

pub fn system_message(system_prompt: String) -> ChatMessage {
    ChatMessage {
        role: Role::System,
        content: Some(MessageContent::String(system_prompt)),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        usage: None,
        ..Default::default()
    }
}

pub fn tool_result(tool_call_id: String, result: String) -> ChatMessage {
    ChatMessage {
        role: Role::Tool,
        content: Some(MessageContent::String(result)),
        name: None,
        tool_calls: None,
        tool_call_id: Some(tool_call_id),
        usage: None,
        ..Default::default()
    }
}

pub async fn add_local_context<'a>(
    messages: &'a [ChatMessage],
    user_input: &'a str,
    local_context: &'a Option<LocalContext>,
    force_add: bool,
) -> Result<(String, Option<&'a LocalContext>), Box<dyn std::error::Error>> {
    if let Some(local_context) = local_context {
        // Add local context if this is the first message OR if force_add is true
        let is_first_message = messages
            .iter()
            .filter(|m: &&ChatMessage| m.role != Role::System)
            .count()
            == 0;

        if is_first_message || force_add {
            let context_display = local_context.format_display().await?;
            let formatted_input = format!(
                "{}\n<local_context>\n{}\n</local_context>",
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

pub fn add_rulebooks(user_input: &str, rulebooks: &[ListRuleBook]) -> (String, Option<String>) {
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

    let formatted_input = format!(
        "{}\n<rulebooks>\n{}\n</rulebooks>",
        user_input, rulebooks_text
    );
    (formatted_input, Some(rulebooks_text))
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

pub fn add_agents_md(user_input: &str, agents_md: &AgentsMdInfo) -> (String, String) {
    let agents_text = format_agents_md_for_context(agents_md);
    let formatted_input = format!("{}\n<agents_md>\n{}\n</agents_md>", user_input, agents_text);
    (formatted_input, agents_text)
}

pub fn add_apps_md(user_input: &str, apps_md: &AppsMdInfo) -> (String, String) {
    let apps_text = format_apps_md_for_context(apps_md);
    let formatted_input = format!("{}\n<apps_md>\n{}\n</apps_md>", user_input, apps_text);
    (formatted_input, apps_text)
}

/// Refresh billing info and send it to the TUI.
/// This is used to update the balance display after assistant messages.
pub async fn refresh_billing_info(
    client: &dyn stakpak_api::AgentProvider,
    input_tx: &tokio::sync::mpsc::Sender<stakpak_tui::InputEvent>,
) {
    if let Ok(account_data) = client.get_my_account().await {
        let billing_username = account_data
            .scope
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or(&account_data.username);

        if let Ok(billing_info) = client.get_billing_info(billing_username).await {
            let _ = crate::commands::agent::run::tui::send_input_event(
                input_tx,
                stakpak_tui::InputEvent::BillingInfoLoaded(billing_info),
            )
            .await;
        }
    }
}
