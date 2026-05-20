use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub rule: String,
    pub file: String,
    pub line: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaEvent {
    pub version: u32,
    pub commit: String,
    pub parent: String,
    pub timestamp: String,
    pub added: Vec<Finding>,
    pub removed: Vec<String>,
}
