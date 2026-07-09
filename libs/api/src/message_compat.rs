use serde::{Deserialize, Deserializer};
use serde_json::Value;
use stakai::{ContentPart, ImageDetail, Message, MessageContent, Role};

pub(crate) fn deserialize_messages<'de, D>(deserializer: D) -> Result<Vec<Message>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    values
        .into_iter()
        .map(deserialize_message)
        .collect::<Result<Vec<_>, _>>()
        .map_err(serde::de::Error::custom)
}

fn deserialize_message(value: Value) -> Result<Message, String> {
    if requires_legacy_migration(&value) {
        return migrate_legacy_message(&value)
            .ok_or_else(|| "Invalid legacy checkpoint message".to_string());
    }

    serde_json::from_value::<Message>(value.clone())
        .or_else(|error| migrate_legacy_message(&value).ok_or_else(|| error.to_string()))
}

fn requires_legacy_migration(value: &Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };

    obj.contains_key("tool_calls")
        || obj.contains_key("tool_call_id")
        || obj.get("role").and_then(Value::as_str) == Some("developer")
        || obj
            .get("content")
            .is_some_and(content_contains_legacy_parts)
}

fn content_contains_legacy_parts(content: &Value) -> bool {
    let Value::Array(parts) = content else {
        return false;
    };

    parts.iter().any(|part| {
        let Some(part) = part.as_object() else {
            return false;
        };
        part.contains_key("image_url")
            || part.get("type").and_then(Value::as_str) == Some("image_url")
    })
}

fn migrate_legacy_message(value: &Value) -> Option<Message> {
    let obj = value.as_object()?;
    let role = legacy_role(
        obj.get("role")
            .and_then(Value::as_str)
            .unwrap_or("assistant"),
    );
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    if role == Role::Tool
        && let Some(tool_call_id) = obj.get("tool_call_id").and_then(Value::as_str)
    {
        let mut message = Message::new(
            role,
            MessageContent::Parts(vec![ContentPart::tool_result(
                tool_call_id,
                legacy_tool_result_content(obj.get("content")),
            )]),
        );
        message.name = name;
        return Some(message);
    }

    let mut parts = legacy_content_parts(obj.get("content"));
    let legacy_tool_calls = legacy_tool_calls(obj.get("tool_calls"));
    let has_legacy_tool_calls = !legacy_tool_calls.is_empty();
    parts.extend(legacy_tool_calls);

    let content = match parts.as_slice() {
        [] => MessageContent::Text(String::new()),
        [ContentPart::Text { text, .. }] if !has_legacy_tool_calls => {
            MessageContent::Text(text.clone())
        }
        _ => MessageContent::Parts(parts),
    };

    let mut message = Message::new(role, content);
    message.name = name;
    Some(message)
}

fn legacy_role(role: &str) -> Role {
    match role {
        "system" => Role::System,
        "developer" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

fn legacy_content_parts(content: Option<&Value>) -> Vec<ContentPart> {
    match content {
        Some(Value::String(text)) if !text.is_empty() => vec![ContentPart::text(text.clone())],
        Some(Value::Array(parts)) => parts.iter().filter_map(legacy_content_part).collect(),
        _ => Vec::new(),
    }
}

fn legacy_content_part(part: &Value) -> Option<ContentPart> {
    if let Ok(part) = serde_json::from_value::<ContentPart>(part.clone()) {
        return Some(part);
    }

    let obj = part.as_object()?;

    if let Some(text) = obj.get("text").and_then(Value::as_str) {
        return Some(ContentPart::text(text.to_string()));
    }

    let image_url = obj.get("image_url")?.as_object()?;
    let url = image_url.get("url")?.as_str()?.to_string();
    let detail = image_url
        .get("detail")
        .and_then(Value::as_str)
        .and_then(legacy_image_detail);

    Some(ContentPart::Image {
        url,
        detail,
        provider_options: None,
    })
}

fn legacy_image_detail(detail: &str) -> Option<ImageDetail> {
    match detail {
        "low" => Some(ImageDetail::Low),
        "high" => Some(ImageDetail::High),
        "auto" => Some(ImageDetail::Auto),
        _ => None,
    }
}

fn legacy_tool_calls(tool_calls: Option<&Value>) -> Vec<ContentPart> {
    let Some(Value::Array(tool_calls)) = tool_calls else {
        return Vec::new();
    };

    tool_calls
        .iter()
        .filter_map(|tool_call| {
            let obj = tool_call.as_object()?;
            let id = obj.get("id").and_then(Value::as_str)?.to_string();
            let function = obj.get("function")?.as_object()?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments = legacy_tool_arguments(function.get("arguments"));
            let metadata = obj.get("metadata").cloned();

            let mut part = ContentPart::tool_call(id, name, arguments);
            if let ContentPart::ToolCall {
                metadata: part_metadata,
                ..
            } = &mut part
            {
                *part_metadata = metadata;
            }
            Some(part)
        })
        .collect()
}

fn legacy_tool_arguments(arguments: Option<&Value>) -> Value {
    match arguments {
        Some(Value::String(raw)) => {
            serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()))
        }
        Some(value) => value.clone(),
        None => Value::Object(serde_json::Map::new()),
    }
}

fn legacy_tool_result_content(content: Option<&Value>) -> Value {
    match content {
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    part.as_object()
                        .and_then(|obj| obj.get("text"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join("\n");
            Value::String(text)
        }
        Some(Value::Null) | None => Value::String(String::new()),
        Some(value) => value.clone(),
    }
}
