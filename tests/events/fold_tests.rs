use gradual::events::{
    diff::{KeyDelta, describe_new, diff, group_by_id, new_count, removed_count},
    fold::fold,
    reader::{count_event_files, read_all_events},
    types::{Change, DeltaEvent, EVENT_VERSION, Entry, Finding},
    writer::write_delta_event,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

fn change(id: &str, delta: i64) -> Change {
    Change {
        id: id.to_string(),
        rule: "ts:2304".to_string(),
        file: "src/foo.ts".to_string(),
        delta,
    }
}

fn event(ts: &str, changes: Vec<Change>) -> DeltaEvent {
    DeltaEvent {
        version: EVENT_VERSION,
        commit: "abc1234".to_string(),
        parent: "def5678".to_string(),
        timestamp: ts.to_string(),
        changes,
    }
}

fn finding(id: &str, line: u32) -> Finding {
    Finding {
        id: id.to_string(),
        rule: "ts:2304".to_string(),
        file: "src/foo.ts".to_string(),
        line,
        message: "test finding".to_string(),
    }
}

fn counts(state: &HashMap<String, Entry>) -> Vec<(String, u32)> {
    let mut v: Vec<_> = state.iter().map(|(k, e)| (k.clone(), e.count)).collect();
    v.sort();
    v
}

// --- fold tests ---

#[test]
fn empty_events_gives_empty_state() {
    assert!(fold(&[]).is_empty());
}

#[test]
fn single_add_puts_count_in_state() {
    let state = fold(&[event("2026-01-01T00:00:00Z", vec![change("aaa", 2)])]);
    assert_eq!(counts(&state), [("aaa".to_string(), 2)]);
    assert_eq!(state["aaa"].rule, "ts:2304");
    assert_eq!(state["aaa"].file, "src/foo.ts");
}

#[test]
fn deltas_are_summed() {
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![change("aaa", 3)]),
        event("2026-01-01T01:00:00Z", vec![change("aaa", -1)]),
        event("2026-01-01T02:00:00Z", vec![change("aaa", 2), change("bbb", 1)]),
    ]);
    assert_eq!(counts(&state), [("aaa".to_string(), 4), ("bbb".to_string(), 1)]);
}

#[test]
fn add_then_remove_gives_empty_state() {
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![change("aaa", 2)]),
        event("2026-01-01T01:00:00Z", vec![change("aaa", -2)]),
    ]);
    assert!(state.is_empty());
}

#[test]
fn removing_unknown_id_does_not_panic_or_create_credit() {
    let state = fold(&[event("2026-01-01T00:00:00Z", vec![change("nonexistent", -1)])]);
    assert!(state.is_empty());
}

#[test]
fn total_below_zero_is_clamped_not_a_credit() {
    // Two branches both fixed the same single finding: -1 + -1 sums to -2. The final
    // total is clamped to "absent"; it is never a negative count.
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![change("aaa", 1)]),
        event("2026-01-01T01:00:00Z", vec![change("aaa", -1)]),
        event("2026-01-01T02:00:00Z", vec![change("aaa", -1)]),
    ]);
    assert!(state.is_empty());
}

#[test]
fn fold_does_not_depend_on_event_order() {
    let events = [
        event("2026-01-01T00:00:00Z", vec![change("aaa", 2), change("bbb", 1)]),
        event("2026-01-01T01:00:00Z", vec![change("aaa", -1)]),
        event("2026-01-01T02:00:00Z", vec![change("aaa", 3), change("bbb", -1)]),
    ];
    let expected = counts(&fold(&events));
    for perm in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let shuffled: Vec<DeltaEvent> = perm.iter().map(|&i| events[i].clone()).collect();
        assert_eq!(counts(&fold(&shuffled)), expected, "order {perm:?}");
    }
}

// Deleted files: when a source file is deleted, its findings no longer appear in
// analyzer output. The diff step turns that into negative deltas, and the fold drops
// the entries.
#[test]
fn deleted_file_findings_are_removed_from_baseline() {
    let genesis = event("2026-01-01T00:00:00Z", vec![change("gone_file_id", 2)]);
    let after_deletion = event("2026-01-01T01:00:00Z", vec![change("gone_file_id", -2)]);
    let state = fold(&[genesis, after_deletion]);
    assert!(state.is_empty(), "finding from deleted file must not appear in baseline");
}

// --- diff tests ---

fn baseline(entries: &[(&str, u32)]) -> HashMap<String, Entry> {
    entries
        .iter()
        .map(|(id, count)| {
            (
                (*id).to_string(),
                Entry { count: *count, rule: "ts:2304".into(), file: "src/foo.ts".into() },
            )
        })
        .collect()
}

#[test]
fn diff_is_empty_when_counts_match() {
    let current = group_by_id(vec![finding("aaa", 1), finding("aaa", 5), finding("bbb", 9)]);
    assert!(diff(&current, &baseline(&[("aaa", 2), ("bbb", 1)])).is_empty());
}

#[test]
fn diff_reports_excess_and_missing() {
    let current = group_by_id(vec![finding("aaa", 1), finding("aaa", 5), finding("aaa", 9)]);
    let deltas = diff(&current, &baseline(&[("aaa", 2), ("gone", 3)]));
    assert_eq!(new_count(&deltas), 1);
    assert_eq!(removed_count(&deltas), 3);
    let changes: Vec<(String, i64)> =
        deltas.iter().map(KeyDelta::change).map(|c| (c.id, c.delta)).collect();
    assert!(changes.contains(&("aaa".to_string(), 1)));
    assert!(changes.contains(&("gone".to_string(), -3)));
}

#[test]
fn describe_lists_a_single_new_finding_by_line() {
    let current = group_by_id(vec![finding("aaa", 7)]);
    let lines = describe_new(&diff(&current, &baseline(&[])));
    assert_eq!(lines, ["src/foo.ts:7  ts:2304  test finding"]);
}

#[test]
fn describe_lists_whole_group_when_only_some_are_new() {
    let current = group_by_id(vec![finding("aaa", 3), finding("aaa", 9), finding("aaa", 14)]);
    let lines = describe_new(&diff(&current, &baseline(&[("aaa", 2)])));
    assert_eq!(
        lines,
        ["src/foo.ts  ts:2304  test finding  (1 new among 3 identical findings, lines 3, 9, 14)"]
    );
}

#[test]
fn describe_lists_every_finding_when_the_whole_group_is_new() {
    let current = group_by_id(vec![finding("aaa", 3), finding("aaa", 9)]);
    let lines = describe_new(&diff(&current, &baseline(&[])));
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("src/foo.ts:3"));
    assert!(lines[1].starts_with("src/foo.ts:9"));
}

// --- reader tests ---

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/events/fixtures")
}

fn dir_with(fixtures: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    for (fixture, name) in fixtures {
        std::fs::copy(fixtures_dir().join(fixture), tmp.path().join(name)).unwrap();
    }
    tmp
}

#[test]
fn reader_loads_valid_event_from_fixture() {
    let tmp = dir_with(&[("single_add.json", "single_add.json")]);
    let events = read_all_events(tmp.path()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].changes.len(), 1);
    assert_eq!(events[0].changes[0].id, "aaaabbbbccccddddeeeeffffgggghhhh");
    assert_eq!(events[0].changes[0].delta, 2);
}

#[test]
fn reader_rejects_unknown_version() {
    let tmp = dir_with(&[("unknown_version.json", "unknown_version.json")]);
    let err = read_all_events(tmp.path()).unwrap_err().to_string();
    assert!(err.contains("unsupported baseline format version 99"), "{err}");
}

#[test]
fn reader_rejects_v1_baseline_with_migration_message() {
    let tmp = dir_with(&[("legacy_v1.json", "legacy_v1.json")]);
    let err = read_all_events(tmp.path()).unwrap_err().to_string();
    assert!(err.contains("unsupported baseline format version 1"), "{err}");
    assert!(err.contains("gradual init"), "message must say how to migrate: {err}");
}

#[test]
fn reader_skips_malformed_json() {
    let tmp = dir_with(&[("single_add.json", "a.json")]);
    std::fs::write(tmp.path().join("broken.json"), "{ not json").unwrap();
    assert_eq!(read_all_events(tmp.path()).unwrap().len(), 1);
}

#[test]
fn reader_sorts_by_timestamp() {
    // single_add is 10:00, subsequent_remove is 11:00
    let tmp = dir_with(&[("subsequent_remove.json", "b.json"), ("single_add.json", "a.json")]);
    let events = read_all_events(tmp.path()).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].timestamp, "2026-01-15T10:00:00Z");
    assert_eq!(events[1].timestamp, "2026-01-15T11:00:00Z");
    assert_eq!(counts(&fold(&events)), [("aaaabbbbccccddddeeeeffffgggghhhh".to_string(), 1)]);
}

#[test]
fn reader_returns_empty_for_missing_dir() {
    let events = read_all_events(std::path::Path::new("/tmp/gradual_nonexistent_dir_xyz")).unwrap();
    assert!(events.is_empty());
}

#[test]
fn count_event_files_counts_files_of_any_version() {
    let tmp = dir_with(&[("legacy_v1.json", "v1.json"), ("single_add.json", "v2.json")]);
    assert_eq!(count_event_files(tmp.path()), 2);
    assert_eq!(count_event_files(std::path::Path::new("/tmp/gradual_nonexistent_dir_xyz")), 0);
}

// --- writer tests ---

#[test]
fn writer_creates_sharded_path() {
    let tmp = TempDir::new().unwrap();
    let evt = event("2026-03-15T09:05:30Z", vec![]);
    let evt = DeltaEvent { commit: "abcdef1234567".to_string(), ..evt };
    let path = write_delta_event(tmp.path(), &evt).unwrap();
    // Should be at <tmp>/2026/03/15/09-05-30-abcdef1.json
    assert!(path.exists());
    assert_eq!(path.file_name().unwrap(), "09-05-30-abcdef1.json");
    assert!(path.to_string_lossy().contains("2026/03/15"));
}

#[test]
fn writer_output_is_valid_json_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let evt = DeltaEvent {
        commit: "abcdef1234567".to_string(),
        ..event("2026-03-15T09:05:30Z", vec![change("zzz", 2), change("old_id", -1)])
    };
    let path = write_delta_event(tmp.path(), &evt).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: DeltaEvent = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed.version, EVENT_VERSION);
    assert_eq!(parsed.commit, "abcdef1234567");
    assert_eq!(parsed.changes.len(), 2);
    assert_eq!((parsed.changes[0].id.as_str(), parsed.changes[0].delta), ("zzz", 2));
    assert_eq!((parsed.changes[1].id.as_str(), parsed.changes[1].delta), ("old_id", -1));
}

#[test]
fn writer_and_reader_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let evt = event("2026-03-15T09:05:30Z", vec![change("roundtrip_id", 1)]);
    write_delta_event(tmp.path(), &evt).unwrap();
    let events = read_all_events(tmp.path()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].changes[0].id, "roundtrip_id");
}
