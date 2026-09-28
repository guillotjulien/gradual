use super::{analyze, describe};
use crate::events::diff::{KeyDelta, concurrent_fix_hint, diff, new_count};
use crate::events::types::{DeltaEvent, EVENT_VERSION};
use crate::events::writer::write_delta_event;
use crate::git::{get_current_sha, get_parent_sha};
use std::io::{BufRead, IsTerminal};
use std::time::Duration;

pub fn run(force: bool, yes: bool, timeout: Option<Duration>) -> anyhow::Result<()> {
    let result = analyze(timeout)?;

    let deltas = diff(&result.current, &result.baseline);
    let added = new_count(&deltas);

    if added > 0 && !force {
        for line in describe(&result.repo_root, &deltas) {
            eprintln!("{line}");
        }
        eprintln!();
        if let Some(hint) = concurrent_fix_hint(&deltas, &result.repeatedly_fixed) {
            eprintln!("{hint}");
            eprintln!();
        }
        eprintln!("{added} new finding(s). Run with --force to accept regressions.");
        std::process::exit(1);
    }

    if added > 0 {
        eprintln!("⚠ Accepting {added} new finding(s):");
        for line in describe(&result.repo_root, &deltas) {
            eprintln!("  {line}");
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

    if deltas.is_empty() {
        println!("✓ Baseline is already up to date. Nothing to record.");
        return Ok(());
    }

    let commit = get_current_sha(&result.repo_root).unwrap_or_else(|_| "unknown".to_string());
    let parent = get_parent_sha(&result.repo_root).unwrap_or_else(|_| "unknown".to_string());

    let event = DeltaEvent {
        version: EVENT_VERSION,
        commit,
        parent,
        timestamp: chrono::Utc::now().to_rfc3339(),
        changes: deltas.iter().map(KeyDelta::change).collect(),
    };

    let path = write_delta_event(&result.events_dir, &event)?;
    println!("Written: {}", path.display());
    println!("Stage and commit this file to record the baseline update.");
    Ok(())
}
