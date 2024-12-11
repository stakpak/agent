use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
