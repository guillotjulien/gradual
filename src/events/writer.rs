use crate::events::types::DeltaEvent;
use anyhow::Context;
use std::path::{Path, PathBuf};

pub fn write_delta_event(events_dir: &Path, event: &DeltaEvent) -> anyhow::Result<PathBuf> {
    let ts = &event.timestamp;
    // timestamp must be ISO 8601: "YYYY-MM-DDTHH:MM:SSZ"
    let date = ts.get(..10).context("timestamp too short")?;
    let time = ts.get(11..19).context("timestamp missing time component")?;

    let mut parts = date.splitn(3, '-');
    let year = parts.next().context("missing year")?;
    let month = parts.next().context("missing month")?;
    let day = parts.next().context("missing day")?;

    let dir = events_dir.join(year).join(month).join(day);
    std::fs::create_dir_all(&dir).context("failed to create event directory")?;

    let time_slug = time.replace(':', "-");
    let sha_prefix = event.commit.get(..7).unwrap_or(&event.commit);
    let filename = format!("{time_slug}-{sha_prefix}.json");
    let final_path = dir.join(&filename);
    let tmp_path = dir.join(format!("{filename}.tmp"));

    let json = serde_json::to_string_pretty(event).context("failed to serialize event")?;
    std::fs::write(&tmp_path, &json).context("failed to write temp event file")?;
    std::fs::rename(&tmp_path, &final_path).context("failed to rename temp file to final")?;

    Ok(final_path)
}
