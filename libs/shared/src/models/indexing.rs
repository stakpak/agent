use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingStatus {
    pub indexed: bool,
    pub reason: String,
    pub file_count: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
