use super::raw_to_finding;
use crate::analyzers;
use crate::config::GradualConfig;
use crate::events::reader::read_all_events;
use crate::events::types::DeltaEvent;
use crate::events::writer::write_delta_event;
use crate::git::{find_repo_root, get_current_sha, get_parent_sha};
use crate::identity::parser::ParseCache;
use std::collections::HashSet;

pub fn run() -> anyhow::Result<()> {
    let repo_root = find_repo_root()?;
    let config = GradualConfig::load(&repo_root)?;
    let events_dir = repo_root.join(&config.events_dir);

    let existing = read_all_events(&events_dir);
    if !existing.is_empty() {
        anyhow::bail!(
            "Already initialized ({} event(s) found in {}).\n\
             Run `gradual update` to record changes.",
            existing.len(),
            events_dir.display()
        );
    }

    let raw_findings = analyzers::run_all(&config, &repo_root, None)?;

    let mut cache = ParseCache::new();
    let mut genesis_findings = Vec::new();
    for raw in &raw_findings {
        match raw_to_finding(raw, &repo_root, &mut cache) {
            Ok(f) => genesis_findings.push(f),
            Err(e) => eprintln!(
                "warning: skipping finding at {}:{}: {:#}",
                raw.file.display(),
                raw.line,
                e
            ),
        }
    }

    let file_count: HashSet<&str> = genesis_findings.iter().map(|f| f.file.as_str()).collect();

    let commit = get_current_sha(&repo_root).unwrap_or_else(|_| "initial".to_string());
    let parent = get_parent_sha(&repo_root).unwrap_or_else(|_| "none".to_string());

    let event = DeltaEvent {
        version: 1,
        commit,
        parent,
        timestamp: chrono::Utc::now().to_rfc3339(),
        added: genesis_findings.clone(),
        removed: vec![],
    };

    write_delta_event(&events_dir, &event)?;

    let gradual_dir = repo_root.join(".gradual");
    std::fs::create_dir_all(&gradual_dir)?;
    let gitignore = gradual_dir.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, "cache/\n")?;
    }

    println!(
        "Initialized. Found {} finding(s) across {} file(s).",
        genesis_findings.len(),
        file_count.len()
    );
    println!();
    println!("Next steps:");
    println!("  git add .gradual/");
    println!("  git commit -m \"chore: initialize gradual baseline\"");
    println!("  gradual install-hook");
    Ok(())
}
