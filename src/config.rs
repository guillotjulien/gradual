use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
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
    /// Glob patterns (repo-relative, forward-slash) for the files to track. When
    /// empty, every file is tracked. E.g. `["src/**/*.ts"]`.
    #[serde(default)]
    pub include: Vec<String>,
    /// Glob patterns to drop even if they match `include`. `node_modules` is always
    /// excluded. E.g. `["**/*.spec.ts"]`.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// `node_modules` findings are never the user's code, so they are always excluded
/// regardless of configuration.
const DEFAULT_EXCLUDES: &[&str] = &["**/node_modules/**", "node_modules/**"];

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

    /// Builds the include/exclude matcher used to keep only findings under the
    /// configured paths.
    pub fn path_filter(&self) -> anyhow::Result<PathFilter> {
        let include = if self.include.is_empty() {
            None
        } else {
            Some(build_set(self.include.iter().map(String::as_str))?)
        };
        let exclude = build_set(
            DEFAULT_EXCLUDES
                .iter()
                .copied()
                .chain(self.exclude.iter().map(String::as_str)),
        )?;
        Ok(PathFilter { include, exclude })
    }
}

/// Decides whether a finding's repo-relative path should be tracked.
pub struct PathFilter {
    /// `None` means "match everything" (no `include` configured).
    include: Option<GlobSet>,
    exclude: GlobSet,
}

impl PathFilter {
    /// A finding is kept when it matches `include` (or `include` is unset) and does
    /// not match any `exclude` pattern. `rel_path` must be repo-relative with
    /// forward slashes.
    pub fn matches(&self, rel_path: &str) -> bool {
        let included = self.include.as_ref().is_none_or(|g| g.is_match(rel_path));
        included && !self.exclude.is_match(rel_path)
    }
}

fn build_set<'a>(patterns: impl IntoIterator<Item = &'a str>) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        let pat = pat.trim_start_matches("./");
        // `literal_separator(true)` makes `*` stop at `/`, so `**` is required to
        // cross directories — matching the usual glob expectation (`src/**/*.ts`).
        let glob = GlobBuilder::new(pat)
            .literal_separator(true)
            .build()
            .map_err(|e| anyhow::anyhow!("invalid glob pattern {pat:?}: {e}"))?;
        builder.add(glob);
    }
    Ok(builder.build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(include: &[&str], exclude: &[&str]) -> PathFilter {
        GradualConfig {
            tsconfig: "tsconfig.json".into(),
            eslint_config: None,
            events_dir: default_events_dir(),
            include: include.iter().map(|s| (*s).to_string()).collect(),
            exclude: exclude.iter().map(|s| (*s).to_string()).collect(),
        }
        .path_filter()
        .unwrap()
    }

    #[test]
    fn node_modules_always_excluded() {
        let f = filter(&[], &[]);
        assert!(!f.matches("node_modules/semver/index.d.ts"));
        assert!(!f.matches("src/deep/node_modules/x.ts"));
        assert!(f.matches("src/app.ts"));
    }

    #[test]
    fn include_restricts_to_src() {
        let f = filter(&["src/**/*.ts"], &[]);
        assert!(f.matches("src/app.ts"));
        assert!(f.matches("src/a/b/c.ts"));
        assert!(!f.matches("test/app.ts"));
        assert!(!f.matches("src/app.js"));
    }

    #[test]
    fn exclude_spec_files() {
        let f = filter(&["src/**/*.ts"], &["**/*.spec.ts"]);
        assert!(f.matches("src/app.ts"));
        assert!(!f.matches("src/app.spec.ts"));
        assert!(!f.matches("src/nested/thing.spec.ts"));
    }

    #[test]
    fn leading_dot_slash_is_normalized() {
        let f = filter(&["./src/**/*.ts"], &[]);
        assert!(f.matches("src/app.ts"));
    }
}
