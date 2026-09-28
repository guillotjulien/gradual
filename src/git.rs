use anyhow::Context;
use std::collections::HashSet;
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

fn git_output(repo_root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(repo_root).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Line numbers (in the new file) added by a `git diff -U0` output, read from the
/// hunk headers (`@@ -a,b +c,d @@`). `d` defaults to 1, and `d == 0` is a pure
/// deletion.
pub fn parse_added_lines(diff: &str) -> Vec<u32> {
    let mut lines = Vec::new();
    for header in diff.lines().filter(|l| l.starts_with("@@ ")) {
        let Some(new_range) = header.split_whitespace().nth(2).and_then(|r| r.strip_prefix('+'))
        else {
            continue;
        };
        let (start, count) = new_range.split_once(',').unwrap_or((new_range, "1"));
        if let (Ok(start), Ok(count)) = (start.parse::<u32>(), count.parse::<u32>()) {
            lines.extend(start..start + count);
        }
    }
    lines
}

/// Lines of `rel_file` (working tree) that were added relative to `base`.
pub fn added_lines(repo_root: &Path, base: &str, rel_file: &str) -> anyhow::Result<HashSet<u32>> {
    let out = Command::new("git")
        .args(["diff", "-U0", "--no-color", "--no-ext-diff", "--no-renames", base, "--", rel_file])
        .current_dir(repo_root)
        .output()
        .context("failed to run git diff")?;
    if !out.status.success() {
        anyhow::bail!("git diff failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(parse_added_lines(&String::from_utf8_lossy(&out.stdout)).into_iter().collect())
}

/// The commit where the current branch left the default branch, if there is one and
/// it differs from `HEAD`. Tries `origin/HEAD`, then common default branch names.
pub fn default_branch_base(repo_root: &Path) -> Option<String> {
    let head = git_output(repo_root, &["rev-parse", "HEAD"])?;
    let origin_head = git_output(repo_root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]);
    let default_ref = origin_head.into_iter().chain(
        ["origin/main", "origin/master", "main", "master"].map(String::from),
    ).find(|r| git_output(repo_root, &["rev-parse", "--verify", "--quiet", r]).is_some())?;
    let base = git_output(repo_root, &["merge-base", "HEAD", &default_ref])?;
    (base != head).then_some(base)
}

/// Added lines of `rel_file` in two tiers, tightest first: uncommitted changes
/// (against `HEAD`), then everything since the branch left the default branch.
/// Failures (no git, no history, untracked file) give empty tiers, never an error.
pub fn added_line_tiers(repo_root: &Path, rel_file: &str) -> Vec<HashSet<u32>> {
    let mut tiers = vec![added_lines(repo_root, "HEAD", rel_file).unwrap_or_default()];
    if let Some(base) = default_branch_base(repo_root) {
        tiers.push(added_lines(repo_root, &base, rel_file).unwrap_or_default());
    }
    tiers
}
