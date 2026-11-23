//! Resolve capabilities once for both the management editor and the Codex catalog.
//!
//! Resolution is a view, not a migration: explicit overrides (including empty
//! arrays and false flags) stay intact, and simply reading a model never writes it.
use codexmanager_core::storage::{ManagedModelV2, ResolvedModelCapabilitiesV2};
use serde_json::Value;

use super::{codex_capabilities, discovery_metadata, official_capabilities};

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn source(stored: bool, official: bool, official_source: &str) -> String {
    if stored {
        "stored"
    } else if official {
        official_source
    } else {
        "codex_default"
    }
    .into()
}

pub(super) fn resolve(model: &ManagedModelV2) -> (ManagedModelV2, ResolvedModelCapabilitiesV2) {
    resolve_with_catalog(model, &official_capabilities::Catalog::load(None))
}

pub(super) fn resolve_with_catalog(
    model: &ManagedModelV2,
    catalog: &official_capabilities::Catalog,
) -> (ManagedModelV2, ResolvedModelCapabilitiesV2) {
    let official = super::supports_text_generation(model)
        .then(|| catalog.lookup(&model.slug))
        .flatten();
    resolve_with_official(model, official)
}

fn resolve_with_official(
    model: &ManagedModelV2,
    official: Option<official_capabilities::OfficialCapabilities>,
) -> (ManagedModelV2, ResolvedModelCapabilitiesV2) {
    let mut effective = model.clone();
    effective.resolved_capabilities = None;
    let stored = discovery_metadata::sanitize_metadata(&model.capabilities)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let text_model = super::supports_text_generation(model);
    let official_source = official
        .as_ref()
        .map(|item| item.source)
        .unwrap_or("codex_default");
    let defaults = official
        .as_ref()
        .and_then(|item| item.metadata.as_object())
        .cloned()
        .unwrap_or_default();

    let stored_reasoning =
        stored.contains_key("reasoning_efforts") || model.default_reasoning_effort.is_some();
    let stored_input = stored.contains_key("input_modalities");
    let stored_context = model.context_window.is_some_and(|value| value > 0)
        || model.max_context_window.is_some_and(|value| value > 0);
    let reasoning_source = source(
        stored_reasoning,
        defaults.contains_key("reasoning_efforts")
            || defaults.contains_key("default_reasoning_level"),
        official_source,
    );
    let input_source = source(
        stored_input,
        defaults.contains_key("input_modalities"),
        official_source,
    );
    let context_source = source(
        stored_context,
        defaults.contains_key("context_window") || defaults.contains_key("max_context_window"),
        official_source,
    );

    let mut capabilities = effective
        .capabilities
        .as_object()
        .cloned()
        .unwrap_or_default();
    // Canonicalize only in this derived view. This also makes snake/camel
    // precedence identical for the editor, API selectors, and exported catalog.
    capabilities.extend(stored.clone());
    for (key, value) in &defaults {
        if !matches!(
            key.as_str(),
            "context_window" | "max_context_window" | "default_reasoning_level"
        ) && !stored.contains_key(key)
        {
            capabilities.insert(key.clone(), value.clone());
        }
    }
    if !stored_input && !defaults.contains_key("input_modalities")
        || !capabilities
            .get("input_modalities")
            .is_some_and(Value::is_array)
    {
        capabilities.insert(
            "input_modalities".into(),
            serde_json::json!(["text", "image"]),
        );
    }
    effective.capabilities = Value::Object(capabilities);
    effective.context_window = model
        .context_window
        .filter(|value| *value > 0)
        .or(model.max_context_window.filter(|value| *value > 0))
        .or_else(|| defaults.get("context_window").and_then(Value::as_i64))
        .or_else(|| defaults.get("max_context_window").and_then(Value::as_i64))
        .or_else(|| text_model.then_some(codex_capabilities::DEFAULT_CONTEXT_WINDOW));
    effective.max_context_window = model
        .max_context_window
        .filter(|value| *value > 0)
        .or(model.context_window.filter(|value| *value > 0))
        .or_else(|| defaults.get("max_context_window").and_then(Value::as_i64))
        .or(effective.context_window);
    if !stored_reasoning {
        effective.default_reasoning_effort = defaults
            .get("default_reasoning_level")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    let overall_source: String = [
        reasoning_source.as_str(),
        context_source.as_str(),
        input_source.as_str(),
    ]
    .into_iter()
    .find(|item| item.starts_with("official_"))
    .unwrap_or_else(|| {
        if stored_reasoning || stored_input || stored_context {
            "stored"
        } else {
            "codex_default"
        }
    })
    .into();
    let resolved = ResolvedModelCapabilitiesV2 {
        source: overall_source,
        matched_slug: official.map(|item| item.matched_slug),
        context_window: effective.context_window,
        max_context_window: effective.max_context_window,
        default_reasoning_effort: effective.default_reasoning_effort.clone(),
        reasoning_efforts: strings(effective.capabilities.get("reasoning_efforts")),
        input_modalities: strings(effective.capabilities.get("input_modalities")),
        reasoning_source,
        context_source,
        input_source,
    };
    (effective, resolved)
}

pub(super) fn decorate(mut model: ManagedModelV2) -> ManagedModelV2 {
    model.resolved_capabilities = Some(resolve(&model).1);
    model
}

pub(super) fn decorate_with_storage(
    mut model: ManagedModelV2,
    storage: &codexmanager_core::storage::Storage,
) -> ManagedModelV2 {
    let catalog = official_capabilities::Catalog::load(Some(storage));
    model.resolved_capabilities = Some(resolve_with_catalog(&model, &catalog).1);
    model
}

pub(super) fn decorate_models(
    models: Vec<ManagedModelV2>,
    storage: Option<&codexmanager_core::storage::Storage>,
) -> Vec<ManagedModelV2> {
    let catalog = official_capabilities::Catalog::load(storage);
    models
        .into_iter()
        .map(|mut model| {
            model.resolved_capabilities = Some(resolve_with_catalog(&model, &catalog).1);
            model
        })
        .collect()
}

pub(super) fn decorate_list(
    mut result: super::ManagedModelListV2Result,
) -> super::ManagedModelListV2Result {
    result.items = decorate_models(result.items, None);
    result
}

pub(super) fn decorate_list_with_storage(
    mut result: super::ManagedModelListV2Result,
    storage: &codexmanager_core::storage::Storage,
) -> super::ManagedModelListV2Result {
    result.items = decorate_models(result.items, Some(storage));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Pure resolution tests must not initialize the process-global storage or
    // read the developer's Codex profile while other database tests are running.
    fn resolve(model: &ManagedModelV2) -> (ManagedModelV2, ResolvedModelCapabilitiesV2) {
        resolve_with_catalog(model, &official_capabilities::Catalog::default())
    }

    fn decorate(mut model: ManagedModelV2) -> ManagedModelV2 {
        model.resolved_capabilities = Some(resolve(&model).1);
        model
    }

    fn model(slug: &str) -> ManagedModelV2 {
        ManagedModelV2 {
            slug: slug.into(),
            capabilities: json!({"supports_text_generation":true,"output_modalities":["text"]}),
            ..Default::default()
        }
    }

    #[test]
    fn known_model_without_metadata_inherits_official_capabilities_without_mutating_raw() {
        let raw = model("gpt-6.1-sol");
        let decorated = decorate(raw.clone());
        let resolved = decorated.resolved_capabilities.unwrap();
        assert_eq!(resolved.reasoning_source, "official_builtin");
        assert_eq!(resolved.default_reasoning_effort.as_deref(), Some("low"));
        assert!(resolved
            .reasoning_efforts
            .iter()
            .any(|effort| effort == "high"));
        assert_eq!(resolved.context_window, Some(272000));
        assert_eq!(resolved.input_modalities, ["text", "image"]);
        assert!(decorated.default_reasoning_effort.is_none());
        assert!(decorated.context_window.is_none());
        assert_eq!(decorated.capabilities, raw.capabilities);
        let info = super::super::model_info(&raw);
        assert_eq!(
            info.default_reasoning_level,
            resolved.default_reasoning_effort
        );
        assert_eq!(
            info.supported_reasoning_levels
                .iter()
                .map(|level| level.effort.clone())
                .collect::<Vec<_>>(),
            resolved.reasoning_efforts
        );
    }

    #[test]
    fn unknown_model_keeps_reasoning_unknown_instead_of_guessing_from_another_version() {
        let (effective, resolved) = resolve(&model("gpt-6.2-sol"));
        assert!(resolved.matched_slug.is_none());
        assert_eq!(resolved.reasoning_source, "codex_default");
        assert!(resolved.reasoning_efforts.is_empty());
        assert!(resolved.default_reasoning_effort.is_none());
        assert_eq!(effective.context_window, Some(272000));
        assert_eq!(resolved.input_modalities, ["text", "image"]);
    }

    #[test]
    fn explicit_empty_reasoning_and_text_only_and_small_context_are_not_replaced() {
        let mut raw = model("gpt-6.1-sol");
        raw.context_window = Some(272);
        raw.capabilities = json!({"reasoning_efforts":[],"input_modalities":["text"],"supports_parallel_tool_calls":false});
        let (effective, resolved) = resolve(&raw);
        assert!(resolved.reasoning_efforts.is_empty());
        assert!(resolved.default_reasoning_effort.is_none());
        assert_eq!(resolved.reasoning_source, "stored");
        assert_eq!(resolved.input_modalities, ["text"]);
        assert_eq!(resolved.context_window, Some(272));
        assert_eq!(
            effective.capabilities["supports_parallel_tool_calls"],
            false
        );
    }

    #[test]
    fn manual_text_only_without_metadata_or_context_is_preserved() {
        let raw = ManagedModelV2 {
            slug: "gpt-6.1-sol".into(),
            origin: "custom".into(),
            instructions_mode: "passthrough".into(),
            price: codexmanager_core::storage::ModelPriceV2 {
                price_status: "missing".into(),
                ..Default::default()
            },
            routes: vec![codexmanager_core::storage::ModelRouteV2 {
                source_kind: "account_pool".into(),
                source_id: "default".into(),
                upstream_model: "gpt-6.1-sol".into(),
                enabled: true,
                weight: 1,
                ..Default::default()
            }],
            capabilities: json!({
                "supports_text_generation": true,
                "input_modalities": ["text"],
                "output_modalities": ["text"]
            }),
            ..Default::default()
        };
        let (effective, resolved) = resolve(&raw);
        assert_eq!(resolved.input_modalities, ["text"]);
        assert_eq!(resolved.input_source, "stored");
        assert_eq!(effective.capabilities["input_modalities"], json!(["text"]));
        assert!(raw.context_window.is_none());
        assert!(raw.max_context_window.is_none());
        assert!(raw.capabilities.get("discovery_metadata_version").is_none());
        assert_eq!(raw.capabilities["input_modalities"], json!(["text"]));
    }

    #[test]
    fn explicit_high_default_and_snake_alias_take_priority() {
        let mut raw = model("gpt-6.1-sol");
        raw.default_reasoning_effort = Some("high".into());
        raw.capabilities = json!({"reasoning_efforts":["high"],"reasoningEfforts":["low"]});
        let (_, resolved) = resolve(&raw);
        assert_eq!(resolved.reasoning_efforts, ["high"]);
        assert_eq!(resolved.default_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(resolved.reasoning_source, "stored");
    }

    #[test]
    fn setting_only_the_default_does_not_drop_inherited_reasoning_levels() {
        let mut raw = model("gpt-6.1-sol");
        raw.default_reasoning_effort = Some("high".into());
        let (_, resolved) = resolve(&raw);
        assert_eq!(resolved.default_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            resolved.reasoning_efforts,
            ["low", "medium", "high", "xhigh", "max", "ultra"]
        );
        assert!(!raw
            .capabilities
            .as_object()
            .unwrap()
            .contains_key("reasoning_efforts"));
    }

    #[test]
    fn max_only_official_metadata_is_used_as_the_effective_context() {
        let raw = model("future-model");
        let (_, resolved) = resolve_with_official(
            &raw,
            Some(official_capabilities::OfficialCapabilities {
                metadata: json!({"max_context_window":500000}),
                source: "official_cache",
                matched_slug: "future-model".into(),
            }),
        );
        assert_eq!(resolved.context_window, Some(500000));
        assert_eq!(resolved.max_context_window, Some(500000));
        assert_eq!(resolved.context_source, "official_cache");
    }

    #[test]
    fn response_metadata_cannot_be_persisted_as_an_override() {
        use codexmanager_core::storage::{ManagedModelV2Upsert, Storage};
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let mut raw = storage
            .get_managed_model_v2("gpt-6.1-sol")
            .unwrap()
            .unwrap();
        raw.context_window = None;
        raw.max_context_window = None;
        raw.default_reasoning_effort = None;
        raw.capabilities = json!({"supports_text_generation":true});
        raw.resolved_capabilities = Some(ResolvedModelCapabilitiesV2 {
            context_window: Some(1),
            reasoning_efforts: vec!["forged".into()],
            ..Default::default()
        });
        let saved = storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                model: raw,
                ..Default::default()
            })
            .unwrap();
        assert!(saved.resolved_capabilities.is_none());
        assert!(saved.context_window.is_none());
        assert!(saved.default_reasoning_effort.is_none());
        assert_eq!(saved.capabilities, json!({"supports_text_generation":true}));
        let reread = storage
            .get_managed_model_v2("gpt-6.1-sol")
            .unwrap()
            .unwrap();
        assert!(reread.resolved_capabilities.is_none());
        let resolved = decorate(reread).resolved_capabilities.unwrap();
        assert_eq!(resolved.context_window, Some(272000));
        assert!(!resolved
            .reasoning_efforts
            .iter()
            .any(|effort| effort == "forged"));
    }
}
