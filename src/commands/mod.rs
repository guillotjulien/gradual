pub mod check;
pub mod init;
pub mod status;
pub mod update;

use crate::analyzers;
use crate::analyzers::types::RawFinding;
use crate::config::{GradualConfig, PathFilter};
use crate::events::fold::fold;
use crate::events::reader::read_all_events;
use crate::events::types::Finding;
use crate::git::find_repo_root;
use crate::identity::hasher::{assign_counters, compute_block_id};
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

/// Converts raw findings into `Finding`s with stable, content-based ids.
///
/// Each id is a forward-context-block hash (see `identity::hasher`) plus a `:n`
/// occurrence counter. The counter is assigned over a deterministic source order
/// (`file`, `line`, `column`, `rule`, `message`) so the *set* of ids is identical
/// across runs even though analyzers emit findings in a nondeterministic order.
pub fn build_findings(raws: &[RawFinding], repo_root: &Path, filter: &PathFilter) -> Vec<Finding> {
    struct Pending<'a> {
        raw: &'a RawFinding,
        base: String,
        rel: String,
    }

    let mut pending: Vec<Pending> = Vec::new();
    for raw in raws {
        // `compute_block_id` validates the file is under the repo root; compute the
        // relative path first so we can drop out-of-scope findings before hashing.
        let Ok(rel) = raw.file.strip_prefix(repo_root) else {
            continue;
        };
        let rel = rel.to_slash_lossy().to_string();
        if !filter.matches(&rel) {
            continue;
        }

        let base = match compute_block_id(raw, repo_root) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "warning: skipping finding at {}:{}: {:#}",
                    raw.file.display(),
                    raw.line,
                    e
                );
                continue;
            }
        };
        pending.push(Pending { raw, base, rel });
    }

    pending.sort_by(|a, b| {
        (
            a.rel.as_str(),
            a.raw.line,
            a.raw.column,
            a.raw.rule.as_str(),
            a.raw.message.as_str(),
        )
            .cmp(&(
                b.rel.as_str(),
                b.raw.line,
                b.raw.column,
                b.raw.rule.as_str(),
                b.raw.message.as_str(),
            ))
    });

    let base_ids: Vec<String> = pending.iter().map(|p| p.base.clone()).collect();
    let ids = assign_counters(&base_ids);

    pending
        .iter()
        .zip(ids)
        .map(|(p, id)| Finding {
            id,
            rule: p.raw.rule.clone(),
            file: p.rel.clone(),
            line: p.raw.line,
            message: p.raw.message.clone(),
        })
        .collect()
}
