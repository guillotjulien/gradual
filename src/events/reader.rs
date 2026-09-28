use crate::events::types::{DeltaEvent, EVENT_VERSION};
use std::path::Path;
use walkdir::WalkDir;

fn event_paths(events_dir: &Path) -> Vec<std::path::PathBuf> {
    if !events_dir.exists() {
        return Vec::new();
    }
    WalkDir::new(events_dir)
        .into_iter()
        .filter_map(Result::ok)
        .map(walkdir::DirEntry::into_path)
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect()
}

/// Number of event files on disk, whatever their version or validity.
pub fn count_event_files(events_dir: &Path) -> usize {
    event_paths(events_dir).len()
}

/// Reads every event, ordered by timestamp. Unreadable or malformed files are logged
/// and skipped. An event with an unsupported `version` is a hard error: skipping it
/// would silently produce a wrong baseline.
pub fn read_all_events(events_dir: &Path) -> anyhow::Result<Vec<DeltaEvent>> {
    let mut events: Vec<DeltaEvent> = Vec::new();
    for path in event_paths(events_dir) {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to read {}: {e}", path.display());
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("warning: failed to parse {}: {e}", path.display());
                continue;
            }
        };
        let version = value.get("version").and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(EVENT_VERSION)) {
            anyhow::bail!(
                "{} has unsupported baseline format version {} (expected {EVENT_VERSION}).\n\
                 The baseline format changed. To migrate, delete {} on a clean branch,\n\
                 run `gradual init`, and commit the new baseline.",
                path.display(),
                version.map_or_else(|| "<missing>".to_string(), |v| v.to_string()),
                events_dir.display()
            );
        }
        match serde_json::from_value::<DeltaEvent>(value) {
            Ok(event) => events.push(event),
            Err(e) => eprintln!("warning: failed to parse {}: {e}", path.display()),
        }
    }
    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.commit.cmp(&b.commit)));
    Ok(events)
}
