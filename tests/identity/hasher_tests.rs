use gradual::analyzers::types::RawFinding;
use gradual::identity::hasher::{assign_counters, compute_block_id};
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/identity/fixtures")
}

fn finding_at(file: PathBuf, line: u32) -> RawFinding {
    RawFinding {
        rule: "ts:2304".to_string(),
        file,
        line,
        column: 1, // unused by the content-block identity
        message: "Cannot find name 'x'".to_string(),
    }
}

/// Writes fixture content to a stable path inside a tempdir, then computes the base
/// id. Using a fixed path keeps the repo-relative component identical across variants.
fn id_for_content(fixture: &str, line: u32, tmp_dir: &Path) -> String {
    let src_dir = tmp_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let file_path = src_dir.join("target.ts");

    let content = std::fs::read_to_string(fixtures_dir().join(fixture))
        .unwrap_or_else(|e| panic!("Could not read fixture {fixture}: {e}"));
    std::fs::write(&file_path, content).unwrap();

    compute_block_id(&finding_at(file_path, line), tmp_dir)
        .unwrap_or_else(|e| panic!("compute_block_id failed for {fixture}: {e}"))
}

/// Computes the base id directly against the fixture file (path is part of the id).
fn id_for_fixture(fixture: &str, line: u32) -> String {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let finding = finding_at(fixtures_dir().join(fixture), line);
    compute_block_id(&finding, &repo_root)
        .unwrap_or_else(|e| panic!("compute_block_id failed for {fixture}: {e}"))
}

// base.ts line 2: `const message = "Hello, " + name;`
const BASE_LINE: u32 = 2;

#[test]
fn add_import_does_not_change_id() {
    // with_import.ts adds an `import` line at the top, shifting greet down. The
    // forward block is unaffected by code inserted above → id must be stable.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, dir.path());
    let shifted_id = id_for_content("with_import.ts", 4, dir.path());
    assert_eq!(base_id, shifted_id, "ID changed after adding an import above");
}

#[test]
fn renaming_variable_changes_id() {
    // variable_renamed.ts: `message` → `greeting` — the line's content changes.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, dir.path());
    let renamed_id = id_for_content("variable_renamed.ts", BASE_LINE, dir.path());
    assert_ne!(base_id, renamed_id, "ID did not change after renaming variable");
}

#[test]
fn editing_expression_changes_id() {
    // expression_edited.ts: `"Hello, " + name` → template literal.
    let dir = tempfile::tempdir().unwrap();
    let base_id = id_for_content("base.ts", BASE_LINE, dir.path());
    let edited_id = id_for_content("expression_edited.ts", BASE_LINE, dir.path());
    assert_ne!(base_id, edited_id, "ID did not change after editing expression");
}

#[test]
fn different_file_changes_id() {
    // Different file path → different repo-relative path component → different id.
    let base_id = id_for_fixture("base.ts", BASE_LINE);
    let other_id = id_for_fixture("function_moved.ts", 9);
    assert_ne!(base_id, other_id, "IDs should differ across files");
}

#[test]
fn distinct_lines_have_distinct_ids() {
    // duplicated_line.ts lines 2 (`message`) and 3 (`message2`) differ in content
    // and in forward context, so they get distinct base ids without a counter.
    let base_id = id_for_fixture("duplicated_line.ts", 2);
    let second_id = id_for_fixture("duplicated_line.ts", 3);
    assert_ne!(base_id, second_id, "Distinct lines should have distinct IDs");
}

#[test]
fn broken_syntax_does_not_error() {
    // error_node.ts has invalid syntax; the block scheme does not parse, so it must
    // still produce an id without error.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let finding = finding_at(fixtures_dir().join("error_node.ts"), 2);
    let result = compute_block_id(&finding, &repo_root);
    assert!(result.is_ok(), "Should handle files with broken syntax: {result:?}");
}

#[test]
fn same_finding_produces_same_id_twice() {
    let id1 = id_for_fixture("base.ts", BASE_LINE);
    let id2 = id_for_fixture("base.ts", BASE_LINE);
    assert_eq!(id1, id2, "ID is not deterministic");
}

#[test]
fn base_id_is_32_hex_chars() {
    let id = id_for_fixture("base.ts", BASE_LINE);
    assert_eq!(id.len(), 32, "base id should be 32 hex chars (128-bit xxH3): got {id}");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "base id should be lowercase hex: {id}"
    );
}

fn id_for_message(message: &str, tmp_dir: &Path) -> String {
    let src_dir = tmp_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let file_path = src_dir.join("target.ts");
    let content = std::fs::read_to_string(fixtures_dir().join("base.ts")).unwrap();
    std::fs::write(&file_path, content).unwrap();

    let finding = RawFinding {
        rule: "ts:2304".to_string(),
        file: file_path,
        line: BASE_LINE,
        column: 1,
        message: message.to_string(),
    };
    compute_block_id(&finding, tmp_dir).unwrap()
}

#[test]
fn different_message_changes_id() {
    // Same file/line/code — only the diagnostic message differs.
    let dir = tempfile::tempdir().unwrap();
    let id_a = id_for_message("Cannot find name 'x'", dir.path());
    let id_b = id_for_message("Cannot find name 'y'", dir.path());
    assert_ne!(id_a, id_b, "IDs should differ when the message differs");
}

#[test]
fn abs_path_in_message_is_stable() {
    // A message embedding an absolute node_modules path must yield the same id across
    // checkouts at different absolute locations.
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let msg = |root: &Path| {
        format!(
            "Could not find a declaration file for module 'semver'. '{}/node_modules/semver/index.js' implicitly has an 'any' type.",
            root.display()
        )
    };
    let id_a = id_for_message(&msg(dir_a.path()), dir_a.path());
    let id_b = id_for_message(&msg(dir_b.path()), dir_b.path());
    assert_eq!(id_a, id_b, "ID should be stable across checkouts (abs path normalized)");
}

#[test]
fn counter_disambiguates_identical_base_ids() {
    // Findings whose entire block matches (true twins) fall back to the `:n` counter.
    let base = assign_counters(&[
        "aaa".to_string(),
        "aaa".to_string(),
        "bbb".to_string(),
        "aaa".to_string(),
    ]);
    assert_eq!(base, vec!["aaa:0", "aaa:1", "bbb:0", "aaa:2"]);
}
