//! Stable finding identity ("fingerprint") for static-analysis findings.
//!
//! Identity is `hash(rule, file, normalized_message, line_block)`:
//!
//!   - **`line_block`** is the whitespace-normalized text of the finding's own line.
//!     Being content-relative, it survives code inserted or removed anywhere else
//!     in the file, edits to neighbouring lines, re-indentation and CRLF/LF changes.
//!   - **`normalized_message`** strips absolute paths so ids are stable per checkout.
//!
//! Findings that share an id (identical lines with the same rule and message in one
//! file, "twins") are not numbered: the baseline stores how many are accepted, and
//! `check` fails when the current count is higher. Counting, unlike positional
//! numbering, gives the same answer no matter how parallel branches interleave.
//!
//! Trade-off: only the finding's own line is hashed, so editing that line, renaming
//! the file, or changing the diagnostic changes the id.

use crate::analyzers::types::RawFinding;
use anyhow::Context;
use path_slash::PathExt;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use xxhash_rust::xxh3::xxh3_128;

// ===========================================================================
// Message normalization
// ===========================================================================

/// Matches an absolute path prefix ending at a `node_modules/` segment, so any
/// external-dependency path in a diagnostic message collapses to a location that
/// is stable regardless of where the repo is checked out.
static NODE_MODULES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\s'\x22]*/node_modules/").unwrap());

/// Canonicalizes a diagnostic message for hashing so the id is stable across
/// checkouts and reformatting of the message text:
///   1. strip the absolute repo-root prefix (in-repo paths become relative),
///   2. collapse any `.../node_modules/` prefix to `node_modules/`,
///   3. collapse runs of whitespace to single spaces.
pub fn normalize_message(message: &str, repo_root: &Path) -> String {
    let root = repo_root.to_slash_lossy();
    let stripped = if root.is_empty() {
        message.to_string()
    } else {
        message.replace(root.as_ref(), "")
    };
    let no_nm = NODE_MODULES_RE.replace_all(&stripped, "node_modules/");
    no_nm.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ===========================================================================
// Line block
// ===========================================================================

/// Extracts only the normalized text of the finding's specific line.
/// By ignoring neighbouring lines, edits elsewhere in the file (like adding a new
/// property) will never alter this finding's identity.
pub fn line_block(source: &str, line: u32) -> String {
    let start = line.saturating_sub(1) as usize;

    source
        .lines()
        .nth(start)
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ===========================================================================
// Identity
// ===========================================================================

fn hash_components(rule: &str, rel_path: &str, message: &str, code: &str) -> String {
    let input = [rule, rel_path, message, code].join("\0");
    format!("{:032x}", xxh3_128(input.as_bytes()))
}

/// Identity of a finding.
pub fn compute_block_id(finding: &RawFinding, repo_root: &Path) -> anyhow::Result<String> {
    let source = fs::read_to_string(&finding.file)
        .with_context(|| format!("Failed to read {}", finding.file.display()))?;
    let rel = finding
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
    let message = normalize_message(&finding.message, repo_root);
    let code = line_block(&source, finding.line);
    Ok(hash_components(&finding.rule, rel.as_ref(), &message, &code))
}
