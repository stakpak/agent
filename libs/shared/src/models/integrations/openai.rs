use crate::models::llm::{LLMMessage, LLMMessageContent, LLMMessageTypedContent};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
    Tool,
    // Function,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => write!(f, "system"),
            Role::Developer => write!(f, "developer"),
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopSequence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl ChatCompletionRequest {
    pub fn new(messages: Vec<ChatMessage>, tools: Option<Vec<Tool>>, stream: Option<bool>) -> Self {
        Self {
            model: "pablo-v1".to_string(),
            messages,
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            max_tokens: None,
            n: None,
            presence_penalty: None,
            response_format: None,
            seed: None,
            stop: None,
            stream,
            temperature: None,
            top_p: None,
            tools,
            tool_choice: None,
            user: None,
            context: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn last_server_message(messages: &[ChatMessage]) -> Option<&ChatMessage> {
        messages
            .iter()
            .rev()
            .find(|message| message.role != Role::User && message.role != Role::Tool)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    String(String),
    Array(Vec<ContentPart>),
}

impl MessageContent {
    pub fn inject_checkpoint_id(&self, checkpoint_id: Uuid) -> Self {
        match self {
            MessageContent::String(s) => MessageContent::String(format!(
                "<checkpoint_id>{}</checkpoint_id>\n{}",
                checkpoint_id, s
            )),
            MessageContent::Array(parts) => MessageContent::Array(
                std::iter::once(ContentPart {
                    r#type: "text".to_string(),
                    text: Some(format!("<checkpoint_id>{}</checkpoint_id>", checkpoint_id)),
                    image_url: None,
                })
                .chain(parts.iter().cloned())
                .collect(),
            ),
        }
    }

    pub fn extract_checkpoint_id(&self) -> Option<Uuid> {
        match self {
            MessageContent::String(s) => s
                .rfind("<checkpoint_id>")
                .and_then(|start| {
                    s[start..]
                        .find("</checkpoint_id>")
                        .map(|end| (start + "<checkpoint_id>".len(), start + end))
                })
                .and_then(|(start, end)| Uuid::parse_str(&s[start..end]).ok()),
            MessageContent::Array(parts) => parts.iter().rev().find_map(|part| {
                part.text.as_deref().and_then(|text| {
                    text.rfind("<checkpoint_id>")
                        .and_then(|start| {
                            text[start..]
                                .find("</checkpoint_id>")
                                .map(|end| (start + "<checkpoint_id>".len(), start + end))
                        })
                        .and_then(|(start, end)| Uuid::parse_str(&text[start..end]).ok())
                })
            }),
        }
    }
}

impl std::fmt::Display for MessageContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageContent::String(s) => write!(f, "{}", s),
            MessageContent::Array(parts) => {
                let text_parts: Vec<String> =
                    parts.iter().filter_map(|part| part.text.clone()).collect();
                write!(f, "{}", text_parts.join("\n"))
            }
        }
    }
}
impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::String(String::new())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ContentPart {
    pub r#type: String,
    pub text: Option<String>,
    pub image_url: Option<ImageUrl>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ResponseFormat {
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum StopSequence {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Tool {
    pub r#type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolChoice {
    Auto,
    Required,
    Object(ToolChoiceObject),
}

impl Serialize for ToolChoice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ToolChoice::Auto => serializer.serialize_str("auto"),
            ToolChoice::Required => serializer.serialize_str("required"),
            ToolChoice::Object(obj) => obj.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ToolChoice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ToolChoiceVisitor;

        impl<'de> serde::de::Visitor<'de> for ToolChoiceVisitor {
            type Value = ToolChoice;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("string or object")
            }

            fn visit_str<E>(self, value: &str) -> Result<ToolChoice, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "auto" => Ok(ToolChoice::Auto),
                    "required" => Ok(ToolChoice::Required),
                    _ => Err(serde::de::Error::unknown_variant(
                        value,
                        &["auto", "required"],
                    )),
                }
            }

            fn visit_map<M>(self, map: M) -> Result<ToolChoice, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let obj = ToolChoiceObject::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(ToolChoice::Object(obj))
            }
        }

        deserializer.deserialize_any(ToolChoiceVisitor)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolChoiceObject {
    pub r#type: String,
    pub function: FunctionChoice,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionChoice {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCallResult {
    pub call: ToolCall,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallResultProgress {
    pub id: Uuid,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: Usage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub logprobs: Option<LogProbs>,
    pub finish_reason: FinishReason,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LogProbs {
    pub content: Option<Vec<LogProbContent>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LogProbContent {
    pub token: String,
    pub logprob: f32,
    pub bytes: Option<Vec<u8>>,
    pub top_logprobs: Option<Vec<TokenLogprob>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TokenLogprob {
    pub token: String,
    pub logprob: f32,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionStreamResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionStreamChoice>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatCompletionStreamChoice {
    pub index: usize,
    pub delta: ChatMessageDelta,
    pub finish_reason: Option<FinishReason>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatMessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub r#type: Option<String>,
    pub function: Option<FunctionCallDelta>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

impl From<LLMMessage> for ChatMessage {
    fn from(llm_message: LLMMessage) -> Self {
        let role = match llm_message.role.as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            // "function" => Role::Function,
            "developer" => Role::Developer,
            _ => Role::User, // Default to user for unknown roles
        };

        let (content, tool_calls) = match llm_message.content {
            LLMMessageContent::String(text) => (Some(MessageContent::String(text)), None),
            LLMMessageContent::List(items) => {
                let mut text_parts = Vec::new();
                let mut tool_call_parts = Vec::new();

                for item in items {
                    match item {
                        LLMMessageTypedContent::Text { text } => {
                            text_parts.push(ContentPart {
                                r#type: "text".to_string(),
                                text: Some(text),
                                image_url: None,
                            });
                        }
                        LLMMessageTypedContent::ToolCall { id, name, args } => {
                            tool_call_parts.push(ToolCall {
                                id,
                                r#type: "function".to_string(),
                                function: FunctionCall {
                                    name,
                                    arguments: args.to_string(),
                                },
                            });
                        }
                    }
                }

                let content = if !text_parts.is_empty() {
                    Some(MessageContent::Array(text_parts))
                } else {
                    None
                };

                let tool_calls = if !tool_call_parts.is_empty() {
                    Some(tool_call_parts)
                } else {
                    None
                };

                (content, tool_calls)
            }
        };

        ChatMessage {
            role,
            content,
            name: None, // LLMMessage doesn't have a name field
            tool_calls,
            tool_call_id: None, // LLMMessage doesn't have a tool_call_id field
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_basic_request() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: Some(MessageContent::String(
                        "You are a helpful assistant.".to_string(),
                    )),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: Role::User,
                    content: Some(MessageContent::String("Hello!".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            max_tokens: Some(100),
            n: None,
            presence_penalty: None,
            response_format: None,
            seed: None,
            stop: None,
            stream: None,
            temperature: Some(0.7),
            top_p: None,
            tools: None,
            tool_choice: None,
            user: None,
            context: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"gpt-4\""));
        assert!(json.contains("\"messages\":["));
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("\"content\":\"You are a helpful assistant.\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello!\""));
        assert!(json.contains("\"max_tokens\":100"));
        assert!(json.contains("\"temperature\":0.7"));
    }

    #[test]
    fn test_deserialize_response() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-3.5-turbo",
            "system_fingerprint": "fp_123abc",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help you today?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 12,
                "total_tokens": 21
            }
        }"#;

        let response: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "chatcmpl-123");
        assert_eq!(response.object, "chat.completion");
        assert_eq!(response.created, 1677652288);
        assert_eq!(response.model, "gpt-3.5-turbo");
        assert_eq!(response.system_fingerprint, Some("fp_123abc".to_string()));

        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].index, 0);
        assert_eq!(response.choices[0].message.role, Role::Assistant);

        match &response.choices[0].message.content {
            Some(MessageContent::String(content)) => {
                assert_eq!(content, "Hello! How can I help you today?");
            }
            _ => panic!("Expected string content"),
        }

        assert_eq!(response.choices[0].finish_reason, FinishReason::Stop);
        assert_eq!(response.usage.prompt_tokens, 9);
        assert_eq!(response.usage.completion_tokens, 12);
        assert_eq!(response.usage.total_tokens, 21);
    }

    #[test]
    fn test_tool_calls_request_response() {
        // Test a request with tools
        let tools_request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(
                    "What's the weather in San Francisco?".to_string(),
                )),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: Some(vec![Tool {
                r#type: "function".to_string(),
                function: FunctionDefinition {
                    name: "get_weather".to_string(),
                    description: Some("Get the current weather in a given location".to_string()),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "The city and state, e.g. San Francisco, CA"
                            }
                        },
                        "required": ["location"]
                    }),
                },
            }]),
            tool_choice: Some(ToolChoice::Auto),
            max_tokens: Some(100),
            temperature: Some(0.7),
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            n: None,
            presence_penalty: None,
            response_format: None,
            seed: None,
            stop: None,
            stream: None,
            top_p: None,
            user: None,
            context: None,
        };

        let json = serde_json::to_string(&tools_request).unwrap();
        println!("Tool request JSON: {}", json);

        assert!(json.contains("\"tools\":["));
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("\"name\":\"get_weather\""));
        // Auto should be serialized as "auto" (string)
        assert!(json.contains("\"tool_choice\":\"auto\""));

        // Test response with tool calls
        let tool_response_json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-3.5-turbo",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_abc123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"location\":\"San Francisco, CA\"}"
                            }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 82,
                "completion_tokens": 17,
                "total_tokens": 99
            }
        }"#;

        let tool_response: ChatCompletionResponse =
            serde_json::from_str(tool_response_json).unwrap();
        assert_eq!(tool_response.choices[0].message.role, Role::Assistant);
        assert_eq!(tool_response.choices[0].message.content, None);
        assert!(tool_response.choices[0].message.tool_calls.is_some());

        let tool_calls = tool_response.choices[0]
            .message
            .tool_calls
            .as_ref()
            .unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_abc123");
        assert_eq!(tool_calls[0].r#type, "function");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert_eq!(
            tool_calls[0].function.arguments,
            "{\"location\":\"San Francisco, CA\"}"
        );

        assert_eq!(
            tool_response.choices[0].finish_reason,
            FinishReason::ToolCalls,
        );
    }

    #[test]
    fn test_content_with_image() {
        let message_with_image = ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Array(vec![
                ContentPart {
                    r#type: "text".to_string(),
                    text: Some("What's in this image?".to_string()),
                    image_url: None,
                },
                ContentPart {
                    r#type: "image_url".to_string(),
                    text: None,
                    image_url: Some(ImageUrl {
                        url: "data:image/jpeg;base64,/9j/4AAQSkZ...".to_string(),
                        detail: Some("low".to_string()),
                    }),
                },
            ])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        };

        let json = serde_json::to_string(&message_with_image).unwrap();
        println!("Serialized JSON: {}", json);

        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"What's in this image?\""));
        assert!(json.contains("\"type\":\"image_url\""));
        assert!(json.contains("\"url\":\"data:image/jpeg;base64,/9j/4AAQSkZ...\""));
        assert!(json.contains("\"detail\":\"low\""));
    }

    #[test]
    fn test_response_format() {
        let json_format_request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: Some(MessageContent::String(
                    "Generate a JSON object with name and age fields".to_string(),
                )),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            response_format: Some(ResponseFormat {
                r#type: "json_object".to_string(),
            }),
            max_tokens: Some(100),
            temperature: None,
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            n: None,
            presence_penalty: None,
            seed: None,
            stop: None,
            stream: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            user: None,
            context: None,
        };

        let json = serde_json::to_string(&json_format_request).unwrap();
        assert!(json.contains("\"response_format\":{\"type\":\"json_object\"}"));
    }

    #[test]
    fn test_llm_message_to_chat_message() {
        // Test simple string content
        let llm_message = LLMMessage {
            role: "user".to_string(),
            content: LLMMessageContent::String("Hello, world!".to_string()),
        };

        let chat_message = ChatMessage::from(llm_message);
        assert_eq!(chat_message.role, Role::User);
        match &chat_message.content {
            Some(MessageContent::String(text)) => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected string content"),
        }
        assert_eq!(chat_message.tool_calls, None);

        // Test tool call conversion
        let llm_message_with_tool = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::List(vec![LLMMessageTypedContent::ToolCall {
                id: "call_123".to_string(),
                name: "get_weather".to_string(),
                args: serde_json::json!({"location": "San Francisco"}),
            }]),
        };

        let chat_message = ChatMessage::from(llm_message_with_tool);
        assert_eq!(chat_message.role, Role::Assistant);
        assert_eq!(chat_message.content, None); // No text content
        assert!(chat_message.tool_calls.is_some());

        let tool_calls = chat_message.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert!(tool_calls[0].function.arguments.contains("San Francisco"));

        // Test mixed content
        let llm_message_mixed = LLMMessage {
            role: "assistant".to_string(),
            content: LLMMessageContent::List(vec![
                LLMMessageTypedContent::Text {
                    text: "The weather is:".to_string(),
                },
                LLMMessageTypedContent::ToolCall {
                    id: "call_456".to_string(),
                    name: "get_weather".to_string(),
                    args: serde_json::json!({"location": "New York"}),
                },
            ]),
        };

        let chat_message = ChatMessage::from(llm_message_mixed);
        assert_eq!(chat_message.role, Role::Assistant);

        match &chat_message.content {
            Some(MessageContent::Array(parts)) => {
                assert_eq!(parts.len(), 1);
                assert_eq!(parts[0].r#type, "text");
                assert_eq!(parts[0].text, Some("The weather is:".to_string()));
            }
            _ => panic!("Expected array content"),
        }

        let tool_calls = chat_message.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_456");
        assert_eq!(tool_calls[0].function.name, "get_weather");
        assert!(tool_calls[0].function.arguments.contains("New York"));
    }
}
