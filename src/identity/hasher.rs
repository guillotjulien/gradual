//! Stable finding identity ("fingerprint") for static-analysis findings.
//!
//! Identity is `hash(rule, file, normalized_message, forward_block)` plus a
//! per-base-hash occurrence counter (`:n`), modeled on github/codeql-action's
//! `fingerprints.ts`:
//!
//!   - **`forward_block`** is the first `BLOCK_SIZE` non-whitespace characters
//!     starting at the finding's line and spilling into following lines. Being
//!     content-relative and whitespace-free, it survives reformatting and code
//!     inserted *above* (or far below) the finding, while the surrounding context
//!     keeps byte-identical lines in different places distinct.
//!   - **`normalized_message`** strips absolute paths so ids are stable per checkout.
//!   - the **`:n` counter** (assigned by `assign_counters` in source order)
//!     guarantees final uniqueness for the rare findings whose whole block matches.
//!
//! Trade-off: because the block includes following context, an edit *within* that
//! window — or moving code so its trailing context changes — will change the id.
//! That is the intended behavior (you touched the finding's surroundings).

use crate::analyzers::types::RawFinding;
use anyhow::Context;
use path_slash::PathExt;
use regex::Regex;
use std::collections::HashMap;
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
// Forward context block (à la codeql-action fingerprints.ts)
// ===========================================================================

/// Number of non-whitespace characters in a forward context block.
pub const BLOCK_SIZE: usize = 100;

/// The first `BLOCK_SIZE` non-whitespace characters starting at `line` (1-indexed)
/// and spilling into following lines. All whitespace is skipped, so reindentation
/// and reformatting do not change it.
pub fn forward_block(source: &str, line: u32) -> String {
    let start = line.saturating_sub(1) as usize;
    let mut out = String::new();
    let mut count = 0usize;
    for l in source.lines().skip(start) {
        for c in l.chars() {
            if c.is_whitespace() {
                continue;
            }
            out.push(c);
            count += 1;
            if count >= BLOCK_SIZE {
                return out;
            }
        }
    }
    out
}

// ===========================================================================
// Identity
// ===========================================================================

fn hash_components(rule: &str, rel_path: &str, message: &str, code: &str) -> String {
    let input = [rule, rel_path, message, code].join("\0");
    format!("{:032x}", xxh3_128(input.as_bytes()))
}

/// Base identity (no occurrence counter) for a finding.
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
    let code = forward_block(&source, finding.line);
    Ok(hash_components(&finding.rule, rel.as_ref(), &message, &code))
}

/// Appends CodeQL-style `:n` occurrence counters to base ids so identical base ids
/// become unique. Counters are assigned in the given order (callers pass findings
/// in a deterministic source order), starting at 0.
pub fn assign_counters(base_ids: &[String]) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    base_ids
        .iter()
        .map(|id| {
            let n = seen.entry(id.as_str()).or_insert(0);
            let out = format!("{id}:{n}");
            *n += 1;
            out
        })
        .collect()
}
