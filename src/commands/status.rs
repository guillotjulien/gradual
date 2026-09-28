use crate::config::GradualConfig;
use crate::events::fold::fold;
use crate::events::reader::read_all_events;
use crate::git::find_repo_root;
use std::collections::HashMap;

pub fn run() -> anyhow::Result<()> {
    let repo_root = find_repo_root()?;
    let config = GradualConfig::load(&repo_root)?;
    let events_dir = repo_root.join(&config.events_dir);

    let events = read_all_events(&events_dir)?;
    if events.is_empty() {
        println!("No baseline found. Run `gradual init` to initialize.");
        return Ok(());
    }

    let baseline = fold(&events);

    // Count findings by rule, sorted descending.
    let mut by_rule: HashMap<&str, usize> = HashMap::new();
    for entry in baseline.values() {
        *by_rule.entry(entry.rule.as_str()).or_insert(0) += entry.count as usize;
    }
    let mut rule_counts: Vec<(&str, usize)> = by_rule.into_iter().collect();
    rule_counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let baseline_total: usize = rule_counts.iter().map(|(_, n)| n).sum();

    // How many findings from the genesis event have been fixed?
    let mut genesis_total = 0usize;
    let mut fixed_since_init = 0usize;
    for change in events[0].changes.iter().filter(|c| c.delta > 0) {
        let accepted = usize::try_from(change.delta).unwrap_or(usize::MAX);
        let remaining = baseline.get(&change.id).map_or(0, |e| e.count as usize);
        genesis_total += accepted;
        fixed_since_init += accepted.saturating_sub(remaining);
    }

    println!("Baseline: {baseline_total} finding(s)  ({} event(s))", events.len());
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
