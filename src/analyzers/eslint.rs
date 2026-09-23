use crate::analyzers::{find_binary, run_command};
use crate::analyzers::types::RawFinding;
use crate::config::GradualConfig;
use anyhow::Context;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Deserialize)]
struct EslintFile {
    #[serde(rename = "filePath")]
    file_path: String,
    messages: Vec<EslintMessage>,
}

#[derive(Deserialize)]
struct EslintMessage {
    #[serde(rename = "ruleId")]
    rule_id: Option<String>,
    message: String,
    // Fatal messages (e.g. parse errors) omit position, so these are optional.
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    column: Option<u32>,
}

enum EslintVersion {
    V8,
    V9,
}

fn detect_version(repo_root: &Path, config_path: Option<&str>) -> EslintVersion {
    if let Some(cfg) = config_path
        && std::path::Path::new(cfg)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("js") || ext.eq_ignore_ascii_case("mjs") || ext.eq_ignore_ascii_case("cjs"))
    {
        return EslintVersion::V9;
    }
    if repo_root.join("eslint.config.js").exists()
        || repo_root.join("eslint.config.mjs").exists()
        || repo_root.join("eslint.config.cjs").exists()
    {
        EslintVersion::V9
    } else {
        EslintVersion::V8
    }
}

pub fn run_eslint(
    config: &GradualConfig,
    repo_root: &Path,
    timeout: Option<Duration>,
) -> anyhow::Result<Vec<RawFinding>> {
    let bin_path = find_binary("eslint", repo_root).ok_or_else(|| {
        anyhow::anyhow!(
            "ESLint not found. Install it with:\n  npm install eslint\n\
             Then add an eslint config path to gradual.json under \"eslint_config\"."
        )
    })?;

    // Use case-insensitive extension check so .JS/.MJS are treated the same.
    let config_lower = config
        .eslint_config
        .as_deref()
        .map(str::to_ascii_lowercase);
    let version = detect_version(repo_root, config_lower.as_deref());

    let mut cmd = Command::new(&bin_path);
    cmd.args(["--format", "json"]);

    // Persist eslint's per-file cache across runs. Sound because lint rules are
    // file-local: unchanged files are skipped. Best-effort dir creation.
    let cache_dir = repo_root.join(".gradual/cache");
    std::fs::create_dir_all(&cache_dir).ok();
    cmd.arg("--cache")
        .arg("--cache-location")
        .arg(cache_dir.join(".eslintcache"));

    match version {
        EslintVersion::V8 => {
            cmd.arg("--no-eslintrc");
            if let Some(cfg) = &config.eslint_config {
                cmd.args(["-c", cfg]);
            }
            cmd.args(["--ext", ".ts,.tsx"]);
        }
        EslintVersion::V9 => {
            cmd.arg("--no-config-lookup");
            if let Some(cfg) = &config.eslint_config {
                cmd.args(["--config", cfg]);
            }
        }
    }
    // Scope eslint to the configured paths (like betterer's `.include(...)`) so it
    // never lints build output such as `dist/`. Excludes become ignore patterns.
    // With no `include`, fall back to linting the whole repo.
    for pat in &config.exclude {
        cmd.args(["--ignore-pattern", pat]);
    }
    if config.include.is_empty() {
        cmd.arg(repo_root);
    } else {
        for pat in &config.include {
            cmd.arg(pat);
        }
    }
    cmd.current_dir(repo_root);

    let output = run_command(&mut cmd, timeout).with_context(|| {
        format!(
            "failed to run eslint ({}). Check that it is installed and on PATH \
             or in node_modules/.bin/.",
            bin_path.display()
        )
    })?;

    // ESLint exits 1 when lint errors are found — that's expected.
    if output.combined.trim().is_empty() {
        if output.exit_code != 0 {
            let exit_code = output.exit_code;
            anyhow::bail!(
                "eslint exited with code {exit_code} but produced no JSON output.\n\
                 Check your eslint config (currently: {}).\n\
                 To diagnose, run: eslint --format json {}",
                config
                    .eslint_config
                    .as_deref()
                    .unwrap_or("(auto-detected)"),
                repo_root.display()
            );
        }
        return Ok(Vec::new());
    }

    let files: Vec<EslintFile> = serde_json::from_str(&output.combined).with_context(|| {
        let preview = &output.combined[..output.combined.len().min(500)];
        format!("failed to parse eslint JSON output (first 500 chars):\n{preview}")
    })?;

    let mut findings = Vec::new();
    for file in files {
        for msg in file.messages {
            let Some(rule_id) = msg.rule_id else {
                // Parse errors from ESLint have null ruleId — skip them.
                eprintln!(
                    "warning: skipping eslint parse error in {}: {}",
                    file.file_path, msg.message
                );
                continue;
            };
            findings.push(RawFinding {
                rule: format!("eslint:{rule_id}"),
                file: PathBuf::from(&file.file_path),
                line: msg.line.unwrap_or(1),
                column: msg.column.unwrap_or(1),
                message: msg.message,
            });
        }
    }

    Ok(findings)
}
