use super::analyze;
use std::time::Duration;

pub fn run(timeout: Option<Duration>) -> anyhow::Result<()> {
    let result = analyze(timeout)?;

    let mut added: Vec<_> = result
        .current
        .values()
        .filter(|f| !result.baseline.contains_key(&f.id))
        .collect();
    added.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));

    let removed_count = result
        .baseline
        .values()
        .filter(|f| !result.current.contains_key(&f.id))
        .count();

    if !added.is_empty() {
        for f in &added {
            eprintln!("{}:{}  {}  {}", f.file, f.line, f.rule, f.message);
        }
        eprintln!();
        eprintln!(
            "{} new finding(s) since baseline. Fix them or run `gradual update --force`.",
            added.len()
        );
        std::process::exit(1);
    }

    if removed_count > 0 {
        println!("✓ No regressions ({removed_count} finding(s) removed since baseline).");
    } else {
        println!("✓ No regressions.");
    }
    Ok(())
}
