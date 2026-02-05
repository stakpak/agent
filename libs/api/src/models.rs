use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rmcp::model::Content;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stakpak_shared::models::{
    integrations::openai::{
        AgentModel, ChatMessage, FunctionCall, MessageContent, Role, Tool, ToolCall,
    },
    llm::{LLMInput, LLMMessage, LLMMessageContent, LLMMessageTypedContent, LLMTokenUsage},
};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ApiStreamError {
    AgentInputInvalid(String),
    AgentStateInvalid,
    AgentNotSupported,
    AgentExecutionLimitExceeded,
    AgentInvalidResponseStream,
    InvalidGeneratedCode,
    CopilotError,
    SaveError,
    Unknown(String),
}

impl From<&str> for ApiStreamError {
    fn from(error_str: &str) -> Self {
        match error_str {
            s if s.contains("Agent not supported") => ApiStreamError::AgentNotSupported,
            s if s.contains("Agent state is not valid") => ApiStreamError::AgentStateInvalid,
            s if s.contains("Agent thinking limit exceeded") => {
                ApiStreamError::AgentExecutionLimitExceeded
            }
            s if s.contains("Invalid response stream") => {
                ApiStreamError::AgentInvalidResponseStream
            }
            s if s.contains("Invalid generated code") => ApiStreamError::InvalidGeneratedCode,
            s if s.contains(
                "Our copilot is handling too many requests at this time, please try again later.",
            ) =>
            {
                ApiStreamError::CopilotError
            }
            s if s
                .contains("An error occurred while saving your data. Please try again later.") =>
            {
                ApiStreamError::SaveError
            }
            s if s.contains("Agent input is not valid: ") => {
                ApiStreamError::AgentInputInvalid(s.replace("Agent input is not valid: ", ""))
            }
            _ => ApiStreamError::Unknown(error_str.to_string()),
        }
    }
}

impl From<String> for ApiStreamError {
    fn from(error_str: String) -> Self {
        ApiStreamError::from(error_str.as_str())
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Document {
    pub content: String,
    pub uri: String,
    pub provisioner: ProvisionerType,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SimpleDocument {
    pub uri: String,
    pub content: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Block {
    pub id: Uuid,
    pub provider: String,
    pub provisioner: ProvisionerType,
    pub language: String,
    pub key: String,
    pub digest: u64,
    pub references: Vec<Vec<Segment>>,
    pub kind: String,
    pub r#type: Option<String>,
    pub name: Option<String>,
    pub config: serde_json::Value,
    pub document_uri: String,
    pub code: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_point: Point,
    pub end_point: Point,
    pub state: Option<serde_json::Value>,
    pub updated_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub dependents: Vec<DependentBlock>,
    pub dependencies: Vec<Dependency>,
    pub api_group_version: Option<ApiGroupVersion>,

    pub generated_summary: Option<String>,
}

impl Block {
    pub fn get_uri(&self) -> String {
        format!(
            "{}#L{}-L{}",
            self.document_uri, self.start_point.row, self.end_point.row
        )
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub enum ProvisionerType {
    #[serde(rename = "Terraform")]
    Terraform,
    #[serde(rename = "Kubernetes")]
    Kubernetes,
    #[serde(rename = "Dockerfile")]
    Dockerfile,
    #[serde(rename = "GithubActions")]
    GithubActions,
    #[serde(rename = "None")]
    None,
}
impl std::str::FromStr for ProvisionerType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "terraform" => Ok(Self::Terraform),
            "kubernetes" => Ok(Self::Kubernetes),
            "dockerfile" => Ok(Self::Dockerfile),
            "github-actions" => Ok(Self::GithubActions),
            _ => Ok(Self::None),
        }
    }
}
impl std::fmt::Display for ProvisionerType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ProvisionerType::Terraform => write!(f, "terraform"),
            ProvisionerType::Kubernetes => write!(f, "kubernetes"),
            ProvisionerType::Dockerfile => write!(f, "dockerfile"),
            ProvisionerType::GithubActions => write!(f, "github-actions"),
            ProvisionerType::None => write!(f, "none"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(untagged)]
pub enum Segment {
    Key(String),
    Index(usize),
}

impl std::fmt::Display for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Segment::Key(key) => write!(f, "{}", key),
            Segment::Index(index) => write!(f, "{}", index),
        }
    }
}
impl std::fmt::Debug for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Segment::Key(key) => write!(f, "{}", key),
            Segment::Index(index) => write!(f, "{}", index),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Point {
    pub row: usize,
    pub column: usize,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct DependentBlock {
    pub key: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Dependency {
    pub id: Option<Uuid>,
    pub expression: Option<String>,
    pub from_path: Option<Vec<Segment>>,
    pub to_path: Option<Vec<Segment>>,
    #[serde(default = "Vec::new")]
    pub selectors: Vec<DependencySelector>,
    #[serde(skip_serializing)]
    pub key: Option<String>,
    pub digest: Option<u64>,
    #[serde(default = "Vec::new")]
    pub from: Vec<Segment>,
    pub from_field: Option<Vec<Segment>>,
    pub to_field: Option<Vec<Segment>>,
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
    pub start_point: Option<Point>,
    pub end_point: Option<Point>,
    pub satisfied: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DependencySelector {
    pub references: Vec<Vec<Segment>>,
    pub operator: DependencySelectorOperator,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum DependencySelectorOperator {
    Equals,
    NotEquals,
    In,
    NotIn,
    Exists,
    DoesNotExist,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiGroupVersion {
    pub alias: String,
    pub group: String,
    pub version: String,
    pub provisioner: ProvisionerType,
    pub status: APIGroupVersionStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum APIGroupVersionStatus {
    #[serde(rename = "UNAVAILABLE")]
    Unavailable,
    #[serde(rename = "PENDING")]
    Pending,
    #[serde(rename = "AVAILABLE")]
    Available,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BuildCodeIndexInput {
    pub documents: Vec<SimpleDocument>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IndexError {
    pub uri: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildCodeIndexOutput {
    pub blocks: Vec<Block>,
    pub errors: Vec<IndexError>,
    pub warnings: Vec<IndexError>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CodeIndex {
    pub last_updated: DateTime<Utc>,
    pub index: BuildCodeIndexOutput,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum RuleBookVisibility {
    #[default]
    Public,
    Private,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuleBook {
    pub id: String,
    pub uri: String,
    pub description: String,
    pub content: String,
    pub visibility: RuleBookVisibility,
    pub tags: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolsCallParams {
    pub name: String,
    pub arguments: Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolsCallResponse {
    pub content: Vec<Content>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct APIKeyScope {
    pub r#type: String,
    pub name: String,
}

impl std::fmt::Display for APIKeyScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.r#type)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GetMyAccountResponse {
    pub username: String,
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub scope: Option<APIKeyScope>,
}

impl GetMyAccountResponse {
    pub fn to_text(&self) -> String {
        format!(
            "ID: {}\nUsername: {}\nName: {} {}\nEmail: {}",
            self.id, self.username, self.first_name, self.last_name, self.email
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListRuleBook {
    pub id: String,
    pub uri: String,
    pub description: String,
    pub visibility: RuleBookVisibility,
    pub tags: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ListRulebooksResponse {
    pub results: Vec<ListRuleBook>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateRuleBookInput {
    pub uri: String,
    pub description: String,
    pub content: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<RuleBookVisibility>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateRuleBookResponse {
    pub id: String,
}

impl ListRuleBook {
    pub fn to_text(&self) -> String {
        format!(
            "URI: {}\nDescription: {}\nTags: {}\n",
            self.uri,
            self.description,
            self.tags.join(", ")
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SimpleLLMMessage {
    #[serde(rename = "role")]
    pub role: SimpleLLMRole,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SimpleLLMRole {
    User,
    Assistant,
}

impl std::fmt::Display for SimpleLLMRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimpleLLMRole::User => write!(f, "user"),
            SimpleLLMRole::Assistant => write!(f, "assistant"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchDocsRequest {
    pub keywords: String,
    pub exclude_keywords: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchMemoryRequest {
    pub keywords: Vec<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SlackReadMessagesRequest {
    pub channel: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SlackReadRepliesRequest {
    pub channel: String,
    pub ts: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SlackSendMessageRequest {
    pub channel: String,
    pub markdown_text: String,
    pub thread_ts: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentState {
    pub agent_model: AgentModel,
    pub messages: Vec<ChatMessage>,
    pub tools: Option<Vec<Tool>>,

    pub llm_input: Option<LLMInput>,
    pub llm_output: Option<LLMOutput>,

    pub metadata: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LLMOutput {
    pub new_message: LLMMessage,
    pub usage: LLMTokenUsage,
}

impl From<&LLMOutput> for ChatMessage {
    fn from(value: &LLMOutput) -> Self {
        let message_content = match &value.new_message.content {
            LLMMessageContent::String(s) => s.clone(),
            LLMMessageContent::List(l) => l
                .iter()
                .map(|c| match c {
                    LLMMessageTypedContent::Text { text } => text.clone(),
                    LLMMessageTypedContent::ToolCall { .. } => String::new(),
                    LLMMessageTypedContent::ToolResult { content, .. } => content.clone(),
                    LLMMessageTypedContent::Image { .. } => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        let tool_calls = if let LLMMessageContent::List(items) = &value.new_message.content {
            let calls: Vec<ToolCall> = items
                .iter()
                .filter_map(|item| {
                    if let LLMMessageTypedContent::ToolCall { id, name, args } = item {
                        Some(ToolCall {
                            id: id.clone(),
                            r#type: "function".to_string(),
                            function: FunctionCall {
                                name: name.clone(),
                                arguments: args.to_string(),
                            },
                        })
                    } else {
                        None
                    }
                })
                .collect();

            if calls.is_empty() { None } else { Some(calls) }
        } else {
            None
        };
        ChatMessage {
            role: Role::Assistant,
            content: Some(MessageContent::String(message_content)),
            name: None,
            tool_calls,
            tool_call_id: None,
            usage: Some(value.usage.clone()),
            ..Default::default()
        }
    }
}

impl AgentState {
    pub fn new(
        agent_model: AgentModel,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<Tool>>,
    ) -> Self {
        Self {
            agent_model,
            messages,
            tools,
            llm_input: None,
            llm_output: None,
            metadata: None,
        }
    }

    pub fn set_messages(&mut self, messages: Vec<ChatMessage>) {
        self.messages = messages;
    }

    pub fn set_tools(&mut self, tools: Option<Vec<Tool>>) {
        self.tools = tools;
    }

    pub fn set_agent_model(&mut self, agent_model: AgentModel) {
        self.agent_model = agent_model;
    }

    pub fn set_llm_input(&mut self, llm_input: Option<LLMInput>) {
        self.llm_input = llm_input;
    }

    pub fn set_llm_output(&mut self, new_message: LLMMessage, new_usage: Option<LLMTokenUsage>) {
        self.llm_output = Some(LLMOutput {
            new_message,
            usage: new_usage.unwrap_or_default(),
        });
    }

    pub fn append_new_message(&mut self, new_message: ChatMessage) {
        self.messages.push(new_message);
    }
}
