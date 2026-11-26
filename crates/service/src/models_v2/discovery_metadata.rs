//! Carry model capabilities through discovery without importing upstream prompts or secrets.
use codexmanager_core::storage::ManagedModelV2;
use serde_json::{Map, Value};

mod sanitizer;
pub(crate) use sanitizer::sanitize_metadata;

fn apply_metadata(model: &mut ManagedModelV2, metadata: Option<&Value>) {
    let Some(Value::Object(mut metadata)) = metadata.and_then(sanitize_metadata) else {
        return;
    };
    model.context_window = metadata
        .remove("context_window")
        .and_then(|value| value.as_i64());
    model.max_context_window = metadata
        .remove("max_context_window")
        .and_then(|value| value.as_i64());
    model.default_reasoning_effort = metadata
        .remove("default_reasoning_level")
        .and_then(|value| value.as_str().map(str::to_owned));
    let capabilities = model
        .capabilities
        .as_object_mut()
        .expect("new discovery model capabilities are an object");
    capabilities.extend(metadata);
    capabilities.insert("discovery_metadata_version".into(), 1.into());
}

// An explicit generation flag takes precedence over output modalities. Without
// either, discovery keeps its existing assumption that the model produces text.
pub(super) fn declared_text_generation(metadata: &Map<String, Value>) -> Option<bool> {
    metadata
        .get("supports_text_generation")
        .or_else(|| metadata.get("supportsTextGeneration"))
        .and_then(Value::as_bool)
        .or_else(|| {
            let outputs = metadata
                .get("output_modalities")
                .or_else(|| metadata.get("outputModalities"))?
                .as_array()?;
            Some(
                outputs
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|s| s.trim().eq_ignore_ascii_case("text")),
            )
        })
}

/// Fill canonicalized capabilities from a sanitized official entry. Compatibility
/// fallbacks belong to the read-only view and must never become stored overrides.
pub(super) fn fill_official_defaults(model: &mut ManagedModelV2, defaults: &Map<String, Value>) {
    let Some(caps) = model.capabilities.as_object_mut() else {
        return;
    };
    if declared_text_generation(caps) == Some(false)
        || declared_text_generation(defaults) == Some(false)
    {
        return;
    }
    if model.context_window.is_none() {
        model.context_window = defaults
            .get("context_window")
            .and_then(Value::as_i64)
            .map(|ctx| model.max_context_window.map_or(ctx, |max| ctx.min(max)));
    }
    if model.max_context_window.is_none() {
        model.max_context_window = defaults
            .get("max_context_window")
            .and_then(Value::as_i64)
            .map(|max| model.context_window.map_or(max, |ctx| max.max(ctx)));
    }
    if model.default_reasoning_effort.is_none() {
        model.default_reasoning_effort = defaults
            .get("default_reasoning_level")
            .and_then(Value::as_str)
            .filter(|level| {
                caps.get("reasoning_efforts").is_none_or(|efforts| {
                    efforts
                        .as_array()
                        .is_some_and(|items| items.iter().any(|v| v.as_str() == Some(*level)))
                })
            })
            .map(str::to_owned);
    }
    for (key, value) in defaults {
        if !matches!(
            key.as_str(),
            "context_window" | "max_context_window" | "default_reasoning_level"
        ) {
            caps.entry(key).or_insert_with(|| value.clone());
        }
    }
}

/// Only for a newly discovered model. Upstream metadata, including restrictions,
/// takes precedence over the official entry and the minimal discovery scaffold.
pub(crate) fn apply_new_model_metadata(
    model: &mut ManagedModelV2,
    metadata: Option<&Value>,
    catalog: &super::official_capabilities::Catalog,
) {
    apply_metadata(model, metadata);
    let mut text = model
        .capabilities
        .as_object()
        .and_then(declared_text_generation);
    if text != Some(false) {
        if let Some(Value::Object(defaults)) =
            catalog.lookup(&model.slug).map(|entry| entry.metadata)
        {
            text = text.or_else(|| declared_text_generation(&defaults));
            fill_official_defaults(model, &defaults);
        }
    }
    let caps = model
        .capabilities
        .as_object_mut()
        .expect("new discovery model capabilities are an object");
    let text = text.unwrap_or(true);
    caps.entry("supports_text_generation")
        .or_insert(Value::Bool(text));
    if text {
        caps.entry("output_modalities")
            .or_insert_with(|| serde_json::json!(["text"]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn discovery_metadata_preserves_official_limits_and_capabilities_without_prompts() {
        let value = json!({
            "context_window": 272000, "max_context_window": 872000,
            "input_modalities": ["text", "image"], "supports_parallel_tool_calls": false,
            "supported_reasoning_levels": [{"effort":"high", "description":"High"}],
            "default_reasoning_level":"high", "service_tiers":[{"id":"priority"}],
            "truncation_policy":{"mode":"tokens","limit":10000},
            "base_instructions":"private prompt", "model_messages":{"instructions_template":"private"},
            "api_key":"private key", "use_responses_lite":true
        });
        let mut model = ManagedModelV2 {
            capabilities: json!({"supports_text_generation":true}),
            ..Default::default()
        };
        apply_metadata(&mut model, Some(&value));
        assert_eq!(model.context_window, Some(272000));
        assert_eq!(model.max_context_window, Some(872000));
        assert_eq!(model.default_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            model.capabilities["input_modalities"],
            json!(["text", "image"])
        );
        assert_eq!(model.capabilities["supports_parallel_tool_calls"], false);
        assert_eq!(model.capabilities["reasoning_efforts"], json!(["high"]));
        assert_eq!(model.capabilities["service_tiers"], json!(["priority"]));
        assert_eq!(model.capabilities["truncation_limit"], 10000);
        assert_eq!(model.capabilities["discovery_metadata_version"], 1);
        for key in [
            "base_instructions",
            "model_messages",
            "api_key",
            "use_responses_lite",
        ] {
            assert!(model.capabilities.get(key).is_none());
        }
    }

    #[test]
    fn invalid_or_absent_discovery_capabilities_remain_unknown() {
        assert!(sanitize_metadata(
            &json!({"context_window":0,"max_context_window":-1,"input_modalities":"image"})
        )
        .is_none());
        let mut model = ManagedModelV2 {
            capabilities: json!({"supports_text_generation":true}),
            ..Default::default()
        };
        apply_metadata(&mut model, None);
        assert!(model.context_window.is_none());
        assert!(model.capabilities.get("input_modalities").is_none());
        assert!(model
            .capabilities
            .get("discovery_metadata_version")
            .is_none());
    }

    #[test]
    fn unknown_discovery_with_only_context_does_not_invent_a_maximum() {
        let mut model = ManagedModelV2 {
            slug: "unknown-context-only".into(),
            capabilities: json!({}),
            ..Default::default()
        };
        apply_new_model_metadata(
            &mut model,
            Some(&json!({"contextWindow":2048})),
            &crate::models_v2::official_capabilities::Catalog::default(),
        );
        assert_eq!(model.context_window, Some(2048));
        assert!(model.max_context_window.is_none());
        assert!(model.default_reasoning_effort.is_none());
        assert!(model.capabilities.get("reasoning_efforts").is_none());
    }

    #[test]
    fn discovery_filters_closed_codex_enums_but_preserves_extensible_reasoning_efforts() {
        let metadata = sanitize_metadata(&json!({
            "context_window":272000,
            "input_modalities":["text","video","image","audio"],
            "reasoning_efforts":["high","extreme","persistent"],
            "default_reasoning_level":"extreme", "multi_agent_reasoning_effort":"future-effort",
            "shell_type":"future_shell", "apply_patch_tool_type":"function",
            "web_search_tool_type":"browser", "default_verbosity":"verbose",
            "default_reasoning_summary":"hidden", "truncation_mode":"characters",
            "defaultServiceTier":"priority", "supportVerbosity":false
        }))
        .unwrap();
        assert_eq!(
            metadata["input_modalities"],
            json!(["text", "image", "audio"])
        );
        assert_eq!(
            metadata["reasoning_efforts"],
            json!(["high", "extreme", "persistent"])
        );
        assert_eq!(metadata["default_reasoning_level"], "extreme");
        assert_eq!(metadata["multi_agent_reasoning_effort"], "future-effort");
        assert_eq!(metadata["default_service_tier"], "priority");
        assert_eq!(metadata["supports_verbosity"], false);
        for key in [
            "shell_type",
            "apply_patch_tool_type",
            "web_search_tool_type",
            "default_verbosity",
            "default_reasoning_summary",
            "truncation_mode",
        ] {
            assert!(
                metadata.get(key).is_none(),
                "unknown closed enum leaked: {key}"
            );
        }
        let valid = sanitize_metadata(&json!({
            "shell_type":"shell_command", "apply_patch_tool_type":"freeform",
            "web_search_tool_type":"text_and_image", "default_verbosity":"high",
            "default_reasoning_summary":"none", "truncation_mode":"bytes"
        }))
        .unwrap();
        assert_eq!(valid["shell_type"], "shell_command");
        assert_eq!(valid["apply_patch_tool_type"], "freeform");
        assert_eq!(valid["web_search_tool_type"], "text_and_image");
        assert_eq!(valid["default_verbosity"], "high");
        assert_eq!(valid["default_reasoning_summary"], "none");
        assert_eq!(valid["truncation_mode"], "bytes");
    }

    #[test]
    fn unknown_input_enums_keep_fallback_while_explicit_empty_input_is_preserved() {
        for value in [json!(["video"]), json!([false, 42]), json!(["", "video"])] {
            let mut model = ManagedModelV2 {
                capabilities: json!({"supports_text_generation":true}),
                ..Default::default()
            };
            apply_metadata(
                &mut model,
                Some(&json!({"context_window":272000,"input_modalities":value})),
            );
            assert!(model.capabilities.get("input_modalities").is_none());
            assert_eq!(
                crate::models_v2::model_info(&model).input_modalities,
                ["text", "image"]
            );
        }
        let mut model = ManagedModelV2 {
            capabilities: json!({"supports_text_generation":true}),
            ..Default::default()
        };
        apply_metadata(&mut model, Some(&json!({"input_modalities":[]})));
        assert_eq!(model.capabilities["input_modalities"], json!([]));
        assert!(crate::models_v2::model_info(&model)
            .input_modalities
            .is_empty());
    }
}
