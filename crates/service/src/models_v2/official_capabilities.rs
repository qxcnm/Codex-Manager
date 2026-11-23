//! Safe capability metadata from Codex's local cache and a pinned bundled catalog.
use super::discovery_metadata::sanitize_metadata;
use codexmanager_core::storage::Storage;
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_CATALOG_BYTES: u64 = 8 * 1024 * 1024;
const BUILTIN_CATALOG: &str = include_str!("official_model_capabilities.json");

#[derive(Debug, Clone)]
pub(super) struct OfficialCapabilities {
    pub(super) metadata: Value,
    pub(super) source: &'static str,
    pub(super) matched_slug: String,
}

/// One operation's local snapshot. Resolving every model against this value
/// avoids repeated settings queries and parsing, and keeps the operation's view
/// consistent if Codex replaces its cache while a catalog is being rendered.
#[derive(Default)]
pub(super) struct Catalog {
    cached: Option<Value>,
}

impl Catalog {
    pub(super) fn load(storage: Option<&Storage>) -> Self {
        let home = match storage {
            Some(storage) if !crate::storage_helpers::seaorm_enabled() => {
                sqlite_profile_dir(storage)
            }
            _ => crate::codex_profile::resolve_profile_dir(None).ok(),
        };
        home.map(|home| Self::load_with_path(&home.join("models_cache.json")))
            .unwrap_or_default()
    }

    fn load_with_path(path: &Path) -> Self {
        Self {
            cached: File::open(path).ok().and_then(read_catalog),
        }
    }

    pub(super) fn lookup(&self, slug: &str) -> Option<OfficialCapabilities> {
        let slug = slug.trim();
        if slug.is_empty() {
            return None;
        }
        let cached = self
            .cached
            .as_ref()
            .and_then(|catalog| lookup_in_catalog(catalog, slug, "official_cache"));
        cached.or_else(|| {
            static BUILTIN: OnceLock<Option<Value>> = OnceLock::new();
            BUILTIN
                .get_or_init(|| serde_json::from_str(BUILTIN_CATALOG).ok())
                .as_ref()
                .and_then(|catalog| lookup_in_catalog(catalog, slug, "official_builtin"))
        })
    }
}

fn sqlite_profile_dir(storage: &Storage) -> Option<PathBuf> {
    // The caller already owns a SQLite handle. Resolving with None here would
    // acquire another pooled handle and can exhaust a one-connection pool.
    let stored_home = storage
        .get_app_setting("codex_profile.codex_home")
        .ok()
        .flatten()
        .filter(|home| !home.trim().is_empty());
    let explicit_home = stored_home.or_else(|| {
        std::env::var("CODEX_HOME")
            .ok()
            .filter(|home| !home.trim().is_empty())
    });
    match explicit_home {
        Some(home) => crate::codex_profile::resolve_profile_dir(Some(&home)).ok(),
        None => crate::codex_profile::user_home_dir()
            .ok()
            .map(|home| home.join(".codex")),
    }
}

fn read_catalog(reader: impl Read) -> Option<Value> {
    let mut contents = Vec::new();
    reader
        .take(MAX_CATALOG_BYTES + 1)
        .read_to_end(&mut contents)
        .ok()?;
    if contents.len() as u64 > MAX_CATALOG_BYTES {
        return None;
    }
    serde_json::from_slice(&contents).ok()
}

fn longest_prefix<'a>(models: &'a [Value], slug: &str) -> Option<&'a Value> {
    let mut best: Option<(&Value, usize)> = None;
    for model in models {
        let Some(candidate) = model.get("slug").and_then(Value::as_str) else {
            continue;
        };
        // Empty cache entries must not become a universal prefix match.
        if candidate.is_empty() || !slug.starts_with(candidate) {
            continue;
        }
        if best.is_none_or(|(_, length)| candidate.len() > length) {
            best = Some((model, candidate.len()));
        }
    }
    best.map(|(model, _)| model)
}

fn lookup_in_catalog(
    catalog: &Value,
    slug: &str,
    source: &'static str,
) -> Option<OfficialCapabilities> {
    let models = catalog.get("models")?.as_array()?;
    // Match Codex's longest-prefix lookup first, then its single simple namespace
    // retry. In particular, this never replaces an unknown 6.2 model with 6.1.
    let matched = longest_prefix(models, slug).or_else(|| {
        let (namespace, suffix) = slug.split_once('/')?;
        if namespace.is_empty()
            || suffix.contains('/')
            || !namespace
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        longest_prefix(models, suffix)
    })?;
    let matched_slug = matched.get("slug")?.as_str()?.to_string();
    // The bundled snapshot is pre-filtered. Local cache entries are always
    // filtered at this boundary as well; prompts and arbitrary JSON never escape.
    let metadata = sanitize_metadata(matched.get("metadata").unwrap_or(matched))?;
    Some(OfficialCapabilities {
        metadata,
        source,
        matched_slug,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct CacheFile(PathBuf);

    impl CacheFile {
        fn new(contents: &[u8]) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "codexmanager-official-capabilities-{}-{nonce}.json",
                std::process::id()
            ));
            std::fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for CacheFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn builtin(slug: &str) -> Option<OfficialCapabilities> {
        let catalog: Value = serde_json::from_str(BUILTIN_CATALOG).unwrap();
        lookup_in_catalog(&catalog, slug, "official_builtin")
    }

    #[test]
    fn local_snapshot_is_reused_until_the_next_operation() {
        let file =
            CacheFile::new(br#"{"models":[{"slug":"gpt-6.2-sol","context_window":500000}]}"#);
        let snapshot = Catalog::load_with_path(&file.0);
        std::fs::write(
            &file.0,
            br#"{"models":[{"slug":"gpt-6.2-sol","context_window":900000}]}"#,
        )
        .unwrap();
        for _ in 0..2 {
            let found = snapshot.lookup("gpt-6.2-sol").unwrap();
            assert_eq!(found.source, "official_cache");
            assert_eq!(found.metadata["context_window"], 500000);
        }
        let next = Catalog::load_with_path(&file.0);
        assert_eq!(
            next.lookup("gpt-6.2-sol").unwrap().metadata["context_window"],
            900000
        );
        assert_eq!(
            snapshot.lookup("gpt-6.1-sol").unwrap().source,
            "official_builtin"
        );
        assert!(snapshot.lookup(" ").is_none());
    }

    #[test]
    fn invalid_local_snapshot_keeps_the_builtin_fallback() {
        let file = CacheFile::new(b"invalid json");
        let snapshot = Catalog::load_with_path(&file.0);
        assert_eq!(
            snapshot.lookup("gpt-6.1-sol").unwrap().source,
            "official_builtin"
        );
        assert!(snapshot.lookup("gpt-6.2-sol").is_none());
    }

    #[test]
    fn sqlite_profile_home_comes_from_the_supplied_storage_handle() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let expected = std::env::temp_dir().join("codexmanager-explicit-codex-home");
        storage
            .set_app_setting(
                "codex_profile.codex_home",
                &format!("  {}  ", expected.display()),
                1,
            )
            .unwrap();
        assert_eq!(sqlite_profile_dir(&storage), Some(expected));
    }

    #[test]
    fn pinned_official_builtin_preserves_gpt61_capabilities() {
        let found = builtin("gpt-6.1-sol").unwrap();
        assert_eq!(found.source, "official_builtin");
        assert_eq!(found.matched_slug, "gpt-6.1-sol");
        assert_eq!(found.metadata["context_window"], 272_000);
        assert_eq!(found.metadata["max_context_window"], 872_000);
        assert_eq!(found.metadata["default_reasoning_level"], "low");
        assert_eq!(
            found.metadata["reasoning_efforts"],
            json!(["low", "medium", "high", "xhigh", "max", "ultra"])
        );
        assert_eq!(found.metadata["input_modalities"], json!(["text", "image"]));
    }

    #[test]
    fn bundled_snapshot_contains_only_sanitized_capabilities() {
        let catalog: Value = serde_json::from_str(BUILTIN_CATALOG).unwrap();
        assert_eq!(catalog["codex_version"], "0.169.0-alpha.11.1");
        for model in catalog["models"].as_array().unwrap() {
            assert_eq!(
                sanitize_metadata(&model["metadata"]).as_ref(),
                Some(&model["metadata"]),
                "unsanitized model {}",
                model["slug"]
            );
        }
    }

    #[test]
    fn namespaced_dated_slugs_use_the_official_longest_prefix() {
        let found = builtin("provider/gpt-6.1-sol-2026-11-18").unwrap();
        assert_eq!(found.matched_slug, "gpt-6.1-sol");
        assert_eq!(found.metadata["default_reasoning_level"], "low");
        assert!(builtin("provider/nested/gpt-6.1-sol").is_none());
        assert!(builtin("bad.namespace/gpt-6.1-sol").is_none());

        let catalog = json!({"models":[
            {"slug":"gpt-test", "context_window":128000},
            {"slug":"gpt-test-plus", "context_window":256000}
        ]});
        let found = lookup_in_catalog(&catalog, "gpt-test-plus-2026", "official_cache").unwrap();
        assert_eq!(found.matched_slug, "gpt-test-plus");
        assert_eq!(found.metadata["context_window"], 256000);
    }

    #[test]
    fn unknown_gpt62_does_not_inherit_gpt61_metadata() {
        assert!(builtin("gpt-6.2-sol").is_none());
        assert!(builtin("provider/gpt-6.2-sol-2026-11-18").is_none());
        assert!(builtin("gpt-6.2-astra").is_none());
    }

    #[test]
    fn local_cache_metadata_filters_prompts_secrets_and_invalid_modalities() {
        let catalog = json!({"models":[{
            "slug":"gpt-6.2-sol", "default_reasoning_level":"high",
            "supported_reasoning_levels":[{"effort":"high","description":"High"}],
            "context_window":272000,"input_modalities":["text","image","video"],
            "base_instructions":"must not escape", "instructions":"must not escape",
            "model_messages":{"instructions_template":"must not escape"},
            "api_key":"must not escape", "access_token":"must not escape",
            "use_responses_lite":true
        }]});
        let found = lookup_in_catalog(&catalog, "gpt-6.2-sol", "official_cache").unwrap();
        assert_eq!(found.source, "official_cache");
        assert_eq!(found.matched_slug, "gpt-6.2-sol");
        assert_eq!(found.metadata["default_reasoning_level"], "high");
        assert_eq!(found.metadata["reasoning_efforts"], json!(["high"]));
        assert_eq!(found.metadata["input_modalities"], json!(["text", "image"]));
        for key in [
            "base_instructions",
            "instructions",
            "model_messages",
            "api_key",
            "access_token",
            "use_responses_lite",
        ] {
            assert!(found.metadata.get(key).is_none(), "unexpected field {key}");
        }
    }

    #[test]
    fn oversized_or_invalid_local_catalogs_are_ignored() {
        assert!(read_catalog(std::io::repeat(b' ').take(MAX_CATALOG_BYTES + 1)).is_none());
        assert!(read_catalog(&b"invalid json"[..]).is_none());
        assert!(lookup_in_catalog(
            &json!({"models":[{"slug":"", "context_window":1}]}),
            "gpt-6.2-sol",
            "official_cache"
        )
        .is_none());
    }
}
