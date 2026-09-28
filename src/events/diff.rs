use crate::events::types::{Change, DeltaEvent, Entry, Finding};
use std::collections::{HashMap, HashSet};

/// The difference between the current findings and the baseline for one id.
#[derive(Debug, Clone)]
pub struct KeyDelta {
    pub id: String,
    pub rule: String,
    pub file: String,
    /// Findings currently reported under this id, in source order (may be empty).
    pub current: Vec<Finding>,
    /// `current.len() - baseline count`; never zero.
    pub delta: i64,
}

impl KeyDelta {
    pub fn change(&self) -> Change {
        Change {
            id: self.id.clone(),
            rule: self.rule.clone(),
            file: self.file.clone(),
            delta: self.delta,
        }
    }
}

/// Groups findings by id, preserving the given (source) order within each group.
pub fn group_by_id(findings: Vec<Finding>) -> HashMap<String, Vec<Finding>> {
    let mut groups: HashMap<String, Vec<Finding>> = HashMap::new();
    for f in findings {
        groups.entry(f.id.clone()).or_default().push(f);
    }
    groups
}

/// Every id whose current count differs from its baseline count, sorted by file,
/// first line and id so output is deterministic.
#[allow(clippy::implicit_hasher)]
pub fn diff(
    current: &HashMap<String, Vec<Finding>>,
    baseline: &HashMap<String, Entry>,
) -> Vec<KeyDelta> {
    let mut out: Vec<KeyDelta> = Vec::new();
    for (id, findings) in current {
        let base = baseline.get(id).map_or(0, |e| i64::from(e.count));
        let delta = i64::try_from(findings.len()).unwrap_or(i64::MAX) - base;
        if delta != 0 {
            out.push(KeyDelta {
                id: id.clone(),
                rule: findings[0].rule.clone(),
                file: findings[0].file.clone(),
                current: findings.clone(),
                delta,
            });
        }
    }
    for (id, entry) in baseline {
        if !current.contains_key(id) {
            out.push(KeyDelta {
                id: id.clone(),
                rule: entry.rule.clone(),
                file: entry.file.clone(),
                current: Vec::new(),
                delta: -i64::from(entry.count),
            });
        }
    }
    out.sort_by(|a, b| {
        let line = |k: &KeyDelta| k.current.first().map_or(0, |f| f.line);
        (a.file.as_str(), line(a), a.id.as_str()).cmp(&(b.file.as_str(), line(b), b.id.as_str()))
    });
    out
}

/// Number of new findings (sum of positive deltas).
pub fn new_count(deltas: &[KeyDelta]) -> u64 {
    deltas.iter().filter(|d| d.delta > 0).map(|d| d.delta.unsigned_abs()).sum()
}

/// Number of findings that disappeared (sum of negative deltas).
pub fn removed_count(deltas: &[KeyDelta]) -> u64 {
    deltas.iter().filter(|d| d.delta < 0).map(|d| d.delta.unsigned_abs()).sum()
}

/// Human-readable lines for the new findings.
///
/// Findings sharing an id are indistinguishable, so when only some of a group are
/// new, the whole group is listed on one line: which twin is new cannot be known.
pub fn describe_new(deltas: &[KeyDelta]) -> Vec<String> {
    let mut lines = Vec::new();
    for d in deltas.iter().filter(|d| d.delta > 0) {
        let excess = usize::try_from(d.delta).unwrap_or(usize::MAX);
        if excess >= d.current.len() {
            for f in &d.current {
                lines.push(format!("{}:{}  {}  {}", f.file, f.line, f.rule, f.message));
            }
        } else {
            let first = &d.current[0];
            let at: Vec<String> = d.current.iter().map(|f| f.line.to_string()).collect();
            lines.push(format!(
                "{}  {}  {}  ({excess} new among {} identical findings, lines {})",
                first.file,
                first.rule,
                first.message,
                d.current.len(),
                at.join(", ")
            ));
        }
    }
    lines
}

/// Ids of groups of identical findings (at some point two or more accepted) that were
/// reduced by more than one event. That is the signature of two branches that fixed
/// the same finding: their `-1`s add up to more than the code changed, leaving the
/// baseline too low. Events must be in fold order (sorted by timestamp).
#[allow(clippy::implicit_hasher)]
pub fn repeatedly_fixed_ids(events: &[DeltaEvent]) -> HashSet<String> {
    #[derive(Default)]
    struct Track {
        total: i64,
        peak: i64,
        reductions: usize,
    }
    let mut tracks: HashMap<&str, Track> = HashMap::new();
    for change in events.iter().flat_map(|e| &e.changes) {
        let t = tracks.entry(change.id.as_str()).or_default();
        t.total += change.delta;
        t.peak = t.peak.max(t.total);
        t.reductions += usize::from(change.delta < 0);
    }
    tracks
        .into_iter()
        .filter(|(_, t)| t.peak >= 2 && t.reductions >= 2)
        .map(|(id, _)| id.to_string())
        .collect()
}

/// A hint for the one case where a new finding may not be a regression: a group of
/// identical findings (see `repeatedly_fixed_ids`).
#[allow(clippy::implicit_hasher)]
pub fn concurrent_fix_hint(deltas: &[KeyDelta], repeatedly_fixed: &HashSet<String>) -> Option<String> {
    let suspicious = deltas
        .iter()
        .filter(|d| d.delta > 0 && repeatedly_fixed.contains(&d.id))
        .count();
    (suspicious > 0).then(|| {
        format!(
            "note: {suspicious} group(s) of identical findings above were fixed more than once in the\n\
             baseline history. If two branches fixed the same finding, the baseline is now too low:\n\
             run `gradual update --force` to correct it."
        )
    })
}
