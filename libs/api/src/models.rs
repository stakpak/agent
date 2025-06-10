use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stakpak_shared::models::integrations::openai::{ChatMessage, MessageContent, Role};
use uuid::Uuid;

use super::{SimpleLLMMessage, SimpleLLMRole, dave_v1, kevin_v1, norbert_v1, stuart_v1};

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
    pub provisioner: ProvisionerType,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SimpleDocument {
    pub uri: String,
    pub content: String,
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub enum TranspileTargetProvisionerType {
    #[serde(rename = "EraserDSL")]
    EraserDSL,
}
impl std::str::FromStr for TranspileTargetProvisionerType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "eraser" => Ok(Self::EraserDSL),
            _ => Ok(Self::EraserDSL),
        }
    }
}
impl std::fmt::Display for TranspileTargetProvisionerType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TranspileTargetProvisionerType::EraserDSL => write!(f, "EraserDSL"),
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

    pub fn to_url(&self) -> String {
        match self {
            FlowRef::Version {
                owner_name,
                flow_name,
                ..
            } => format!("https://stakpak.dev/{}/{}", owner_name, flow_name,),
            FlowRef::Tag {
                owner_name,
                flow_name,
                ..
            } => format!("https://stakpak.dev/{}/{}", owner_name, flow_name),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentSession {
    pub id: Uuid,
    pub title: String,
    pub agent_id: AgentID,
    pub flow_ref: Option<FlowRef>,
    pub visibility: AgentSessionVisibility,
    pub checkpoints: Vec<AgentCheckpointListItem>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone, PartialEq)]
pub enum AgentID {
    #[default]
    #[serde(rename = "norbert:v1")]
    NorbertV1,
    #[serde(rename = "dave:v1")]
    DaveV1,
    #[serde(rename = "dave:v2")]
    DaveV2,
    #[serde(rename = "kevin:v1")]
    KevinV1,
    #[serde(rename = "stuart:v1")]
    StuartV1,
    #[serde(rename = "pablo:v1")]
    PabloV1,
}

impl std::str::FromStr for AgentID {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "norbert:v1" => Ok(AgentID::NorbertV1),
            "dave:v1" => Ok(AgentID::DaveV1),
            "dave:v2" => Ok(AgentID::DaveV2),
            "kevin:v1" => Ok(AgentID::KevinV1),
            "stuart:v1" => Ok(AgentID::StuartV1),
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
    pub flow_ref: Option<FlowRef>,
    pub visibility: AgentSessionVisibility,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AgentSession> for AgentSessionListItem {
    fn from(item: AgentSession) -> Self {
        Self {
            id: item.id,
            agent_id: item.agent_id,
            flow_ref: item.flow_ref,
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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
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
    ReadDocumentCommand {
        id: String,
        status: ActionStatus,

        args: ReadDocumentCommandArgs,

        content: Option<String>,
    },
    GenerateCodeCommand {
        id: String,
        status: ActionStatus,

        args: GenerateCodeCommandArgs,

        result: Box<Option<serde_json::Value>>,
    },
    SearchCodeCommand {
        id: String,
        status: ActionStatus,

        args: SearchCodeCommandArgs,

        results: Box<Option<Vec<SearchCodeResult>>>,
    },
    GetDockerfileTemplate {
        id: String,
        status: ActionStatus,

        args: GetDockerfileTemplateArgs,

        template: Option<String>,
    },
}

impl Action {
    pub fn get_id(&self) -> &String {
        match self {
            Action::AskUser { id, .. } => id,
            Action::RunCommand { id, .. } => id,
            Action::GetDockerfileTemplate { id, .. } => id,
            Action::ReadDocumentCommand { id, .. } => id,
            Action::GenerateCodeCommand { id, .. } => id,
            Action::SearchCodeCommand { id, .. } => id,
        }
    }
    pub fn get_status(&self) -> &ActionStatus {
        match self {
            Action::AskUser { status, .. } => status,
            Action::RunCommand { status, .. } => status,
            Action::GetDockerfileTemplate { status, .. } => status,
            Action::ReadDocumentCommand { status, .. } => status,
            Action::GenerateCodeCommand { status, .. } => status,
            Action::SearchCodeCommand { status, .. } => status,
        }
    }

    pub fn is_pending(&self) -> bool {
        match self.get_status() {
            ActionStatus::PendingHumanReview => true,
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
    PendingHumanReview,
}

impl std::fmt::Display for ActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionStatus::PendingHumanApproval => write!(f, "PENDING_HUMAN_APPROVAL"),
            ActionStatus::Pending => write!(f, "PENDING"),
            ActionStatus::Succeeded => write!(f, "SUCCEEDED"),
            ActionStatus::Failed => write!(f, "FAILED"),
            ActionStatus::Aborted => write!(f, "ABORTED"),
            ActionStatus::PendingHumanReview => write!(f, "PENDING_HUMAN_REVIEW"),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
/// Ask the user clarifying questions or more information
pub struct AskUserArgs {
    /// Brief description of why you're asking the user
    pub description: String,
    /// Detailed reasoning for why you need this information
    pub reasoning: String,
    /// List of questions to ask the user
    pub questions: Vec<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
/// Project name (aka Flow name)
pub struct FlowName {
    /// The owner of the project
    pub owner: String,
    /// The name of the project
    pub name: String,
    /// The version of the project
    pub version: Option<String>,
}

impl std::fmt::Display for FlowName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(version) = &self.version {
            write!(f, "{}/{}/{}", self.owner, self.name, version)
        } else {
            write!(f, "{}/{}", self.owner, self.name)
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
/// Read the contents of a document
pub struct ReadDocumentCommandArgs {
    /// Brief description of why you're reading the document
    pub description: String,
    /// Detailed reasoning for why you need to read this document
    pub reasoning: String,
    pub target: Option<FlowName>,
    /// The uri of the document to read in the format `file:///path/to/document`
    pub document_uri: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
/// Generate or modify code for a document in the codebase
pub struct GenerateCodeCommandArgs {
    /// Brief description of why you're writing to the document
    pub description: String,
    /// Detailed reasoning for why you need to write to this document
    pub reasoning: String,
    pub target: Option<FlowName>,
    /// The uri of the document to write to
    pub document_uri: String,
    /// The prompt for a specialized LLM to make changes to the document
    pub prompt: String,
    pub content_type: ProvisionerType,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
/// Perform exact search to find relevant code blocks in the codebase
pub struct SearchCodeCommandArgs {
    /// Brief description of why you're searching the code
    pub description: String,
    /// Detailed reasoning for why you need to search the code
    pub reasoning: String,
    /// The search query to find relevant code
    pub query: String,
    /// Optional list of projects (aka Flows) to search in
    pub targets: Option<Vec<FlowName>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
/// A search result from code search
pub struct SearchCodeResult {
    /// The uri of the document where the match was found
    pub document_uri: String,
    /// The line number where the match starts
    pub start_row: u32,
    /// The line number where the match ends
    pub end_row: u32,
    /// The matched code snippet
    pub content: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
/// Get a Dockerfile template for a specific technology stack
pub struct GetDockerfileTemplateArgs {
    /// Brief description of why you're requesting the template
    pub description: String,
    /// Detailed reasoning for why you need this template
    pub reasoning: String,
    /// The main language (e.g., "python", "node", "rust", etc.)
    pub programming_language: String,
    /// The main framework (e.g., "flask", "django", "rails", "laravel", etc.)
    pub framework: Option<String>,
    /// Optional runtime version (e.g., "3.9", "18", etc.)
    pub runtime_version: Option<String>,
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
    #[serde(rename = "dave:v2")]
    DaveV2 {
        user_prompt: Option<String>,
        action_queue: Option<Vec<Action>>,
        action_history: Option<Vec<Action>>,
        scratchpad: Box<Option<dave_v1::state::Scratchpad>>,
    },
    #[serde(rename = "kevin:v1")]
    KevinV1 {
        user_prompt: Option<String>,
        action_queue: Option<Vec<Action>>,
        action_history: Option<Vec<Action>>,
        scratchpad: Box<Option<kevin_v1::state::Scratchpad>>,
    },
    #[serde(rename = "stuart:v1")]
    StuartV1 {
        messages: Option<Vec<SimpleLLMMessage>>,
        action_queue: Option<Vec<Action>>,
        action_history: Option<Vec<Action>>,
        scratchpad: Box<Option<stuart_v1::state::Scratchpad>>,
    },
    #[serde(rename = "pablo:v1")]
    PabloV1 {
        messages: Option<Vec<ChatMessage>>,
        node_states: Option<serde_json::Value>,
    },
}

impl AgentInput {
    pub fn new(agent_id: &AgentID) -> Self {
        match agent_id {
            AgentID::NorbertV1 => AgentInput::NorbertV1 {
                user_prompt: None,
                action_queue: None,
                action_history: None,
                scratchpad: Box::new(None),
            },
            AgentID::DaveV1 => AgentInput::DaveV1 {
                user_prompt: None,
                action_queue: None,
                action_history: None,
                scratchpad: Box::new(None),
            },
            AgentID::DaveV2 => AgentInput::DaveV2 {
                user_prompt: None,
                action_queue: None,
                action_history: None,
                scratchpad: Box::new(None),
            },
            AgentID::KevinV1 => AgentInput::KevinV1 {
                user_prompt: None,
                action_queue: None,
                action_history: None,
                scratchpad: Box::new(None),
            },
            AgentID::StuartV1 => AgentInput::StuartV1 {
                messages: None,
                action_queue: None,
                action_history: None,
                scratchpad: Box::new(None),
            },
            AgentID::PabloV1 => AgentInput::PabloV1 {
                messages: None,
                node_states: None,
            },
        }
    }
    pub fn set_user_prompt(&mut self, prompt: Option<String>) {
        match self {
            AgentInput::NorbertV1 { user_prompt, .. }
            | AgentInput::DaveV1 { user_prompt, .. }
            | AgentInput::DaveV2 { user_prompt, .. }
            | AgentInput::KevinV1 { user_prompt, .. } => {
                *user_prompt = prompt;
            }
            AgentInput::StuartV1 { messages, .. } => {
                if let Some(prompt) = prompt {
                    *messages = Some(vec![SimpleLLMMessage {
                        role: SimpleLLMRole::User,
                        content: prompt,
                    }]);
                }
            }
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
            AgentInput::NorbertV1 { .. } => AgentID::NorbertV1,
            AgentInput::DaveV1 { .. } => AgentID::DaveV1,
            AgentInput::DaveV2 { .. } => AgentID::DaveV2,
            AgentInput::KevinV1 { .. } => AgentID::KevinV1,
            AgentInput::StuartV1 { .. } => AgentID::StuartV1,
            AgentInput::PabloV1 { .. } => AgentID::PabloV1,
        }
    }
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
    #[serde(rename = "dave:v2")]
    DaveV2 {
        message: Option<String>,
        action_queue: Vec<Action>,
        action_history: Vec<Action>,
        scratchpad: Box<dave_v1::state::Scratchpad>,
        user_prompt: String,
    },
    #[serde(rename = "kevin:v1")]
    KevinV1 {
        message: Option<String>,
        action_queue: Vec<Action>,
        action_history: Vec<Action>,
        scratchpad: Box<kevin_v1::state::Scratchpad>,
        user_prompt: String,
    },
    #[serde(rename = "stuart:v1")]
    StuartV1 {
        messages: Vec<SimpleLLMMessage>,
        action_queue: Vec<Action>,
        action_history: Vec<Action>,
        scratchpad: Box<stuart_v1::state::Scratchpad>,
    },
    #[serde(rename = "pablo:v1")]
    PabloV1 {
        messages: Vec<ChatMessage>,
        node_states: serde_json::Value,
    },
}

impl AgentOutput {
    pub fn get_agent_id(&self) -> AgentID {
        match self {
            AgentOutput::NorbertV1 { .. } => AgentID::NorbertV1,
            AgentOutput::DaveV1 { .. } => AgentID::DaveV1,
            AgentOutput::DaveV2 { .. } => AgentID::DaveV2,
            AgentOutput::KevinV1 { .. } => AgentID::KevinV1,
            AgentOutput::StuartV1 { .. } => AgentID::StuartV1,
            AgentOutput::PabloV1 { .. } => AgentID::PabloV1,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TranspileInput {
    pub content: Vec<Document>,
    pub output: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TranspileOutput {
    pub result: TranspileResult,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TranspileResult {
    pub blocks: Vec<Block>,
    pub score: i32,
    pub references: Vec<String>,
    pub trace: TranspileTrace,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TranspileTrace {
    pub trace_id: String,
    pub observation_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentPresetInput {
    pub agent_id: AgentID,
    pub provisioner: ProvisionerType,
    pub dir: Option<String>,
    pub flow_ref: Option<FlowRef>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct AgentPresetResult {
    pub input: AgentInput,
    pub name: String,
    pub description: String,
    pub provisioner: Option<ProvisionerType>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentPresetOutput {
    pub results: Vec<AgentPresetResult>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct AgentTask {
    pub input: AgentInput,
    pub name: String,
    pub description: String,
    pub provisioner: Option<ProvisionerType>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentTaskOutput {
    pub results: Vec<AgentTask>,
}
