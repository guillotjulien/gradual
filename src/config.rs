use serde::Deserialize;
use std::path::Path;

fn default_events_dir() -> String {
    ".gradual/events".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct GradualConfig {
    pub tsconfig: String,
    pub eslint_config: Option<String>,
    #[serde(default = "default_events_dir")]
    pub events_dir: String,
}

impl GradualConfig {
    pub fn load(repo_root: &Path) -> anyhow::Result<Self> {
        let path = repo_root.join("gradual.json");
        if !path.exists() {
            anyhow::bail!(
                "gradual.json not found in {}. Run `gradual init` to create one.",
                repo_root.display()
            );
        }
        let contents = std::fs::read_to_string(&path)?;
        let config: GradualConfig = serde_json::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("Failed to parse gradual.json: {e}"))?;
        Ok(config)
    }
}
