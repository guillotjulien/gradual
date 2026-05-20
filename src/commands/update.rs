use super::analyze;
use crate::events::types::DeltaEvent;
use crate::events::writer::write_delta_event;
use crate::git::{get_current_sha, get_parent_sha};
use std::io::{BufRead, IsTerminal};
use std::time::Duration;

pub fn run(force: bool, yes: bool, timeout: Option<Duration>) -> anyhow::Result<()> {
    let result = analyze(timeout)?;

    let mut added: Vec<_> = result
        .current
        .values()
        .filter(|f| !result.baseline.contains_key(&f.id))
        .cloned()
        .collect();
    added.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

    let mut removed: Vec<String> = result
        .baseline
        .keys()
        .filter(|id| !result.current.contains_key(*id))
        .cloned()
        .collect();
    removed.sort();

    if !added.is_empty() && !force {
        for f in &added {
            eprintln!("{}:{}  {}  {}", f.file, f.line, f.rule, f.message);
        }
        eprintln!();
        eprintln!(
            "{} new finding(s). Run with --force to accept regressions.",
            added.len()
        );
        std::process::exit(1);
    }

    if !added.is_empty() {
        eprintln!("⚠ Accepting {} new finding(s):", added.len());
        for f in &added {
            eprintln!("  {}:{}  {}  {}", f.file, f.line, f.rule, f.message);
        }
        eprintln!();

        if std::io::stdin().is_terminal() {
            eprint!("Are you sure? [y/N]: ");
            let mut input = String::new();
            std::io::stdin().lock().read_line(&mut input)?;
            if !matches!(input.trim(), "y" | "Y") {
                anyhow::bail!("Aborted.");
            }
        } else if !yes {
            anyhow::bail!("--yes required to accept regressions in non-TTY mode.");
        }
    }

    if added.is_empty() && removed.is_empty() {
        println!("✓ Baseline is already up to date. Nothing to record.");
        return Ok(());
    }

    let commit = get_current_sha(&result.repo_root).unwrap_or_else(|_| "unknown".to_string());
    let parent = get_parent_sha(&result.repo_root).unwrap_or_else(|_| "unknown".to_string());

    let event = DeltaEvent {
        version: 1,
        commit,
        parent,
        timestamp: chrono::Utc::now().to_rfc3339(),
        added,
        removed,
    };

    let path = write_delta_event(&result.events_dir, &event)?;
    println!("Written: {}", path.display());
    println!("Stage and commit this file to record the baseline update.");
    Ok(())
}
