use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct RawFinding {
    pub rule: String,
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub message: String,
}
