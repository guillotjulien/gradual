use serde::{Deserialize, Serialize};

/// Format version of the event files written by this binary.
pub const EVENT_VERSION: u32 = 2;

/// A finding in the current analysis. Findings that share an `id` (identical lines
/// with the same rule and message in one file) are counted, never numbered.
#[derive(Debug, Clone)]
pub struct Finding {
    pub id: String,
    pub rule: String,
    pub file: String,
    pub line: u32,
    pub message: String,
}

/// A signed change to the number of accepted findings sharing one `id`. Changes are
/// summed by `fold`, so events from parallel branches add up regardless of order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub rule: String,
    pub file: String,
    pub delta: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaEvent {
    pub version: u32,
    pub commit: String,
    pub parent: String,
    pub timestamp: String,
    pub changes: Vec<Change>,
}

/// Baseline entry: how many findings with this id are accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub count: u32,
    pub rule: String,
    pub file: String,
}
