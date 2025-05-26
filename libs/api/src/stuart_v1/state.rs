use serde::{Deserialize, Serialize};

// #[schema(required)] is required on every field by openai
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scratchpad {
    // Configurations source
    /// Layout and organization of the files in the project
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_structure: Option<String>,
}
