//! Temporary, offline Cargo helper for refresh-codex-capabilities.py.
mod sanitizer;

use sanitizer::sanitize_metadata;
use serde_json::{json, Value};
use std::{collections::HashSet, io};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog: Value = serde_json::from_reader(io::stdin().lock())?;
    let entries = catalog["models"]
        .as_array()
        .filter(|entries| !entries.is_empty())
        .ok_or("missing or empty models array")?;
    let mut seen = HashSet::new();
    let mut models = Vec::with_capacity(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        let slug = entry["slug"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 200
                    && s.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c))
            })
            .ok_or_else(|| format!("invalid slug at models[{i}]"))?;
        if !seen.insert(slug) {
            return Err(format!("duplicate slug at models[{i}]").into());
        }
        let metadata = sanitize_metadata(entry)
            .ok_or_else(|| format!("no recognized metadata at models[{i}]"))?;
        if sanitize_metadata(&metadata).as_ref() != Some(&metadata) {
            return Err(format!("non-idempotent metadata at models[{i}]").into());
        }
        models.push(json!({"slug": slug, "metadata": metadata}));
    }
    serde_json::to_writer(io::stdout().lock(), &models)?;
    Ok(())
}
