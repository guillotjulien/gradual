use anyhow::Context;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn find_repo_root() -> anyhow::Result<PathBuf> {
    let mut dir = std::env::current_dir().context("failed to get current directory")?;
    loop {
        if dir.join(".git").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            anyhow::bail!(
                "not in a git repository (or any parent directory). \
                 Run `git init` to initialize one."
            );
        }
    }
}

pub fn get_current_sha(repo_root: &Path) -> anyhow::Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .context("failed to run git rev-parse HEAD")?;
    if !out.status.success() {
        anyhow::bail!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

pub fn get_parent_sha(repo_root: &Path) -> anyhow::Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD^"])
        .current_dir(repo_root)
        .output()
        .context("failed to run git rev-parse HEAD^")?;
    if !out.status.success() {
        anyhow::bail!(
            "git rev-parse HEAD^ failed (initial commit has no parent?): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}
