use gradual::events::{
    fold::fold,
    reader::read_all_events,
    types::{DeltaEvent, Finding},
    writer::write_delta_event,
};
use std::path::PathBuf;
use tempfile::TempDir;

fn finding(id: &str) -> Finding {
    Finding {
        id: id.to_string(),
        rule: "ts:2304".to_string(),
        file: "src/foo.ts".to_string(),
        line: 1,
        message: "test finding".to_string(),
    }
}

fn event(ts: &str, added: Vec<Finding>, removed: Vec<&str>) -> DeltaEvent {
    DeltaEvent {
        version: 1,
        commit: "abc1234".to_string(),
        parent: "def5678".to_string(),
        timestamp: ts.to_string(),
        added,
        removed: removed.into_iter().map(String::from).collect(),
    }
}

// --- fold tests ---

#[test]
fn empty_events_gives_empty_state() {
    assert!(fold(&[]).is_empty());
}

#[test]
fn single_add_puts_finding_in_state() {
    let state = fold(&[event("2026-01-01T00:00:00Z", vec![finding("aaa")], vec![])]);
    assert_eq!(state.len(), 1);
    assert_eq!(state["aaa"].id, "aaa");
}

#[test]
fn add_then_remove_gives_empty_state() {
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![finding("aaa")], vec![]),
        event("2026-01-01T01:00:00Z", vec![], vec!["aaa"]),
    ]);
    assert!(state.is_empty());
}

#[test]
fn two_adds_of_same_id_are_idempotent() {
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![finding("aaa")], vec![]),
        event("2026-01-01T01:00:00Z", vec![finding("aaa")], vec![]),
    ]);
    assert_eq!(state.len(), 1);
}

#[test]
fn two_removes_of_same_id_do_not_panic() {
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![], vec!["aaa"]),
        event("2026-01-01T01:00:00Z", vec![], vec!["aaa"]),
    ]);
    assert!(state.is_empty());
}

#[test]
fn remove_nonexistent_id_does_not_panic() {
    let state = fold(&[event("2026-01-01T00:00:00Z", vec![], vec!["nonexistent"])]);
    assert!(state.is_empty());
}

#[test]
fn rebase_add_twice_is_present_once() {
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![finding("aaa")], vec![]),
        event("2026-01-01T01:00:00Z", vec![finding("aaa")], vec![]),
    ]);
    assert_eq!(state.len(), 1);
    assert!(state.contains_key("aaa"));
}

#[test]
fn rebase_add_then_remove_is_absent() {
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![finding("aaa")], vec![]),
        event("2026-01-01T01:00:00Z", vec![], vec!["aaa"]),
    ]);
    assert!(!state.contains_key("aaa"));
}

#[test]
fn mixed_add_remove_scenario() {
    // add f and g in event 1, remove f in event 2, add h in event 3 → {g, h}
    let state = fold(&[
        event("2026-01-01T00:00:00Z", vec![finding("f"), finding("g")], vec![]),
        event("2026-01-01T01:00:00Z", vec![], vec!["f"]),
        event("2026-01-01T02:00:00Z", vec![finding("h")], vec![]),
    ]);
    assert_eq!(state.len(), 2);
    assert!(!state.contains_key("f"));
    assert!(state.contains_key("g"));
    assert!(state.contains_key("h"));
}

// Deleted files: when a source file is deleted, its findings no longer appear
// in analyzer output. The diff step puts those IDs into `removed`. The fold
// then drops them from the baseline. This test verifies that invariant at the
// fold layer, which is the only layer that needs to be correct for this property.
#[test]
fn deleted_file_findings_are_removed_from_baseline() {
    // Genesis event records a finding from "src/gone.ts"
    let genesis = event("2026-01-01T00:00:00Z", vec![finding("gone_file_id")], vec![]);
    // After the file is deleted, the diff produces a remove-only event
    let after_deletion = event("2026-01-01T01:00:00Z", vec![], vec!["gone_file_id"]);

    let state = fold(&[genesis, after_deletion]);
    assert!(
        !state.contains_key("gone_file_id"),
        "finding from deleted file must not appear in baseline"
    );
    assert!(state.is_empty());
}

// --- reader tests ---

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/events/fixtures")
}

#[test]
fn reader_loads_valid_event_from_fixture() {
    let dir = fixtures_dir();
    // single_add.json only (subsequent_remove.json would cause an extra event)
    // We read a temp dir with only the single_add fixture copied in
    let tmp = TempDir::new().unwrap();
    std::fs::copy(dir.join("single_add.json"), tmp.path().join("single_add.json")).unwrap();

    let events = read_all_events(tmp.path());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].added.len(), 1);
    assert_eq!(events[0].removed.len(), 0);
    assert_eq!(events[0].added[0].id, "aaaabbbbccccddddeeeeffffgggghhhh");
}

#[test]
fn reader_skips_unknown_version() {
    let dir = fixtures_dir();
    let tmp = TempDir::new().unwrap();
    std::fs::copy(dir.join("unknown_version.json"), tmp.path().join("unknown_version.json"))
        .unwrap();

    let events = read_all_events(tmp.path());
    assert!(events.is_empty());
}

#[test]
fn reader_sorts_by_timestamp() {
    let dir = fixtures_dir();
    let tmp = TempDir::new().unwrap();
    // Copy both; single_add is 10:00, subsequent_remove is 11:00
    std::fs::copy(dir.join("subsequent_remove.json"), tmp.path().join("b.json")).unwrap();
    std::fs::copy(dir.join("single_add.json"), tmp.path().join("a.json")).unwrap();

    let events = read_all_events(tmp.path());
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].timestamp, "2026-01-15T10:00:00Z");
    assert_eq!(events[1].timestamp, "2026-01-15T11:00:00Z");
}

#[test]
fn reader_returns_empty_for_missing_dir() {
    let events = read_all_events(std::path::Path::new("/tmp/gradual_nonexistent_dir_xyz"));
    assert!(events.is_empty());
}

// --- writer tests ---

#[test]
fn writer_creates_sharded_path() {
    let tmp = TempDir::new().unwrap();
    let evt = DeltaEvent {
        version: 1,
        commit: "abcdef1234567".to_string(),
        parent: "0000000".to_string(),
        timestamp: "2026-03-15T09:05:30Z".to_string(),
        added: vec![],
        removed: vec![],
    };
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
        version: 1,
        commit: "abcdef1234567".to_string(),
        parent: "0000000".to_string(),
        timestamp: "2026-03-15T09:05:30Z".to_string(),
        added: vec![finding("zzz")],
        removed: vec!["old_id".to_string()],
    };
    let path = write_delta_event(tmp.path(), &evt).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: DeltaEvent = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed.commit, "abcdef1234567");
    assert_eq!(parsed.added.len(), 1);
    assert_eq!(parsed.added[0].id, "zzz");
    assert_eq!(parsed.removed, vec!["old_id"]);
}

#[test]
fn writer_and_reader_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let evt = DeltaEvent {
        version: 1,
        commit: "abcdef1234567".to_string(),
        parent: "0000000".to_string(),
        timestamp: "2026-03-15T09:05:30Z".to_string(),
        added: vec![finding("roundtrip_id")],
        removed: vec![],
    };
    write_delta_event(tmp.path(), &evt).unwrap();
    let events = read_all_events(tmp.path());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].added[0].id, "roundtrip_id");
}
