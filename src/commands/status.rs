use crate::config::GradualConfig;
use crate::events::fold::fold;
use crate::events::reader::read_all_events;
use crate::git::find_repo_root;
use std::collections::{HashMap, HashSet};

pub fn run() -> anyhow::Result<()> {
    let repo_root = find_repo_root()?;
    let config = GradualConfig::load(&repo_root)?;
    let events_dir = repo_root.join(&config.events_dir);

    let events = read_all_events(&events_dir);
    if events.is_empty() {
        println!("No baseline found. Run `gradual init` to initialize.");
        return Ok(());
    }

    let baseline = fold(&events);

    // Count findings by rule, sorted descending.
    let mut by_rule: HashMap<&str, usize> = HashMap::new();
    for f in baseline.values() {
        *by_rule.entry(f.rule.as_str()).or_insert(0) += 1;
    }
    let mut rule_counts: Vec<(&str, usize)> = by_rule.into_iter().collect();
    rule_counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    // How many findings from the genesis event have been fixed?
    let genesis_ids: HashSet<&str> = events[0]
        .added
        .iter()
        .map(|f| f.id.as_str())
        .collect();
    let genesis_total = genesis_ids.len();
    let still_present = baseline
        .keys()
        .filter(|id| genesis_ids.contains(id.as_str()))
        .count();
    let fixed_since_init = genesis_total.saturating_sub(still_present);

    println!("Baseline: {} finding(s)  ({} event(s))", baseline.len(), events.len());
    if let Some(pct) = (fixed_since_init * 100).checked_div(genesis_total) {
        println!("Fixed since init: {fixed_since_init} / {genesis_total} ({pct}%)");
    }

    if rule_counts.is_empty() {
        println!("\nNo findings in baseline. Well done!");
        return Ok(());
    }

    println!("\nFindings by rule:");
    for (rule, count) in &rule_counts {
        println!("  {count:5}  {rule}");
    }

    Ok(())
}
