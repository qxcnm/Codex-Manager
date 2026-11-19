//! Codex-compatible defaults for catalogs discovered without capability metadata.

// Match model_info_from_slug in Codex 0.169.0-alpha.11.1. A provider's explicit
// context and input modalities always take precedence over these defaults.
pub(crate) const DEFAULT_CONTEXT_WINDOW: i64 = 272_000;
