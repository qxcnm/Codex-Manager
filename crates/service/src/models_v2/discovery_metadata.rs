//! Carry model capabilities through discovery without importing upstream prompts or secrets.
use codexmanager_core::storage::ManagedModelV2;
use serde_json::{Map, Value};

fn field<'a>(source: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| source.get(*key)).or_else(|| {
        source
            .get("capabilities")
            .and_then(Value::as_object)
            .and_then(|capabilities| keys.iter().find_map(|key| capabilities.get(*key)))
    })
}

// Closed wire enums from Codex 0.169.0-alpha.11.1 protocol/openai_models.rs
// and protocol/config_types.rs. ReasoningEffort deliberately stays extensible:
// that version accepts every non-empty string as ReasoningEffort::Custom.
fn supported_codex_string(field: &str, value: &str) -> bool {
    match field {
        "shell_type" => matches!(
            value,
            "unified_exec" | "disabled" | "default" | "local" | "shell_command"
        ),
        "apply_patch_tool_type" => value == "freeform",
        "web_search_tool_type" => matches!(value, "text" | "text_and_image"),
        "default_verbosity" => matches!(value, "low" | "medium" | "high"),
        "default_reasoning_summary" => matches!(value, "auto" | "concise" | "detailed" | "none"),
        "truncation_mode" => matches!(value, "tokens" | "bytes"),
        _ => true,
    }
}

/// Only typed capability fields are accepted. In particular, model instructions,
/// arbitrary extra JSON and authentication fields never enter discovery metadata.
pub(crate) fn sanitize_metadata(value: &Value) -> Option<Value> {
    let source = value.as_object()?;
    let mut metadata = Map::new();
    for (name, aliases) in [
        (
            "context_window",
            &["context_window", "contextWindow", "inputTokenLimit"][..],
        ),
        (
            "max_context_window",
            &["max_context_window", "maxContextWindow"][..],
        ),
        (
            "api_context_window",
            &["api_context_window", "apiContextWindow"][..],
        ),
        (
            "max_output_tokens",
            &["max_output_tokens", "maxOutputTokens", "outputTokenLimit"][..],
        ),
        (
            "auto_compact_token_limit",
            &["auto_compact_token_limit", "autoCompactTokenLimit"][..],
        ),
        (
            "truncation_limit",
            &["truncation_limit", "truncationLimit"][..],
        ),
    ] {
        if let Some(number) = field(source, aliases)
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
        {
            metadata.insert(name.into(), number.into());
        }
    }
    if let Some(percent) = field(
        source,
        &[
            "effective_context_window_percent",
            "effectiveContextWindowPercent",
        ],
    )
    .and_then(Value::as_i64)
    .filter(|n| (1..=100).contains(n))
    {
        metadata.insert("effective_context_window_percent".into(), percent.into());
    }
    for (name, aliases) in [
        (
            "supports_text_generation",
            &["supports_text_generation", "supportsTextGeneration"][..],
        ),
        (
            "supports_image_generation",
            &["supports_image_generation", "supportsImageGeneration"][..],
        ),
        (
            "supports_image_editing",
            &["supports_image_editing", "supportsImageEditing"][..],
        ),
        (
            "supports_parallel_tool_calls",
            &["supports_parallel_tool_calls", "supportsParallelToolCalls"][..],
        ),
        (
            "supports_reasoning_summaries",
            &[
                "supports_reasoning_summaries",
                "supports_reasoning_summary_parameter",
                "supportsReasoningSummaries",
            ][..],
        ),
        (
            "supports_verbosity",
            &[
                "supports_verbosity",
                "support_verbosity",
                "supportVerbosity",
                "supportsVerbosity",
            ][..],
        ),
        (
            "supports_image_detail_original",
            &[
                "supports_image_detail_original",
                "supportsImageDetailOriginal",
            ][..],
        ),
        (
            "supports_search_tool",
            &["supports_search_tool", "supportsSearchTool"][..],
        ),
        (
            "prefer_websockets",
            &["prefer_websockets", "preferWebsockets"][..],
        ),
    ] {
        if let Some(flag) = field(source, aliases).and_then(Value::as_bool) {
            metadata.insert(name.into(), flag.into());
        }
    }
    for (name, aliases) in [
        (
            "default_reasoning_level",
            &[
                "default_reasoning_level",
                "defaultReasoningLevel",
                "default_reasoning_effort",
                "defaultReasoningEffort",
            ][..],
        ),
        ("shell_type", &["shell_type", "shellType"][..]),
        (
            "default_reasoning_summary",
            &["default_reasoning_summary", "defaultReasoningSummary"][..],
        ),
        (
            "default_verbosity",
            &["default_verbosity", "defaultVerbosity"][..],
        ),
        (
            "apply_patch_tool_type",
            &["apply_patch_tool_type", "applyPatchToolType"][..],
        ),
        (
            "web_search_tool_type",
            &["web_search_tool_type", "webSearchToolType"][..],
        ),
        (
            "truncation_mode",
            &["truncation_mode", "truncationMode"][..],
        ),
        (
            "reasoning_summary_format",
            &["reasoning_summary_format", "reasoningSummaryFormat"][..],
        ),
        (
            "multi_agent_reasoning_effort",
            &["multi_agent_reasoning_effort", "multiAgentReasoningEffort"][..],
        ),
        ("tool_mode", &["tool_mode", "toolMode"][..]),
        (
            "multi_agent_version",
            &["multi_agent_version", "multiAgentVersion"][..],
        ),
        ("comp_hash", &["comp_hash", "compHash"][..]),
        (
            "default_service_tier",
            &["default_service_tier", "defaultServiceTier"][..],
        ),
    ] {
        if let Some(text) = field(source, aliases)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 200 && !s.chars().any(char::is_control))
            .filter(|s| supported_codex_string(name, s))
        {
            metadata.insert(name.into(), text.into());
        }
    }
    for (name, aliases, object_key) in [
        (
            "input_modalities",
            &["input_modalities", "inputModalities"][..],
            None,
        ),
        (
            "output_modalities",
            &["output_modalities", "outputModalities"][..],
            None,
        ),
        (
            "reasoning_efforts",
            &[
                "reasoning_efforts",
                "reasoningEfforts",
                "supported_reasoning_levels",
                "supportedReasoningLevels",
            ][..],
            Some("effort"),
        ),
        (
            "service_tiers",
            &["service_tiers", "serviceTiers"][..],
            Some("id"),
        ),
        (
            "additional_speed_tiers",
            &["additional_speed_tiers", "additionalSpeedTiers"][..],
            None,
        ),
        (
            "experimental_supported_tools",
            &["experimental_supported_tools", "experimentalSupportedTools"][..],
            None,
        ),
        (
            "supported_endpoints",
            &["supported_endpoints", "supportedEndpoints"][..],
            None,
        ),
    ] {
        if let Some(items) = field(source, aliases).and_then(Value::as_array) {
            let strings: Vec<Value> = items
                .iter()
                .take(100)
                .filter_map(|item| {
                    item.as_str().or_else(|| {
                        object_key
                            .and_then(|key| item.get(key))
                            .and_then(Value::as_str)
                    })
                })
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.len() <= 200 && !s.chars().any(char::is_control))
                .filter(|s| name != "input_modalities" || matches!(*s, "text" | "image" | "audio"))
                .map(Value::from)
                .collect();
            // Unsupported future modalities are missing metadata rather than
            // an explicit empty restriction. Preserve a deliberate empty list.
            if name != "input_modalities" || items.is_empty() || !strings.is_empty() {
                metadata.insert(name.into(), strings.into());
            }
        }
    }
    if let Some(policy) =
        field(source, &["truncation_policy", "truncationPolicy"]).and_then(Value::as_object)
    {
        if let Some(mode) = policy
            .get("mode")
            .and_then(Value::as_str)
            .filter(|s| matches!(*s, "tokens" | "bytes"))
        {
            metadata.insert("truncation_mode".into(), mode.into());
        }
        if let Some(limit) = policy
            .get("limit")
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
        {
            metadata.insert("truncation_limit".into(), limit.into());
        }
    }
    (!metadata.is_empty()).then_some(Value::Object(metadata))
}

pub(crate) fn apply_metadata(model: &mut ManagedModelV2, metadata: Option<&Value>) {
    let Some(Value::Object(mut metadata)) = metadata.and_then(sanitize_metadata) else {
        return;
    };
    model.context_window = metadata
        .remove("context_window")
        .and_then(|value| value.as_i64());
    model.max_context_window = metadata
        .remove("max_context_window")
        .and_then(|value| value.as_i64())
        .or(model.context_window);
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
