use gradual::events::diff::{KeyDelta, describe_new, describe_new_with, group_by_id, locate_new};
use gradual::events::types::Finding;
use gradual::git::{added_line_tiers, added_lines, default_branch_base, parse_added_lines};
use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

// --- parse_added_lines ---

#[test]
fn parses_single_line_hunk() {
    assert_eq!(parse_added_lines("@@ -3 +5 @@ fn x\n+added\n"), [5]);
}

#[test]
fn parses_ranges_and_multiple_hunks() {
    let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,0 +2,3 @@\n+a\n+b\n+c\n@@ -9 +12,2 @@\n+d\n+e\n";
    assert_eq!(parse_added_lines(diff), [2, 3, 4, 12, 13]);
}

#[test]
fn pure_deletion_adds_nothing() {
    assert!(parse_added_lines("@@ -4,2 +3,0 @@\n-x\n-y\n").is_empty());
}

#[test]
fn empty_or_garbage_input_adds_nothing() {
    assert!(parse_added_lines("").is_empty());
    assert!(parse_added_lines("@@ nonsense @@\n").is_empty());
}

// --- locate_new / describe_new_with (pure) ---

fn twin(line: u32) -> Finding {
    Finding {
        id: "aaa".into(),
        rule: "ts:2304".into(),
        file: "src/app.ts".into(),
        line,
        message: "Cannot find name 'x'".into(),
    }
}

/// A group of `lines` identical findings of which `excess` are new.
fn group(lines: &[u32], excess: i64) -> KeyDelta {
    let current = group_by_id(lines.iter().map(|l| twin(*l)).collect());
    KeyDelta {
        id: "aaa".into(),
        rule: "ts:2304".into(),
        file: "src/app.ts".into(),
        current: current["aaa"].clone(),
        delta: excess,
    }
}

fn set(lines: &[u32]) -> HashSet<u32> {
    lines.iter().copied().collect()
}

#[test]
fn locate_uses_first_conclusive_tier() {
    let g = group(&[2, 6, 10], 1);
    assert_eq!(locate_new(&g, &[set(&[6, 40])]), Some(vec![6]));
    // Tier 1 marks two twins (too many), tier 2 is exact.
    assert_eq!(locate_new(&g, &[set(&[2, 6]), set(&[10])]), Some(vec![10]));
}

#[test]
fn locate_is_inconclusive_without_an_exact_match() {
    let g = group(&[2, 6, 10], 1);
    assert_eq!(locate_new(&g, &[]), None);
    assert_eq!(locate_new(&g, &[set(&[])]), None);
    assert_eq!(locate_new(&g, &[set(&[2, 6, 10])]), None);
}

#[test]
fn describe_points_at_the_twin_in_the_diff() {
    let g = group(&[2, 6, 10], 1);
    let lines = describe_new_with(&[g], &|_| vec![set(&[6])]);
    assert_eq!(
        lines,
        ["src/app.ts:6  ts:2304  Cannot find name 'x'  (in your diff; identical findings also at lines 2, 10)"]
    );
}

#[test]
fn describe_falls_back_to_the_group_listing() {
    let g = group(&[2, 6, 10], 1);
    let lines = describe_new_with(&[g], &|_| vec![set(&[])]);
    assert_eq!(
        lines,
        ["src/app.ts  ts:2304  Cannot find name 'x'  (1 new among 3 identical findings, lines 2, 6, 10)"]
    );
}

#[test]
fn resolver_is_only_consulted_for_ambiguous_groups() {
    let calls = Cell::new(0);
    let resolver = |_: &str| {
        calls.set(calls.get() + 1);
        vec![]
    };
    // Whole group new, and a single new finding: nothing to disambiguate.
    describe_new_with(&[group(&[2, 6], 2), group(&[4], 1)], &resolver);
    assert_eq!(calls.get(), 0);
    describe_new_with(&[group(&[2, 6, 10], 1)], &resolver);
    assert_eq!(calls.get(), 1);
}

#[test]
fn describe_new_without_git_lists_the_group() {
    let lines = describe_new(&[group(&[2, 6, 10], 2)]);
    assert!(lines[0].contains("2 new among 3 identical findings"), "{lines:?}");
}

// --- against a real git repository ---

const TWO_TWINS: &str = "function a() {\n  return missing;\n}\n\nfunction b() {\n  return missing;\n}\n";
const THREE_TWINS: &str = "function a() {\n  return missing;\n}\n\nfunction m() {\n  return missing;\n}\n\nfunction b() {\n  return missing;\n}\n";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be installed to run these tests");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn commit_all(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

fn repo_with_two_twins() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/app.ts"), TWO_TWINS).unwrap();
    commit_all(dir.path(), "base");
    dir
}

fn write_app(dir: &Path, content: &str) {
    std::fs::write(dir.join("src/app.ts"), content).unwrap();
}

#[test]
fn uncommitted_insert_is_found_against_head() {
    let dir = repo_with_two_twins();
    write_app(dir.path(), THREE_TWINS);
    let added = added_lines(dir.path(), "HEAD", "src/app.ts").unwrap();
    assert_eq!(added, set(&[5, 6, 7, 8]));
    assert_eq!(added_line_tiers(dir.path(), "src/app.ts")[0], added);
}

#[test]
fn committed_branch_change_is_found_against_the_default_branch() {
    let dir = repo_with_two_twins();
    git(dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_app(dir.path(), THREE_TWINS);
    commit_all(dir.path(), "add twin");

    let tiers = added_line_tiers(dir.path(), "src/app.ts");
    assert_eq!(tiers.len(), 2);
    assert!(tiers[0].is_empty(), "nothing uncommitted");
    assert_eq!(tiers[1], set(&[5, 6, 7, 8]));

    // End to end through the pure layer: the twin at line 6 is the one in the diff.
    let g = group(&[2, 6, 10], 1);
    assert_eq!(locate_new(&g, &tiers), Some(vec![6]));
}

#[test]
fn clean_default_branch_has_no_base_and_empty_tiers() {
    let dir = repo_with_two_twins();
    assert_eq!(default_branch_base(dir.path()), None);
    let tiers = added_line_tiers(dir.path(), "src/app.ts");
    assert!(tiers.iter().all(HashSet::is_empty));
}

#[test]
fn reindented_decoy_is_not_mistaken_for_the_new_twin() {
    // The existing twin at line 6 is only re-indented, and the new twin sits at line 2.
    // Both are "added lines": 2 hits for 1 excess → inconclusive, fall back.
    let dir = repo_with_two_twins();
    write_app(
        dir.path(),
        "function n() {\n  return missing;\n}\n\nfunction a() {\n  return missing;\n}\n\nfunction b() {\n\treturn missing;\n}\n",
    );
    let tiers = added_line_tiers(dir.path(), "src/app.ts");
    let g = group(&[2, 6, 10], 1);
    assert_eq!(locate_new(&g, &tiers), None);
}

#[test]
fn no_git_or_untracked_file_gives_empty_tiers_without_error() {
    let no_repo = tempfile::tempdir().unwrap();
    std::fs::write(no_repo.path().join("f.ts"), "x\n").unwrap();
    assert!(added_line_tiers(no_repo.path(), "f.ts").iter().all(HashSet::is_empty));

    let dir = repo_with_two_twins();
    std::fs::write(dir.path().join("src/new.ts"), "x\n").unwrap();
    assert!(added_line_tiers(dir.path(), "src/new.ts").iter().all(HashSet::is_empty));
}
