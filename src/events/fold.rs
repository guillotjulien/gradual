use crate::events::types::{DeltaEvent, Entry};
use std::collections::HashMap;

/// Sums the changes of all events into the accepted count per id. Addition is
/// commutative, so the result does not depend on event order. Ids whose total is not
/// positive are dropped (a total below zero can happen when two branches both fix the
/// same finding; it is clamped rather than treated as a credit).
pub fn fold(events: &[DeltaEvent]) -> HashMap<String, Entry> {
    let mut totals: HashMap<String, (i64, String, String)> = HashMap::new();
    for change in events.iter().flat_map(|e| &e.changes) {
        let slot = totals
            .entry(change.id.clone())
            .or_insert_with(|| (0, change.rule.clone(), change.file.clone()));
        slot.0 += change.delta;
    }
    totals
        .into_iter()
        .filter(|(_, (total, _, _))| *total > 0)
        .map(|(id, (total, rule, file))| {
            let count = u32::try_from(total).unwrap_or(u32::MAX);
            (id, Entry { count, rule, file })
        })
        .collect()
}
