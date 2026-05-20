use crate::events::types::{DeltaEvent, Finding};
use std::collections::HashMap;

pub fn fold(events: &[DeltaEvent]) -> HashMap<String, Finding> {
    let mut state: HashMap<String, Finding> = HashMap::new();
    for event in events {
        for id in &event.removed {
            state.remove(id);
        }
        for finding in &event.added {
            state.insert(finding.id.clone(), finding.clone());
        }
    }
    state
}
