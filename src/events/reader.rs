use crate::events::types::DeltaEvent;
use std::path::Path;
use walkdir::WalkDir;

// Intentionally returns Vec (not Result): failures in individual event files are logged
// and skipped rather than propagated, so this function always succeeds.
pub fn read_all_events(events_dir: &Path) -> Vec<DeltaEvent> {
    if !events_dir.exists() {
        return Vec::new();
    }
    let mut events: Vec<DeltaEvent> = Vec::new();
    for entry in WalkDir::new(events_dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to read {}: {e}", path.display());
                continue;
            }
        };
        let event: DeltaEvent = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("warning: failed to parse {}: {e}", path.display());
                continue;
            }
        };
        if event.version != 1 {
            eprintln!(
                "warning: skipping {} with unknown version {}",
                path.display(),
                event.version
            );
            continue;
        }
        events.push(event);
    }
    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.commit.cmp(&b.commit)));
    events
}
