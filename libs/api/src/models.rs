use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::openai::{ChatMessage, MessageContent, Role};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum RecoveryMode {
    #[serde(rename = "REDIRECTION")]
    Redirection,
    #[serde(rename = "REVERT")]
    Revert,
    #[serde(rename = "MODELCHANGE")]
    ModelChange,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecoveryOption {
    pub id: Uuid,
    pub mode: RecoveryMode,
    pub state_edits: serde_json::Value,
    pub reasoning: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirection_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert_to_checkpoint: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt_key: Option<String>,
}

// Helper struct for source_checkpoint which comes as {"id": "uuid"}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceCheckpoint {
    pub id: Uuid,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub id: Uuid,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecoveryOptionsResponse {
    #[serde(default)]
    pub recovery_options: Vec<RecoveryOption>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub source_checkpoint: Option<SourceCheckpoint>,
    #[serde(default)]
    pub session: Option<Session>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecoveryActionType {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecoveryActionRequest {
    pub action: RecoveryActionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<Uuid>,
}

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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentSession {
    pub id: Uuid,
    pub title: String,
    pub agent_id: AgentID,
    pub visibility: AgentSessionVisibility,
    pub checkpoints: Vec<AgentCheckpointListItem>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone, PartialEq)]
pub enum AgentID {
    #[default]
    #[serde(rename = "pablo:v1")]
    PabloV1,
}

impl std::str::FromStr for AgentID {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pablo:v1" => Ok(AgentID::PabloV1),
            _ => Err(format!("Invalid agent ID: {}", s)),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum AgentSessionVisibility {
    #[serde(rename = "PRIVATE")]
    Private,
    #[serde(rename = "PUBLIC")]
    Public,
}

impl std::fmt::Display for AgentSessionVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentSessionVisibility::Private => write!(f, "PRIVATE"),
            AgentSessionVisibility::Public => write!(f, "PUBLIC"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentCheckpointListItem {
    pub id: Uuid,
    pub status: AgentStatus,
    pub execution_depth: usize,
    pub parent: Option<AgentParentCheckpoint>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentSessionListItem {
    pub id: Uuid,
    pub agent_id: AgentID,
    pub visibility: AgentSessionVisibility,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AgentSession> for AgentSessionListItem {
    fn from(item: AgentSession) -> Self {
        Self {
            id: item.id,
            agent_id: item.agent_id,
            visibility: item.visibility,
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentParentCheckpoint {
    pub id: Uuid,
}
#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum AgentStatus {
    #[serde(rename = "RUNNING")]
    Running,
    #[serde(rename = "COMPLETE")]
    Complete,
    #[serde(rename = "BLOCKED")]
    Blocked,
    #[serde(rename = "FAILED")]
    Failed,
}
impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Running => write!(f, "RUNNING"),
            AgentStatus::Complete => write!(f, "COMPLETE"),
            AgentStatus::Blocked => write!(f, "BLOCKED"),
            AgentStatus::Failed => write!(f, "FAILED"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RunAgentInput {
    pub checkpoint_id: Uuid,
    pub input: AgentInput,
}

impl PartialEq for RunAgentInput {
    fn eq(&self, other: &Self) -> bool {
        self.input == other.input
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RunAgentOutput {
    pub checkpoint: AgentCheckpointListItem,
    pub session: AgentSessionListItem,
    pub output: AgentOutput,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(tag = "agent_id")]
pub enum AgentInput {
    #[serde(rename = "pablo:v1")]
    PabloV1 {
        messages: Option<Vec<ChatMessage>>,
        node_states: Option<serde_json::Value>,
    },
}

impl AgentInput {
    pub fn new(agent_id: &AgentID) -> Self {
        match agent_id {
            AgentID::PabloV1 => AgentInput::PabloV1 {
                messages: None,
                node_states: None,
            },
        }
    }
    pub fn set_user_prompt(&mut self, prompt: Option<String>) {
        match self {
            AgentInput::PabloV1 { messages, .. } => {
                if let Some(prompt) = prompt {
                    *messages = Some(vec![ChatMessage {
                        role: Role::User,
                        content: Some(MessageContent::String(prompt)),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    }]);
                }
            }
        }
    }
    pub fn get_agent_id(&self) -> AgentID {
        match self {
            AgentInput::PabloV1 { .. } => AgentID::PabloV1,
        }
    }
}
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "agent_id")]
pub enum AgentOutput {
    #[serde(rename = "pablo:v1")]
    PabloV1 {
        messages: Vec<ChatMessage>,
        node_states: serde_json::Value,
    },
}

impl AgentOutput {
    pub fn get_agent_id(&self) -> AgentID {
        match self {
            AgentOutput::PabloV1 { .. } => AgentID::PabloV1,
        }
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentSessionStats {
    pub aborted_tool_calls: u32,
    pub analysis_period: Option<String>,
    pub failed_tool_calls: u32,
    pub from_date: Option<String>,
    pub sessions_with_activity: u32,
    pub successful_tool_calls: u32,
    pub to_date: Option<String>,
    pub tools_usage: Vec<ToolUsageStats>,
    pub total_sessions: u32,
    pub total_time_saved_seconds: Option<u32>,
    pub total_tool_calls: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolUsageStats {
    pub display_name: String,
    pub time_saved_per_call: Option<f64>,
    pub time_saved_seconds: Option<u32>,
    pub tool_name: String,
    pub usage_counts: ToolUsageCounts,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolUsageCounts {
    pub aborted: u32,
    pub failed: u32,
    pub successful: u32,
    pub total: u32,
}
