use anyhow::Context;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

pub struct ParseCache {
    cache: HashMap<(PathBuf, u64), tree_sitter::Tree>,
}

impl Default for ParseCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ParseCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn parse(&mut self, path: &Path) -> anyhow::Result<&tree_sitter::Tree> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let content_hash = xxh3_64(source.as_bytes());
        let key = (path.to_path_buf(), content_hash);

        if !self.cache.contains_key(&key) {
            let language = language_for_path(path)?;
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&language)
                .context("Failed to set tree-sitter language")?;
            let tree = parser
                .parse(&source, None)
                .context("tree-sitter failed to parse file")?;
            self.cache.insert(key.clone(), tree);
        }

        Ok(self.cache.get(&key).unwrap())
    }
}

fn language_for_path(path: &Path) -> anyhow::Result<tree_sitter::Language> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ts" | "js") => Ok(tree_sitter_typescript::language_typescript()),
        Some("tsx") => Ok(tree_sitter_typescript::language_tsx()),
        other => anyhow::bail!(
            "Unsupported file extension {:?} for {}",
            other,
            path.display()
        ),
    }
}
