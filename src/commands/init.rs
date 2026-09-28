use super::build_findings;
use crate::analyzers;
use crate::config::GradualConfig;
use crate::events::diff::{KeyDelta, diff, group_by_id};
use crate::events::reader::count_event_files;
use crate::events::types::{DeltaEvent, EVENT_VERSION};
use crate::events::writer::write_delta_event;
use crate::git::{find_repo_root, get_current_sha, get_parent_sha};
use std::collections::{HashMap, HashSet};

pub fn run() -> anyhow::Result<()> {
    let repo_root = find_repo_root()?;
    let config = GradualConfig::load(&repo_root)?;
    let events_dir = repo_root.join(&config.events_dir);

    let existing = count_event_files(&events_dir);
    if existing > 0 {
        anyhow::bail!(
            "Already initialized ({existing} event file(s) found in {}).\n\
             Run `gradual update` to record changes, or delete the directory to start over.",
            events_dir.display()
        );
    }

    let raw_findings = analyzers::run_all(&config, &repo_root, None)?;
    let filter = config.path_filter()?;

    let genesis_findings = build_findings(&raw_findings, &repo_root, &filter);

    let file_count: HashSet<&str> = genesis_findings.iter().map(|f| f.file.as_str()).collect();

    let commit = get_current_sha(&repo_root).unwrap_or_else(|_| "initial".to_string());
    let parent = get_parent_sha(&repo_root).unwrap_or_else(|_| "none".to_string());

    let event = DeltaEvent {
        version: EVENT_VERSION,
        commit,
        parent,
        timestamp: chrono::Utc::now().to_rfc3339(),
        changes: diff(&group_by_id(genesis_findings.clone()), &HashMap::new())
            .iter()
            .map(KeyDelta::change)
            .collect(),
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
