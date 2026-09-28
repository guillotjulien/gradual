//! Edit-scenario tests for finding identity.
//!
//! Each test scans a "before" and an "after" state of a small repo, exactly as
//! `gradual check` would (through `build_findings` and the production `diff`), and
//! asserts on the change in accepted counts per id: `added` = new findings (what
//! fails the build), `removed` = findings that disappeared from the baseline.
//!
//! Sections:
//!   A. stability   — everyday edits that must NOT change ids
//!   B. sensitivity — edits that MUST change ids (pins the intended trade-offs)
//!   C. duplicates  — twins (identical findings) are counted, never numbered
//!   D. edge cases  — EOF, missing files, path filtering
//!   E. golden pins — literal expected values; failing means the algorithm changed
//!                    and every committed baseline would be invalidated
//!   F. branches    — parallel branches touching the same twin group

use gradual::analyzers::types::RawFinding;
use gradual::config::{GradualConfig, PathFilter};
use gradual::events::diff::{
    KeyDelta, concurrent_fix_hint, describe_new, diff, group_by_id, new_count, removed_count,
    repeatedly_fixed_ids,
};
use gradual::events::fold::fold;
use gradual::events::types::{Change, DeltaEvent, EVENT_VERSION, Entry, Finding};
use gradual::identity::build_findings;
use gradual::identity::hasher::{compute_block_id, line_block, normalize_message};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ===========================================================================
// Harness
// ===========================================================================

const RULE: &str = "ts:2304";
const MSG: &str = "Cannot find name 'x'";
const APP: &str = "src/app.ts";
const UTIL: &str = "src/util.ts";

/// A finding the simulated analyzer reports: "the `nth` line containing `needle` in
/// `file`". Locating by needle keeps line numbers correct after each simulated edit.
#[derive(Clone)]
struct Mark {
    file: &'static str,
    needle: &'static str,
    nth: usize,
    col: u32,
    rule: &'static str,
    msg: &'static str,
}

/// `{ROOT}` in a message is replaced with the checkout's absolute path.
fn mark(file: &'static str, needle: &'static str) -> Mark {
    Mark { file, needle, nth: 0, col: 1, rule: RULE, msg: MSG }
}

impl Mark {
    fn nth(mut self, nth: usize) -> Self {
        self.nth = nth;
        self
    }
    fn col(mut self, col: u32) -> Self {
        self.col = col;
        self
    }
    fn rule(mut self, rule: &'static str) -> Self {
        self.rule = rule;
        self
    }
    fn msg(mut self, msg: &'static str) -> Self {
        self.msg = msg;
        self
    }
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn repo(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, content) in files {
        write(dir.path(), rel, content);
    }
    dir
}

fn line_of(src: &str, needle: &str, nth: usize) -> u32 {
    let idx = src
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .nth(nth)
        .unwrap_or_else(|| panic!("no match #{nth} for {needle:?} in:\n{src}"))
        .0;
    u32::try_from(idx).unwrap() + 1
}

fn raws(root: &Path, files: &[(&str, &str)], marks: &[Mark]) -> Vec<RawFinding> {
    marks
        .iter()
        .map(|m| {
            let src = files
                .iter()
                .find(|(rel, _)| *rel == m.file)
                .unwrap_or_else(|| panic!("mark refers to unknown file {}", m.file))
                .1;
            RawFinding {
                rule: m.rule.to_string(),
                file: root.join(m.file),
                line: line_of(src, m.needle, m.nth),
                column: m.col,
                message: m.msg.replace("{ROOT}", &root.to_string_lossy()),
            }
        })
        .collect()
}

fn all_paths() -> PathFilter {
    GradualConfig {
        tsconfig: "tsconfig.json".into(),
        eslint_config: None,
        events_dir: ".gradual/events".into(),
        include: vec![],
        exclude: vec![],
    }
    .path_filter()
    .unwrap()
}

/// Runs the full id pipeline on a fresh checkout of `files`.
fn scan(files: &[(&str, &str)], marks: &[Mark]) -> Vec<Finding> {
    let dir = repo(files);
    build_findings(&raws(dir.path(), files, marks), dir.path(), &all_paths())
}

/// The baseline `gradual init` would record for these findings.
fn baseline_of(findings: &[Finding]) -> HashMap<String, Entry> {
    fold(&[event(T0, changes_for(findings, &HashMap::new()))])
}

fn changes_for(current: &[Finding], baseline: &HashMap<String, Entry>) -> Vec<Change> {
    diff(&group_by_id(current.to_vec()), baseline).iter().map(KeyDelta::change).collect()
}

/// What `check` sees when `before` is the baseline and `after` the current state.
fn deltas(before: &[Finding], after: &[Finding]) -> Vec<KeyDelta> {
    diff(&group_by_id(after.to_vec()), &baseline_of(before))
}

/// `(added, removed)` counts, as `check` reports them.
fn delta(before: &[Finding], after: &[Finding]) -> (u64, u64) {
    let d = deltas(before, after);
    (new_count(&d), removed_count(&d))
}

#[track_caller]
fn assert_stable(before: &[Finding], after: &[Finding]) {
    let d = deltas(before, after);
    assert!(d.is_empty(), "accepted counts changed: {d:#?}");
}

/// The edit must surface as exactly `added` new findings and `removed` vanished ones.
#[track_caller]
fn assert_rekeyed(before: &[Finding], after: &[Finding], added: u64, removed: u64) {
    assert_eq!(delta(before, after), (added, removed), "(added, removed)");
}

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

const GREET: &str = r#"export function greet(name: string): string {
  const message = "Hello, " + name;
  return message;
}
"#;

const AUTH: &str = r"export class AuthService {
  validateToken(token: string): boolean {
    const result = token.length > 0;
    return result;
  }
}
";

const UTIL_SRC: &str = r"export const DEFAULT_TIMEOUT = 30;

export function clamp(n: number): number {
  return Math.min(n, LIMIT);
}
";

/// One function with the offending line.
const ONE_TWIN: &str = r"function a() {
  return legacy.value;
}
";

/// Two functions with an identical (twin) offending line.
const TWINS: &str = r"function a() {
  return legacy.value;
}

function b() {
  return legacy.value;
}
";

fn compose(parts: &[&str]) -> String {
    parts.join("\n")
}

fn app() -> String {
    compose(&[GREET, AUTH])
}

/// One finding in `greet` and one in `AuthService`.
fn app_marks() -> Vec<Mark> {
    vec![mark(APP, "const message"), mark(APP, "const result")]
}

fn scan_app(src: &str) -> Vec<Finding> {
    scan(&[(APP, src)], &app_marks())
}

// ===========================================================================
// A. Stability: everyday edits must NOT change ids
// ===========================================================================

#[test]
fn a01_insert_import_above() {
    let after = format!("import {{ readFile }} from \"fs\";\n\n{}", app());
    assert_stable(&scan_app(&app()), &scan_app(&after));
}

#[test]
fn a02_delete_lines_above() {
    let with_header = format!("// header\n// header\nimport x from \"x\";\n\n{}", app());
    assert_stable(&scan_app(&with_header), &scan_app(&app()));
}

#[test]
fn a03_add_statement_right_after_finding() {
    let after = app().replace(
        "  const message = \"Hello, \" + name;\n",
        "  const message = \"Hello, \" + name;\n  console.log(message);\n",
    );
    assert_stable(&scan_app(&app()), &scan_app(&after));
}

#[test]
fn a04_edit_line_just_before_finding() {
    // Signature lines directly above both findings change.
    let after = app()
        .replace("greet(name: string)", "greet(name: string, polite = true)")
        .replace("validateToken(token: string)", "validateToken(token: string | null)");
    assert_stable(&scan_app(&app()), &scan_app(&after));
}

#[test]
fn a05_reindent_when_wrapped_in_new_block() {
    let after = app().replace(
        "  const message = \"Hello, \" + name;\n  return message;\n",
        "  if (name) {\n    const message = \"Hello, \" + name;\n    return message;\n  }\n  return \"\";\n",
    );
    assert_stable(&scan_app(&app()), &scan_app(&after));
}

#[test]
fn a05b_tabs_instead_of_spaces() {
    assert_stable(&scan_app(&app()), &scan_app(&app().replace("  ", "\t")));
}

#[test]
fn a06_trailing_whitespace_on_finding_line() {
    let after = app().replace("+ name;\n", "+ name;   \t\n");
    assert_stable(&scan_app(&app()), &scan_app(&after));
}

#[test]
fn a07_line_endings_lf_to_crlf_and_back() {
    let crlf = app().replace('\n', "\r\n");
    assert_stable(&scan_app(&app()), &scan_app(&crlf));
    assert_stable(&scan_app(&crlf), &scan_app(&app()));
}

#[test]
fn a08_reorder_declarations_in_same_file() {
    let after = compose(&[AUTH, GREET]);
    assert_stable(&scan_app(&app()), &scan_app(&after));
}

#[test]
fn a09_edits_in_another_file() {
    let marks = [mark(APP, "const message"), mark(UTIL, "Math.min")];
    let before = scan(&[(APP, &app()), (UTIL, UTIL_SRC)], &marks);

    let util_after = format!("// utils\nimport {{ LIMIT }} from \"./limits\";\n\nexport const A = 1;\n\n{UTIL_SRC}");
    let app_after = format!("// touched\n{}", app());
    let after = scan(&[(APP, &app_after), (UTIL, &util_after)], &marks);
    assert_stable(&before, &after);

    // Editing only util.ts leaves app.ts's id byte-identical.
    let only_util = scan(&[(APP, &app()), (UTIL, &util_after)], &marks);
    assert_eq!(before[0].id, only_util[0].id);
}

#[test]
fn a10_same_repo_checked_out_at_different_absolute_paths() {
    let marks = [
        mark(APP, "const message").msg(
            "Could not find a declaration file for module 'semver'. '{ROOT}/node_modules/semver/index.js' implicitly has an 'any' type.",
        ),
        mark(APP, "const result").msg("Type mismatch in {ROOT}/src/app.ts"),
    ];
    let a = scan(&[(APP, &app())], &marks);
    let b = scan(&[(APP, &app())], &marks);
    assert_eq!(a.len(), 2);
    assert_stable(&a, &b);
}

#[test]
fn a11_analyzer_emission_order_is_irrelevant() {
    // Twins + a distinct finding: output order is by source position, not emission.
    let files = [(APP, TWINS)];
    let marks = [
        mark(APP, "legacy.value"),
        mark(APP, "legacy.value").nth(1),
        mark(APP, "function b").msg("other"),
    ];
    let dir = repo(&files);
    let ordered = raws(dir.path(), &files, &marks);

    let key = |mut r: Vec<RawFinding>, f: fn(&mut Vec<RawFinding>)| {
        f(&mut r);
        build_findings(&r, dir.path(), &all_paths())
            .into_iter()
            .map(|f| (f.id, f.line))
            .collect::<Vec<_>>()
    };
    let baseline = key(ordered.clone(), |_| {});
    assert_eq!(baseline, key(ordered.clone(), |r| r.reverse()));
    assert_eq!(baseline, key(ordered, |r| r.rotate_left(1)));
}

#[test]
fn a12_column_is_not_part_of_identity() {
    let before = scan(&[(APP, &app())], &[mark(APP, "const message").col(3)]);
    let after = scan(&[(APP, &app())], &[mark(APP, "const message").col(19)]);
    assert_stable(&before, &after);
}

#[test]
fn a13_realistic_commit_only_touches_what_changed() {
    // One commit: add an import, add a doc comment, re-indent with tabs, fix the
    // `result` finding, and introduce a brand-new finding in a new function.
    let before = scan_app(&app());

    let after_src = format!(
        "import {{ readFile }} from \"fs\";\n\n{}",
        compose(&[GREET, "/** Validates tokens. */", AUTH, "export const farewell = (n: string) => bye(n);\n"])
    )
    .replace("  ", "\t")
    .replace("token.length > 0", "typeof token === \"string\"");

    let marks = [mark(APP, "const message"), mark(APP, "bye(n)").msg("Cannot find name 'bye'")];
    let after = scan(&[(APP, &after_src)], &marks);

    let d = deltas(&before, &after);
    assert_eq!((new_count(&d), removed_count(&d)), (1, 1), "{d:#?}");
    let line = line_of(&after_src, "bye(n)", 0);
    assert_eq!(describe_new(&d), [format!("{APP}:{line}  {RULE}  Cannot find name 'bye'")]);
    let gone = d.iter().find(|k| k.delta < 0).unwrap();
    assert_eq!((gone.file.as_str(), gone.rule.as_str()), (APP, RULE));
}

// ===========================================================================
// B. Sensitivity: edits that MUST change ids
// ===========================================================================

#[test]
fn b01_editing_the_finding_line() {
    let after = app().replace("const message =", "const message: string =");
    assert_rekeyed(&scan_app(&app()), &scan_app(&after), 1, 1);
}

#[test]
fn b02_renaming_or_moving_the_file() {
    let marks = |file| vec![mark(file, "const message"), mark(file, "const result")];
    let before = scan(&[(APP, &app())], &marks(APP));
    let renamed = scan(&[("src/greeter.ts", &app())], &marks("src/greeter.ts"));
    let moved = scan(&[("lib/app.ts", &app())], &marks("lib/app.ts"));
    assert_rekeyed(&before, &renamed, 2, 2);
    assert_rekeyed(&before, &moved, 2, 2);
}

#[test]
fn b03_message_change() {
    let before = scan(&[(APP, &app())], &[mark(APP, "const message").msg("Type 'A' is not assignable")]);
    let after = scan(&[(APP, &app())], &[mark(APP, "const message").msg("Type 'B' is not assignable")]);
    assert_rekeyed(&before, &after, 1, 1);
}

#[test]
fn b03b_message_whitespace_only_is_ignored() {
    let before = scan(&[(APP, &app())], &[mark(APP, "const message").msg("Type 'A'  is\nnot assignable")]);
    let after = scan(&[(APP, &app())], &[mark(APP, "const message").msg("Type 'A' is not assignable")]);
    assert_stable(&before, &after);
}

#[test]
fn b04_rule_change() {
    let before = scan(&[(APP, &app())], &[mark(APP, "const message").rule("ts:2304")]);
    let after = scan(&[(APP, &app())], &[mark(APP, "const message").rule("ts:2552")]);
    assert_rekeyed(&before, &after, 1, 1);
}

#[test]
fn b05_formatter_wraps_the_finding_line() {
    let after = app().replace(
        "  const message = \"Hello, \" + name;\n",
        "  const message =\n    \"Hello, \" +\n    name;\n",
    );
    assert_rekeyed(&scan_app(&app()), &scan_app(&after), 1, 1);
}

#[test]
fn b06_intra_line_spacing_change_is_a_known_sensitivity() {
    // Whitespace is collapsed, not removed: `" + "` → `"+"` changes the block.
    let after = app().replace("\"Hello, \" + name", "\"Hello, \"+name");
    assert_rekeyed(&scan_app(&app()), &scan_app(&after), 1, 1);
}

// ===========================================================================
// C. Duplicates and counters
// ===========================================================================

fn twin_marks() -> Vec<Mark> {
    vec![mark(APP, "legacy.value"), mark(APP, "legacy.value").nth(1)]
}

#[test]
fn c01_twins_share_an_id_and_are_counted() {
    let f = scan(&[(APP, TWINS)], &twin_marks());
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].id, f[1].id, "twins are not numbered");
    assert!(!f[0].id.contains(':'));
    let baseline = baseline_of(&f);
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[&f[0].id].count, 2);
}

#[test]
fn c02_fixing_either_twin_removes_exactly_one_finding() {
    let before = scan(&[(APP, TWINS)], &twin_marks());

    let fixed_first = TWINS.replacen("legacy.value", "current.value", 1);
    let mut fixed_second = TWINS.to_string();
    let idx = fixed_second.rfind("legacy.value").unwrap();
    fixed_second.replace_range(idx..idx + "legacy.value".len(), "current.value");

    // The analyzer now reports only the remaining twin.
    for fixed in [fixed_first, fixed_second] {
        let after = scan(&[(APP, &fixed)], &[mark(APP, "legacy.value")]);
        assert_rekeyed(&before, &after, 0, 1);
    }
}

#[test]
fn c03_copy_pasting_a_new_twin_is_reported_as_one_new_finding() {
    let before = scan(&[(APP, ONE_TWIN)], &[mark(APP, "legacy.value")]);
    let after = scan(&[(APP, TWINS)], &twin_marks());
    assert_eq!((before.len(), after.len()), (1, 2));
    assert_rekeyed(&before, &after, 1, 0);
}

#[test]
fn c04_same_line_same_rule_different_columns() {
    let src = "const v = missing + missing;\n";
    let marks = [mark(APP, "missing").col(11), mark(APP, "missing").col(21)];
    let f = scan(&[(APP, src)], &marks);
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].id, f[1].id);
    let g = scan(&[(APP, src)], &[marks[1].clone(), marks[0].clone()]);
    assert_stable(&f, &g);
    // Dropping one of them is one finding removed.
    assert_rekeyed(&f, &scan(&[(APP, src)], &marks[..1]), 0, 1);
}

#[test]
fn c05_twins_in_different_files_are_separate_groups() {
    let marks = [mark(APP, "legacy.value"), mark(UTIL, "legacy.value")];
    let f = scan(&[(APP, TWINS), (UTIL, TWINS)], &marks);
    assert_eq!(f.len(), 2);
    assert_ne!(f[0].id, f[1].id);
    assert!(baseline_of(&f).values().all(|e| e.count == 1));
}

#[test]
fn c06_same_line_different_messages_are_separate_groups() {
    let marks = [mark(APP, "missing").msg("first"), mark(APP, "missing").col(2).msg("second")];
    let f = scan(&[(APP, "const v = missing;\n")], &marks);
    assert_ne!(f[0].id, f[1].id);
}

#[test]
fn c07_twin_inserted_between_twins_lists_the_whole_group() {
    // Which of three identical findings is the new one cannot be told from content
    // alone, so `check` reports the count and lists every location.
    let between = TWINS.replacen(
        "\nfunction b() {",
        "\nfunction m() {\n  return legacy.value;\n}\n\nfunction b() {",
        1,
    );
    let before = scan(&[(APP, TWINS)], &twin_marks());
    let marks = [
        mark(APP, "legacy.value"),
        mark(APP, "legacy.value").nth(1),
        mark(APP, "legacy.value").nth(2),
    ];
    let after = scan(&[(APP, &between)], &marks);

    let d = deltas(&before, &after);
    assert_eq!((new_count(&d), removed_count(&d)), (1, 0));
    assert_eq!(
        describe_new(&d),
        [format!("{APP}  {RULE}  {MSG}  (1 new among 3 identical findings, lines 2, 6, 10)")]
    );
}

// ===========================================================================
// D. Edge cases
// ===========================================================================

fn single_raw(root: &Path, rel: &str, line: u32) -> RawFinding {
    RawFinding {
        rule: RULE.into(),
        file: root.join(rel),
        line,
        column: 1,
        message: MSG.into(),
    }
}

#[test]
fn d01_line_beyond_eof_hashes_like_an_empty_line() {
    let dir = repo(&[(APP, "const a = 1;\n\nconst b = 2;\n")]);
    let id = |line| {
        let f = build_findings(&[single_raw(dir.path(), APP, line)], dir.path(), &all_paths());
        assert_eq!(f.len(), 1, "finding at line {line} must not be dropped");
        f[0].id.clone()
    };
    assert_eq!(id(2), id(99), "stale line number should behave like a blank line");
    assert_eq!(id(0), id(1), "line 0 clamps to the first line");
}

#[test]
fn d02_final_newline_added_or_removed() {
    let with_nl = "const a = 1;\nconst b = missing;\n";
    let without_nl = "const a = 1;\nconst b = missing;";
    let marks = [mark(APP, "missing")];
    assert_stable(&scan(&[(APP, without_nl)], &marks), &scan(&[(APP, with_nl)], &marks));
}

#[test]
fn d03_missing_file_is_skipped_without_affecting_others() {
    let dir = repo(&[(APP, &app())]);
    let good = raws(dir.path(), &[(APP, &app())], &[mark(APP, "const message")]);
    let mut with_ghost = good.clone();
    with_ghost.push(single_raw(dir.path(), "src/deleted.ts", 3));
    let a = build_findings(&good, dir.path(), &all_paths());
    let b = build_findings(&with_ghost, dir.path(), &all_paths());
    assert_eq!(b.len(), 1);
    assert_stable(&a, &b);
}

#[test]
fn d04_findings_outside_repo_root_are_dropped() {
    let repo_a = repo(&[(APP, &app())]);
    let repo_b = repo(&[(APP, &app())]);
    let files = [(APP, app())];
    let files: Vec<(&str, &str)> = files.iter().map(|(p, c)| (*p, c.as_str())).collect();
    let mut all = raws(repo_a.path(), &files, &[mark(APP, "const message")]);
    let alone = build_findings(&all, repo_a.path(), &all_paths());
    all.extend(raws(repo_b.path(), &files, &[mark(APP, "const result")]));
    let mixed = build_findings(&all, repo_a.path(), &all_paths());
    assert_eq!(mixed.len(), 1);
    assert_stable(&alone, &mixed);
}

#[test]
fn d05_excluded_paths_do_not_perturb_included_ids() {
    let filter = GradualConfig {
        tsconfig: "tsconfig.json".into(),
        eslint_config: None,
        events_dir: ".gradual/events".into(),
        include: vec!["src/**/*.ts".into()],
        exclude: vec!["**/*.spec.ts".into()],
    }
    .path_filter()
    .unwrap();

    let files = [(APP, TWINS), ("src/app.spec.ts", TWINS), ("node_modules/x/index.ts", TWINS)];
    let dir = repo(&files);
    let marks = |file| [mark(file, "legacy.value"), mark(file, "legacy.value").nth(1)];

    let app_only = build_findings(&raws(dir.path(), &files, &marks(APP)), dir.path(), &filter);
    let mut everything = raws(dir.path(), &files, &marks("src/app.spec.ts"));
    everything.extend(raws(dir.path(), &files, &marks("node_modules/x/index.ts")));
    everything.extend(raws(dir.path(), &files, &marks(APP)));
    let filtered = build_findings(&everything, dir.path(), &filter);

    assert_eq!(filtered.len(), 2);
    assert_stable(&app_only, &filtered);
}

// ===========================================================================
// E. Golden pins
//
// These literals are the on-disk contract: ids are persisted in committed baseline
// files. If one of these tests fails, the identity algorithm changed and existing
// baselines will report every finding as new. Only update the expected values as a
// deliberate, released, baseline-breaking change.
// ===========================================================================

fn base_id(root: &Path, rel: &str, content: &str, rule: &str, line: u32, message: &str) -> String {
    write(root, rel, content);
    let finding = RawFinding {
        rule: rule.into(),
        file: PathBuf::from(root).join(rel),
        line,
        column: 1,
        message: message.into(),
    };
    compute_block_id(&finding, root).unwrap()
}

const GOLDEN_SIMPLE: &str = "42bef2ce2019430c15d88054fa42ff05";
const GOLDEN_NODE_MODULES: &str = "65b08646d9edd5df2ae58b30241651f4";
const GOLDEN_OTHER_RULE_FILE: &str = "021b51b72da53a7c7cd6ef37a4b06f0b";

#[test]
fn e01_golden_base_ids() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let simple = base_id(root, "src/app.ts", "const a = 1;\n", "ts:2304", 1, "Cannot find name 'x'");
    assert_eq!(simple, GOLDEN_SIMPLE);

    // Whitespace, CRLF and neighbouring lines don't matter: same golden value.
    let noisy = base_id(root, "src/app.ts", "// c\r\n\t  const   a =  1;  \r\nfoo();\r\n", "ts:2304", 2, "Cannot   find\nname 'x'");
    assert_eq!(noisy, GOLDEN_SIMPLE);

    let msg = format!("Could not find a declaration file for module 'semver'. '{}/node_modules/semver/index.js' implicitly has an 'any' type.", root.display());
    let nm = base_id(root, "src/app.ts", "import s from \"semver\";\n", "ts:7016", 1, &msg);
    assert_eq!(nm, GOLDEN_NODE_MODULES);

    let other = base_id(root, "lib/deep/mod.ts", "export const a = 1;\n", "no-unused-vars", 1, "'a' is defined but never used.");
    assert_eq!(other, GOLDEN_OTHER_RULE_FILE);
}

#[test]
fn e02_golden_ids_of_twins() {
    let files = [(APP, TWINS)];
    let f = scan(&files, &twin_marks());
    let got: Vec<&str> = f.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(got, ["34fabf62bd431cf4179a6d3102c6d820"; 2]);
}

#[test]
fn e03_id_format_is_32_lowercase_hex() {
    for finding in &scan(&[(APP, TWINS)], &twin_marks()) {
        assert_eq!(finding.id.len(), 32);
        assert!(finding.id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')), "{}", finding.id);
    }
}

#[test]
fn e04_golden_line_block() {
    assert_eq!(line_block("a\n  b \t c  \nd\n", 2), "b c");
    assert_eq!(line_block("a\r\n  b \t c  \r\nd\r\n", 2), "b c");
    assert_eq!(line_block("a\n", 5), "");
    assert_eq!(line_block("a\nb\n", 0), "a");
}

#[test]
fn e05_golden_normalize_message() {
    let root = Path::new("/work/repo");
    assert_eq!(
        normalize_message("Cannot find '/work/repo/src/a.ts'", root),
        "Cannot find '/src/a.ts'"
    );
    assert_eq!(
        normalize_message("see '/home/u/proj/node_modules/@types/x/index.d.ts'  here", root),
        "see 'node_modules/@types/x/index.d.ts' here"
    );
    assert_eq!(normalize_message("  a \n\t b  ", root), "a b");
}

// ===========================================================================
// F. Concurrent branches
//
// Two branches each run `gradual update --force` from the same baseline; their event
// files are then merged (fold sums every event). Each branch is green on its own, so
// the merged baseline must be as well. This is why twins are counted instead of
// numbered: positional ids (`base:n`) let parallel branches reuse or remove the same
// id for different lines, corrupting the merged baseline.
// ===========================================================================

const T0: &str = "2026-01-01T00:00:00Z";
const T1: &str = "2026-01-01T01:00:00Z";
const T2: &str = "2026-01-01T02:00:00Z";

/// Scans `src` with one finding per `legacy.value` line.
fn scan_twins(src: &str) -> Vec<Finding> {
    let marks: Vec<Mark> = (0..src.matches("legacy.value").count())
        .map(|n| mark(APP, "legacy.value").nth(n))
        .collect();
    scan(&[(APP, src)], &marks)
}

fn event(ts: &str, changes: Vec<Change>) -> DeltaEvent {
    DeltaEvent {
        version: EVENT_VERSION,
        commit: "abc1234".into(),
        parent: "def5678".into(),
        timestamp: ts.into(),
        changes,
    }
}

fn genesis(src: &str) -> DeltaEvent {
    event(T0, changes_for(&scan_twins(src), &HashMap::new()))
}

/// What `gradual update --force` on a branch at `history` would record for `src`.
fn branch(history: &[DeltaEvent], src: &str, ts: &str) -> DeltaEvent {
    event(ts, changes_for(&scan_twins(src), &fold(history)))
}

/// Number of findings `check` would report as new.
fn new_on_check(baseline: &HashMap<String, Entry>, src: &str) -> u64 {
    new_count(&diff(&group_by_id(scan_twins(src)), baseline))
}

fn fix_first(src: &str) -> String {
    src.replacen("legacy.value", "current.value", 1)
}

fn fix_last(src: &str) -> String {
    let mut out = src.to_string();
    let idx = out.rfind("legacy.value").unwrap();
    out.replace_range(idx..idx + "legacy.value".len(), "current.value");
    out
}

fn add_twin(src: &str, name: &str) -> String {
    format!("{src}\nfunction {name}() {{\n  return legacy.value;\n}}\n")
}

#[test]
fn f00_sequential_branches_are_correct() {
    // Control: the same kind of edits, merged one after the other.
    let g = genesis(TWINS);
    let after_x_src = fix_first(TWINS);
    let x = branch(std::slice::from_ref(&g), &after_x_src, T1);
    let final_src = add_twin(&after_x_src, "c");
    let y = branch(&[g.clone(), x.clone()], &final_src, T2);

    let merged = fold(&[g, x, y]);
    assert_eq!(new_on_check(&merged, &final_src), 0);

    // The baseline tracks exactly the two twins that remain accepted.
    assert_eq!(new_on_check(&merged, &add_twin(&final_src, "d")), 1);
}

#[test]
fn f01_parallel_fixes_of_different_twins() {
    // X fixes twin A, Y fixes twin B: -1 and -1 from a baseline of 2.
    let g = genesis(TWINS);
    let x = branch(std::slice::from_ref(&g), &fix_first(TWINS), T1);
    let y = branch(std::slice::from_ref(&g), &fix_last(TWINS), T2);
    let merged = fold(&[g, x, y]);

    let both_fixed = fix_last(&fix_first(TWINS));
    assert_eq!(new_on_check(&merged, &both_fixed), 0);
    assert!(merged.is_empty(), "phantom baseline entries: {merged:?}");

    // With no twins accepted anymore, reintroducing one must fail `check`.
    assert_eq!(new_on_check(&merged, &add_twin(&both_fixed, "c")), 1);
}

#[test]
fn f02_parallel_fix_and_accepted_twin() {
    // X fixes a twin (-1), Y accepts a new one (+1): the baseline stays at 2.
    let g = genesis(TWINS);
    let x_src = fix_first(TWINS);
    let x = branch(std::slice::from_ref(&g), &x_src, T1);
    let y = branch(std::slice::from_ref(&g), &add_twin(TWINS, "c"), T2);
    let merged = fold(&[g, x, y]);

    assert_eq!(new_on_check(&merged, &add_twin(&x_src, "c")), 0, "`check` must pass on main");
}

#[test]
fn f03_parallel_accepted_twins() {
    // X and Y each accept one new twin (+1 and +1): the baseline becomes 4.
    let g = genesis(TWINS);
    let x = branch(std::slice::from_ref(&g), &add_twin(TWINS, "c"), T1);
    let y = branch(std::slice::from_ref(&g), &add_twin(TWINS, "d"), T2);
    let merged = fold(&[g, x, y]);

    let merged_src = add_twin(&add_twin(TWINS, "c"), "d");
    assert_eq!(new_on_check(&merged, &merged_src), 0, "`check` must pass on main");
}

#[test]
fn f04_merge_result_does_not_depend_on_event_order() {
    let g = genesis(TWINS);
    let x = branch(std::slice::from_ref(&g), &fix_first(TWINS), T1);
    let y = branch(std::slice::from_ref(&g), &add_twin(TWINS, "c"), T2);
    let counts = |events: &[DeltaEvent]| {
        let mut v: Vec<_> = fold(events).into_iter().map(|(id, e)| (id, e.count)).collect();
        v.sort();
        v
    };
    let expected = counts(&[g.clone(), x.clone(), y.clone()]);
    for perm in [[&g, &y, &x], [&x, &g, &y], [&x, &y, &g], [&y, &g, &x], [&y, &x, &g]] {
        let events: Vec<DeltaEvent> = perm.iter().map(|e| (*e).clone()).collect();
        assert_eq!(counts(&events), expected);
    }
}

#[test]
fn f05_known_limitation_both_branches_fix_the_same_twin() {
    // Both branches fix twin A: -1 and -1 from 2 puts the baseline (0) below the
    // code (1 twin left). Events for "same twin fixed twice" and "two different twins
    // fixed" are identical, so this cannot be resolved silently without weakening the
    // ratchet. `check` fails on main (loud, safe), says why, and `gradual update
    // --force` records +1 and repairs the baseline.
    let g = genesis(TWINS);
    let fixed = fix_first(TWINS);
    let x = branch(std::slice::from_ref(&g), &fixed, T1);
    let y = branch(std::slice::from_ref(&g), &fixed, T2);
    let merged = fold(&[g.clone(), x.clone(), y.clone()]);
    assert_eq!(new_on_check(&merged, &fixed), 1);

    let d = diff(&group_by_id(scan_twins(&fixed)), &merged);
    let hint = concurrent_fix_hint(&d, &repeatedly_fixed_ids(&[g.clone(), x.clone(), y.clone()]));
    assert!(hint.is_some_and(|h| h.contains("gradual update --force")));

    let repair = branch(&[g.clone(), x.clone(), y.clone()], &fixed, "2026-01-01T03:00:00Z");
    let repaired = fold(&[g, x, y, repair]);
    assert_eq!(new_on_check(&repaired, &fixed), 0);
}

#[test]
fn f06_no_hint_for_a_plain_regression() {
    // A twin added after a single fix is a real regression: no concurrency hint.
    let g = genesis(TWINS);
    let fixed = fix_first(TWINS);
    let x = branch(std::slice::from_ref(&g), &fixed, T1);
    let merged = fold(&[g.clone(), x.clone()]);

    let d = diff(&group_by_id(scan_twins(&add_twin(&fixed, "c"))), &merged);
    assert_eq!(new_count(&d), 1);
    assert!(concurrent_fix_hint(&d, &repeatedly_fixed_ids(&[g, x])).is_none());
}

#[test]
fn f07_no_hint_when_a_single_finding_is_fixed_by_both_branches() {
    // Not a twin group: the clamped sum is already right, so nothing to explain.
    let one = ONE_TWIN;
    let none = one.replace("legacy.value", "current.value");
    let g = genesis(one);
    let x = branch(std::slice::from_ref(&g), &none, T1);
    let y = branch(std::slice::from_ref(&g), &none, T2);
    let events = [g, x, y];
    let merged = fold(&events);

    assert_eq!(new_on_check(&merged, &none), 0);
    let d = diff(&group_by_id(scan_twins(one)), &merged);
    assert_eq!(new_count(&d), 1, "reintroducing it is a regression");
    assert!(concurrent_fix_hint(&d, &repeatedly_fixed_ids(&events)).is_none());
}
