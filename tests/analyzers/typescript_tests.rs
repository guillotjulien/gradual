use gradual::analyzers::typescript::run_typescript;
use gradual::config::GradualConfig;
use std::path::PathBuf;

fn has_tsc() -> bool {
    std::process::Command::new("tsgo")
        .arg("--version")
        .output()
        .is_ok()
        || std::process::Command::new("tsc")
            .arg("--version")
            .output()
            .is_ok()
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/analyzers/fixtures")
}

fn test_config() -> GradualConfig {
    GradualConfig {
        tsconfig: "tsconfig.json".to_string(),
        eslint_config: None,
        events_dir: ".gradual/events".to_string(),
    }
}

#[test]
fn detects_type_error_in_fixture() {
    if !has_tsc() {
        eprintln!("skipping detects_type_error_in_fixture: tsc/tsgo not found on PATH");
        return;
    }
    let findings = run_typescript(&test_config(), &fixtures_dir(), None)
        .expect("run_typescript failed");

    assert!(!findings.is_empty(), "expected at least one finding from ts_error.ts");

    let f = findings
        .iter()
        .find(|f| f.rule == "ts:2322")
        .expect("expected TS2322 (type mismatch) finding");

    assert_eq!(f.line, 1, "error should be on line 1");
    assert!(
        f.file.to_string_lossy().ends_with("ts_error.ts"),
        "file should be ts_error.ts, got: {}",
        f.file.display()
    );
}

#[test]
fn finding_file_path_is_absolute() {
    if !has_tsc() {
        eprintln!("skipping finding_file_path_is_absolute: tsc/tsgo not found on PATH");
        return;
    }
    let findings = run_typescript(&test_config(), &fixtures_dir(), None)
        .expect("run_typescript failed");

    for f in &findings {
        assert!(
            f.file.is_absolute(),
            "expected absolute path, got: {}",
            f.file.display()
        );
    }
}
