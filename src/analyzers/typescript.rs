use crate::analyzers::{find_binary, run_command};
use crate::analyzers::types::RawFinding;
use crate::config::GradualConfig;
use anyhow::Context;
use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

static TSC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+)\((\d+),(\d+)\): (?:error|warning) TS(\d+): (.+)$").unwrap()
});

pub fn run_typescript(
    config: &GradualConfig,
    repo_root: &Path,
    timeout: Option<Duration>,
) -> anyhow::Result<Vec<RawFinding>> {
    let tsconfig_path = repo_root.join(&config.tsconfig);
    let tsconfig_dir = tsconfig_path.parent().unwrap_or(repo_root);

    let bin_path = ["tsgo", "tsc"]
        .iter()
        .find_map(|name| find_binary(name, repo_root))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "TypeScript compiler not found. Install it with:\n  npm install typescript\n\
                 Or install tsgo for faster type-checking:\n  npm install @typescript-go/tsgo"
            )
        })?;

    let bin_name = bin_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tsc");

    let output = run_command(
        Command::new(&bin_path)
            .args(["--noEmit", "--pretty", "false", "-p"])
            .arg(&tsconfig_path)
            .current_dir(repo_root),
        timeout,
    )
    .with_context(|| {
        format!(
            "failed to run {bin_name} ({}). Check that it is installed and on PATH \
             or in node_modules/.bin/.",
            bin_path.display()
        )
    })?;

    let mut findings = Vec::new();
    for line in output.combined.lines() {
        let Some(caps) = TSC_RE.captures(line) else {
            continue;
        };
        let rel_file = caps[1].trim();
        let line_num: u32 = caps[2].parse()?;
        let col_num: u32 = caps[3].parse()?;
        let code = &caps[4];
        let message = caps[5].trim().to_string();

        let abs_file = tsconfig_dir
            .join(rel_file)
            .canonicalize()
            .with_context(|| {
                format!(
                    "{bin_name} reported an error in '{rel_file}', but that path could not be \
                     resolved. Check that your tsconfig's rootDir and include paths are correct."
                )
            })?;

        findings.push(RawFinding {
            rule: format!("ts:{code}"),
            file: abs_file,
            line: line_num,
            column: col_num,
            message,
        });
    }

    // tsc exits nonzero when there are type errors — that's expected.
    // If it exits nonzero but produced zero parseable findings, something went wrong.
    if findings.is_empty() && output.exit_code != 0 {
        let exit_code = output.exit_code;
        anyhow::bail!(
            "{bin_name} exited with code {exit_code} but produced no parseable output.\n\
             Check that your tsconfig path is correct: {}\n\
             Raw output:\n{}",
            tsconfig_path.display(),
            output.combined.trim()
        );
    }

    Ok(findings)
}
