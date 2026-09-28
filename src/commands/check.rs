use super::{analyze, describe};
use crate::events::diff::{concurrent_fix_hint, diff, new_count, removed_count};
use std::time::Duration;

pub fn run(timeout: Option<Duration>) -> anyhow::Result<()> {
    let result = analyze(timeout)?;
    let deltas = diff(&result.current, &result.baseline);

    let added = new_count(&deltas);
    if added > 0 {
        for line in describe(&result.repo_root, &deltas) {
            eprintln!("{line}");
        }
        eprintln!();
        if let Some(hint) = concurrent_fix_hint(&deltas, &result.repeatedly_fixed) {
            eprintln!("{hint}");
            eprintln!();
        }
        eprintln!("{added} new finding(s) since baseline. Fix them or run `gradual update --force`.");
        std::process::exit(1);
    }

    let removed = removed_count(&deltas);
    if removed > 0 {
        println!("✓ No regressions ({removed} finding(s) removed since baseline).");
    } else {
        println!("✓ No regressions.");
    }
    Ok(())
}
