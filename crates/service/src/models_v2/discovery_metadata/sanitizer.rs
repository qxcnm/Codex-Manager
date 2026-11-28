//! Typed capability allowlist shared by runtime discovery and the offline snapshot refresher.
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
