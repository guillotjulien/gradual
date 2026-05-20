pub mod eslint;
pub mod typescript;
pub mod types;

use crate::analyzers::types::RawFinding;
use crate::config::GradualConfig;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use wait_timeout::ChildExt;

pub struct CommandOutput {
    pub exit_code: i32,
    pub combined: String,
}

/// Run a command, capturing stdout+stderr. Kills the process if timeout elapses.
pub fn run_command(
    cmd: &mut std::process::Command,
    timeout: Option<Duration>,
) -> anyhow::Result<CommandOutput> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    // Drain pipes in threads — prevents deadlock if the process writes enough
    // output to fill the OS pipe buffer before we call wait().
    let mut stdout_pipe = child.stdout.take().expect("stdout was piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr was piped");
    let stdout_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stdout_pipe.read_to_end(&mut buf).ok();
        buf
    });
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stderr_pipe.read_to_end(&mut buf).ok();
        buf
    });

    let status = if let Some(dur) = timeout {
        let Some(s) = child.wait_timeout(dur)? else {
            child.kill().ok();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            anyhow::bail!(
                "process timed out after {}s. Increase --timeout if your codebase is large.",
                dur.as_secs()
            );
        };
        s
    } else {
        child.wait()?
    };

    let stdout = stdout_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader thread panicked"))?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader thread panicked"))?;

    Ok(CommandOutput {
        exit_code: status.code().unwrap_or(-1),
        combined: format!(
            "{}{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        ),
    })
}

/// Find a named binary: checks `<repo_root>/node_modules/.bin/` first, then PATH.
pub fn find_binary(name: &str, repo_root: &Path) -> Option<PathBuf> {
    let node_bin = repo_root.join("node_modules/.bin").join(name);
    if node_bin.exists() {
        return Some(node_bin);
    }
    // Search PATH without spawning a subprocess.
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(name);
            if candidate.exists() { Some(candidate) } else { None }
        })
    })
}

pub fn run_all(
    config: &GradualConfig,
    repo_root: &Path,
    timeout: Option<Duration>,
) -> anyhow::Result<Vec<RawFinding>> {
    let ts_config = config.clone();
    let ts_root = repo_root.to_path_buf();
    let ts_handle =
        thread::spawn(move || typescript::run_typescript(&ts_config, &ts_root, timeout));

    let eslint_handle = if config.eslint_config.is_some() {
        let eslint_config = config.clone();
        let eslint_root = repo_root.to_path_buf();
        Some(thread::spawn(move || {
            eslint::run_eslint(&eslint_config, &eslint_root, timeout)
        }))
    } else {
        None
    };

    let mut findings = ts_handle.join().unwrap()?;
    if let Some(handle) = eslint_handle {
        findings.extend(handle.join().unwrap()?);
    }
    Ok(findings)
}
