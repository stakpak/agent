use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{dave_v1, norbert_v1};

#[derive(Serialize, Deserialize, Debug)]
pub struct GetFlowPermission {
    pub read: bool,
    pub write: bool,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Flow {
    pub id: Uuid,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub name: String,
    pub visibility: FlowVisibility,
    pub versions: Vec<FlowVersion>,
}
#[derive(Deserialize, Serialize, Debug, Clone, Copy, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum FlowVisibility {
    #[default]
    Public,
    Private,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FlowVersion {
    pub id: Uuid,
    pub immutable: bool,
    pub created_at: DateTime<Utc>,
    pub tags: Vec<FlowTag>,
    pub parent: Option<FlowVersionRelation>,
    pub children: Vec<FlowVersionRelation>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FlowTag {
    pub name: String,
    pub description: String,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct FlowVersionRelation {
    pub id: Uuid,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct GetFlowDocumentsResponse {
    pub documents: Vec<Document>,
    pub additional_documents: Vec<Document>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Document {
    pub content: String,
    pub uri: String,
}

#[derive(Serialize, Deserialize)]
pub struct QueryBlocksOutput {
    pub results: Vec<QueryBlockResult>,
}
#[derive(Deserialize, Serialize, Debug)]
pub struct QueryBlockResult {
    pub block: Block,
    pub similarity: f64,
    pub flow_version: QueryBlockFlowVersion,
}

#[derive(Deserialize, Serialize, Debug)]
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
impl std::fmt::Display for ProvisionerType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
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

#[derive(Deserialize, Serialize, Debug)]
pub struct QueryBlockFlowVersion {
    pub owner_name: String,
    pub flow_name: String,
    pub version_id: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum FlowRef {
    Version {
        owner_name: String,
        flow_name: String,
        version_id: String,
    },
    Tag {
        owner_name: String,
        flow_name: String,
        tag_name: String,
    },
}

impl std::fmt::Display for FlowRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowRef::Version {
                owner_name,
                flow_name,
                version_id,
            } => write!(f, "{}/{}/{}", owner_name, flow_name, version_id),
            FlowRef::Tag {
                owner_name,
                flow_name,
                tag_name,
            } => write!(f, "{}/{}/{}", owner_name, flow_name, tag_name),
        }
    }
}

impl FlowRef {
    pub fn new(flow_ref: String) -> Result<Self, String> {
        let parts: Vec<&str> = flow_ref.split('/').collect();
        if parts.len() != 3 {
            return Err(
                "Flow ref must be of the format <owner name>/<flow name>/<flow version id or tag>"
                    .into(),
            );
        }
        let owner_name = parts[0].to_string();
        let flow_name = parts[1].to_string();
        let version_ref = parts[2].to_string();

        let flow_version = match Uuid::try_parse(version_ref.as_str()) {
            Ok(version_id) => FlowRef::Version {
                owner_name,
                flow_name,
                version_id: version_id.to_string(),
            },
            Err(_) => FlowRef::Tag {
                owner_name,
                flow_name,
                tag_name: version_ref,
            },
        };
        Ok(flow_version)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentSession {
    pub id: Uuid,
    pub agent_id: AgentID,
    pub visibility: AgentSessionVisibility,
    pub checkpoints: Vec<AgentCheckpointListItem>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub enum AgentID {
    #[default]
    #[serde(rename = "norbert:v1")]
    NorbertV1,
    #[serde(rename = "dave:v1")]
    DaveV1,
}

impl std::str::FromStr for AgentID {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "norbert:v1" => Ok(AgentID::NorbertV1),
            "dave:v1" => Ok(AgentID::DaveV1),
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

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum Action {
    AskUser {
        id: String,
        status: ActionStatus,

        args: AskUserArgs,

        answers: Vec<String>,
    },
    RunCommand {
        id: String,
        status: ActionStatus,

        args: RunCommandArgs,

        exit_code: Option<i32>,
        output: Option<String>,
    },
}

impl Action {
    pub fn get_id(&self) -> &String {
        match self {
            Action::AskUser { id, .. } => id,
            Action::RunCommand { id, .. } => id,
        }
    }
    pub fn get_status(&self) -> &ActionStatus {
        match self {
            Action::AskUser { status, .. } => status,
            Action::RunCommand { status, .. } => status,
        }
    }

    pub fn is_pending(&self) -> bool {
        match self.get_status() {
            ActionStatus::PendingHumanApproval => true,
            ActionStatus::Pending => true,
            ActionStatus::Succeeded => false,
            ActionStatus::Failed => false,
            ActionStatus::Aborted => false,
        }
    }
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionStatus {
    PendingHumanApproval,
    #[default]
    Pending,
    Succeeded,
    Failed,
    Aborted,
}

impl std::fmt::Display for ActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionStatus::PendingHumanApproval => write!(f, "PENDING_HUMAN_APPROVAL"),
            ActionStatus::Pending => write!(f, "PENDING"),
            ActionStatus::Succeeded => write!(f, "SUCCEEDED"),
            ActionStatus::Failed => write!(f, "FAILED"),
            ActionStatus::Aborted => write!(f, "ABORTED"),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
/// Ask the user clarifying questions or more information
pub struct AskUserArgs {
    /// Brief description of why you're asking the user
    pub description: String,
    /// Detailed reasoning for why you need this information
    pub reasoning: String,
    /// List of questions to ask the user
    pub questions: Vec<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
/// Run a shell command and get the output
pub struct RunCommandArgs {
    /// Brief description of why you're asking the user
    pub description: String,
    /// Detailed reasoning for why you need this information
    pub reasoning: String,
    /// The shell command to execute
    pub command: String,
    /// Command to run to undo the changes if needed
    pub rollback_command: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RunAgentInput {
    pub checkpoint_id: Uuid,

    pub input: AgentInput,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RunAgentOutput {
    pub checkpoint: AgentCheckpointListItem,

    pub output: AgentOutput,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "agent_id")]
pub enum AgentInput {
    #[serde(rename = "norbert:v1")]
    NorbertV1 {
        user_prompt: Option<String>,
        action_queue: Option<Vec<Action>>,
        action_history: Option<Vec<Action>>,
        scratchpad: Box<Option<norbert_v1::state::Scratchpad>>,
    },
    #[serde(rename = "dave:v1")]
    DaveV1 {
        user_prompt: Option<String>,
        action_queue: Option<Vec<Action>>,
        action_history: Option<Vec<Action>>,
        scratchpad: Box<Option<dave_v1::state::Scratchpad>>,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "agent_id")]
pub enum AgentOutput {
    #[serde(rename = "norbert:v1")]
    NorbertV1 {
        message: Option<String>,
        action_queue: Vec<Action>,
        action_history: Vec<Action>,
        scratchpad: Box<norbert_v1::state::Scratchpad>,
        user_prompt: String,
    },
    #[serde(rename = "dave:v1")]
    DaveV1 {
        message: Option<String>,
        action_queue: Vec<Action>,
        action_history: Vec<Action>,
        scratchpad: Box<dave_v1::state::Scratchpad>,
        user_prompt: String,
    },
}

impl AgentOutput {
    pub fn get_agent_id(&self) -> AgentID {
        match self {
            AgentOutput::NorbertV1 { .. } => AgentID::NorbertV1,
            AgentOutput::DaveV1 { .. } => AgentID::DaveV1,
        }
    }
}
