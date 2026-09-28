pub mod check;
pub mod init;
pub mod status;
pub mod update;

use crate::analyzers;
use crate::config::GradualConfig;
use crate::events::diff::{KeyDelta, describe_new_with, group_by_id, repeatedly_fixed_ids};
use crate::events::fold::fold;
use crate::events::reader::read_all_events;
use crate::events::types::{Entry, Finding};
use crate::git::{added_line_tiers, find_repo_root};
use crate::identity::build_findings;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct AnalysisResult {
    /// Current findings grouped by id (findings sharing an id are twins).
    pub current: HashMap<String, Vec<Finding>>,
    pub baseline: HashMap<String, Entry>,
    /// Ids lowered by more than one event (see `repeatedly_fixed_ids`).
    pub repeatedly_fixed: HashSet<String>,
    pub repo_root: PathBuf,
    pub events_dir: PathBuf,
}

pub fn analyze(timeout: Option<Duration>) -> anyhow::Result<AnalysisResult> {
    let repo_root = find_repo_root()?;
    let config = GradualConfig::load(&repo_root)?;
    let events_dir = repo_root.join(&config.events_dir);

    // Read the baseline first so an unsupported format fails before slow analyzers run.
    let events = read_all_events(&events_dir)?;
    let baseline = fold(&events);
    let repeatedly_fixed = repeatedly_fixed_ids(&events);

    let raw_findings = analyzers::run_all(&config, &repo_root, timeout)?;
    let filter = config.path_filter()?;

    let current = group_by_id(build_findings(&raw_findings, &repo_root, &filter));

    Ok(AnalysisResult {
        current,
        baseline,
        repeatedly_fixed,
        repo_root,
        events_dir,
    })
}

/// Lines describing the new findings, using git to point at the twin in your diff
/// when a group of identical findings has only some new members.
pub fn describe(repo_root: &Path, deltas: &[KeyDelta]) -> Vec<String> {
    let cache: RefCell<HashMap<String, Vec<HashSet<u32>>>> = RefCell::new(HashMap::new());
    describe_new_with(deltas, &|file| {
        cache
            .borrow_mut()
            .entry(file.to_string())
            .or_insert_with(|| added_line_tiers(repo_root, file))
            .clone()
    })
}
