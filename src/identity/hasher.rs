use crate::analyzers::types::RawFinding;
use crate::identity::parser::ParseCache;
use crate::identity::walker::{
    collect_named_scope_path, compute_occurrence_index, find_meaningful_enclosing,
    normalize_whitespace,
};
use anyhow::Context;
use path_slash::PathExt;
use std::fs;
use std::path::Path;
use xxhash_rust::xxh3::xxh3_128;

pub fn compute_finding_id(
    finding: &RawFinding,
    repo_root: &Path,
    cache: &mut ParseCache,
) -> anyhow::Result<String> {
    let source = fs::read(&finding.file)
        .with_context(|| format!("Failed to read {}", finding.file.display()))?;

    let tree = cache.parse(&finding.file)?;
    let root = tree.root_node();

    // tree-sitter is 0-indexed; findings are 1-indexed
    let pos = tree_sitter::Point {
        row: (finding.line.saturating_sub(1)) as usize,
        column: (finding.column.saturating_sub(1)) as usize,
    };

    let token = root
        .descendant_for_point_range(pos, pos)
        .with_context(|| {
            format!(
                "No AST node at {}:{}:{} ",
                finding.file.display(),
                finding.line,
                finding.column
            )
        })?;

    let meaningful = find_meaningful_enclosing(token);
    let scope_path = collect_named_scope_path(meaningful, &source);
    let occurrence = compute_occurrence_index(meaningful, &source);

    let rel_path = finding
        .file
        .strip_prefix(repo_root)
        .with_context(|| {
            format!(
                "{} is not under repo root {}",
                finding.file.display(),
                repo_root.display()
            )
        })?
        .to_slash_lossy();

    let node_text = meaningful.utf8_text(&source).unwrap_or("");

    let components = [
        finding.rule.as_str(),
        rel_path.as_ref(),
        &scope_path.join("."),
        meaningful.kind(),
        &normalize_whitespace(node_text),
        &occurrence.to_string(),
    ];
    let input = components.join("\0");

    let hash = xxh3_128(input.as_bytes());
    Ok(format!("{hash:032x}"))
}
