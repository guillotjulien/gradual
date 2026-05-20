pub mod check;
pub mod init;
pub mod install_hook;
pub mod status;
pub mod update;

use crate::analyzers;
use crate::analyzers::types::RawFinding;
use crate::config::GradualConfig;
use crate::events::fold::fold;
use crate::events::reader::read_all_events;
use crate::events::types::Finding;
use crate::git::find_repo_root;
use crate::identity::hasher::compute_finding_id;
use crate::identity::parser::ParseCache;
use path_slash::PathExt;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

    let mut cache = ParseCache::new();
    let mut current: HashMap<String, Finding> = HashMap::new();
    for raw in &raw_findings {
        match raw_to_finding(raw, &repo_root, &mut cache) {
            Ok(f) => {
                current.insert(f.id.clone(), f);
            }
            Err(e) => eprintln!(
                "warning: skipping finding at {}:{}: {:#}",
                raw.file.display(),
                raw.line,
                e
            ),
        }
    }

    let events = read_all_events(&events_dir);
    let baseline = fold(&events);

    Ok(AnalysisResult {
        current,
        baseline,
        repo_root,
        events_dir,
    })
}

pub fn raw_to_finding(
    raw: &RawFinding,
    repo_root: &Path,
    cache: &mut ParseCache,
) -> anyhow::Result<Finding> {
    let id = compute_finding_id(raw, repo_root, cache)?;
    let rel_path = raw
        .file
        .strip_prefix(repo_root)?
        .to_slash_lossy()
        .to_string();
    Ok(Finding {
        id,
        rule: raw.rule.clone(),
        file: rel_path,
        line: raw.line,
        message: raw.message.clone(),
    })
}
