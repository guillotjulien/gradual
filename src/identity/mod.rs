pub mod hasher;

use crate::analyzers::types::RawFinding;
use crate::config::PathFilter;
use crate::events::types::Finding;
use hasher::compute_block_id;
use path_slash::PathExt;
use std::path::Path;

/// Converts raw findings into `Finding`s with stable, content-based ids (see
/// `identity::hasher`). Identical findings share an id. The output is sorted by
/// (`file`, `line`, `column`, `rule`, `message`) so it is deterministic even though
/// analyzers emit findings in a nondeterministic order.
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

    pending
        .into_iter()
        .map(|p| Finding {
            id: p.base,
            rule: p.raw.rule.clone(),
            file: p.rel,
            line: p.raw.line,
            message: p.raw.message.clone(),
        })
        .collect()
}
