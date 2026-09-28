pub mod check;
pub mod init;
pub mod status;
pub mod update;

use crate::analyzers;
use crate::config::GradualConfig;
use crate::events::fold::fold;
use crate::events::reader::read_all_events;
use crate::events::types::Finding;
use crate::git::find_repo_root;
use crate::identity::build_findings;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub struct AnalysisResult {
    pub current: HashMap<String, Finding>,
    pub baseline: HashMap<String, Finding>,
    pub repo_root: PathBuf,
    pub events_dir: PathBuf,
}

pub fn analyze(timeout: Option<Duration>) -> anyhow::Result<AnalysisResult> {
    let repo_root = find_repo_root()?;
    let config = GradualConfig::load(&repo_root)?;
    let events_dir = repo_root.join(&config.events_dir);

    let raw_findings = analyzers::run_all(&config, &repo_root, timeout)?;
    let filter = config.path_filter()?;

    let current: HashMap<String, Finding> = build_findings(&raw_findings, &repo_root, &filter)
        .into_iter()
        .map(|f| (f.id.clone(), f))
        .collect();

    let events = read_all_events(&events_dir);
    let baseline = fold(&events);

    Ok(AnalysisResult {
        current,
        baseline,
        repo_root,
        events_dir,
    })
}
