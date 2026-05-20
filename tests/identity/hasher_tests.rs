use gradual::analyzers::types::RawFinding;
use gradual::identity::hasher::compute_finding_id;
use gradual::identity::parser::ParseCache;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/identity/fixtures")
}

fn finding_at(file: PathBuf, line: u32, column: u32) -> RawFinding {
    RawFinding {
        rule: "ts:2304".to_string(),
        file,
        line,
        column,
        message: "Cannot find name 'x'".to_string(),
    }
}

/// Writes fixture content to a stable path inside a tempdir, then computes the finding ID.
/// This ensures the repo-relative path component is identical across all variants.
fn id_for_content(fixture: &str, line: u32, column: u32, tmp_dir: &Path) -> String {
    let src_dir = tmp_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let file_path = src_dir.join("target.ts");

    let content = std::fs::read_to_string(fixtures_dir().join(fixture))
        .unwrap_or_else(|e| panic!("Could not read fixture {fixture}: {e}"));
    std::fs::write(&file_path, content).unwrap();

    let finding = finding_at(file_path, line, column);
    let mut cache = ParseCache::new();
    compute_finding_id(&finding, tmp_dir, &mut cache)
        .unwrap_or_else(|e| panic!("compute_finding_id failed for {fixture}: {e}"))
}

/// Computes the ID directly against the fixture file (file path is part of the ID).
fn id_for_fixture(fixture: &str, line: u32, column: u32) -> String {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = fixtures_dir().join(fixture);
    let finding = finding_at(path, line, column);
    let mut cache = ParseCache::new();
    compute_finding_id(&finding, &repo_root, &mut cache)
        .unwrap_or_else(|e| panic!("compute_finding_id failed for {fixture}: {e}"))
}

// In base.ts:
//   line 2, col 9: `message` in `const message = "Hello, " + name;`
//   Enclosing meaningful node: `variable_declarator` for `message`
//   Scope path: ["greet"]
const BASE_LINE: u32 = 2;
const BASE_COL: u32 = 9;

#[test]
fn add_import_does_not_change_id() {
    // with_import.ts has an extra `import` line at top, shifting greet's body down by 2 lines.
    // Same logical code at a different line → ID must be stable.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, BASE_COL, dir.path());
    // Re-use same temp dir: overwrite the file with the shifted version
    let shifted_id = id_for_content("with_import.ts", 4, BASE_COL, dir.path());
    assert_eq!(base_id, shifted_id, "ID changed after adding an import");
}

#[test]
fn reformat_does_not_change_id() {
    // reformatted.ts has whitespace-only changes; same declarator, normalized text matches.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, BASE_COL, dir.path());
    let reformatted_id = id_for_content("reformatted.ts", BASE_LINE, BASE_COL, dir.path());
    assert_eq!(base_id, reformatted_id, "ID changed after reformatting");
}

#[test]
fn moving_function_does_not_change_id() {
    // function_moved.ts: greet is now at lines 8-11; same declarator at line 9.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, BASE_COL, dir.path());
    let moved_id = id_for_content("function_moved.ts", 9, BASE_COL, dir.path());
    assert_eq!(base_id, moved_id, "ID changed after moving the function");
}

#[test]
fn renaming_variable_changes_id() {
    // variable_renamed.ts: `message` renamed to `greeting` — node text changes.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, BASE_COL, dir.path());
    let renamed_id = id_for_content("variable_renamed.ts", BASE_LINE, BASE_COL, dir.path());
    assert_ne!(base_id, renamed_id, "ID did not change after renaming variable");
}

#[test]
fn editing_expression_changes_id() {
    // expression_edited.ts: `"Hello, " + name` changed to template literal.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, BASE_COL, dir.path());
    let edited_id = id_for_content("expression_edited.ts", BASE_LINE, BASE_COL, dir.path());
    assert_ne!(base_id, edited_id, "ID did not change after editing expression");
}

#[test]
fn different_file_changes_id() {
    // Same code, different file path → different repo-relative path component → different ID.
    let base_id = id_for_fixture("base.ts", BASE_LINE, BASE_COL);
    // function_moved.ts has the same code at different line
    let other_id = id_for_fixture("function_moved.ts", 9, BASE_COL);
    assert_ne!(base_id, other_id, "IDs should differ across files");
}

#[test]
fn duplicated_line_has_distinct_ids() {
    // duplicated_line.ts: lines 2 and 3 both have `const <x> = "Hello, " + name`.
    // Occurrence index must distinguish them.
    let base_id = id_for_fixture("duplicated_line.ts", 2, 9);
    let second_id = id_for_fixture("duplicated_line.ts", 3, 10);
    assert_ne!(base_id, second_id, "Duplicate lines should have distinct IDs");
}

#[test]
fn error_node_does_not_panic() {
    // error_node.ts has broken syntax; must not panic — returns an ID or an error, not a crash.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = fixtures_dir().join("error_node.ts");
    let finding = finding_at(path, 2, 9);
    let mut cache = ParseCache::new();
    let result = compute_finding_id(&finding, &repo_root, &mut cache);
    assert!(result.is_ok(), "Should not panic on files with ERROR nodes: {result:?}");
}

#[test]
fn same_finding_produces_same_id_twice() {
    let id1 = id_for_fixture("base.ts", BASE_LINE, BASE_COL);
    let id2 = id_for_fixture("base.ts", BASE_LINE, BASE_COL);
    assert_eq!(id1, id2, "ID is not deterministic");
}

#[test]
fn id_is_32_hex_chars() {
    let id = id_for_fixture("base.ts", BASE_LINE, BASE_COL);
    assert_eq!(id.len(), 32, "ID should be 32 hex chars (128-bit xxH3): got {id}");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "ID should be lowercase hex: {id}"
    );
}
