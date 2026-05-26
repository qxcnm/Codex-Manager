use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::gateway::runtime_config;

const INSTALLATION_ID_KEY: &str = "x-codex-installation-id";
const DEFAULT_CODEX_COMPAT_INSTRUCTIONS: &str =
    "You are Codex, a helpful AI assistant. Follow the user's instructions.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct OfficialResponsesHttpRequest {
    pub(crate) model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) instructions: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) input: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tools: Vec<Value>,
    #[serde(default, skip_serializing_if = "is_null_or_empty_string")]
    pub(crate) tool_choice: Value,
    #[serde(default)]
    pub(crate) parallel_tool_calls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning: Option<Value>,
    #[serde(default)]
    pub(crate) store: bool,
    #[serde(default)]
    pub(crate) stream: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) client_metadata: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub(crate) extra: Map<String, Value>,
}

fn is_null_or_empty_string(value: &Value) -> bool {
    value.is_null() || value.as_str().is_some_and(str::is_empty)
}

#[derive(Debug, Default)]
pub(crate) struct CodexResponsesRewriteResult {
    pub(crate) changed: bool,
    pub(crate) dropped_keys: Vec<String>,
}

pub(crate) fn is_compact_path(path: &str) -> bool {
    let normalized = path.split('?').next().unwrap_or(path);
    normalized == "/v1/responses/compact"
}

fn is_standard_responses_path(path: &str) -> bool {
    let normalized = path.split('?').next().unwrap_or(path);
    normalized == "/v1/responses"
}

pub(crate) fn is_responses_path(path: &str) -> bool {
    is_standard_responses_path(path) || is_compact_path(path)
}

fn ensure_non_empty_instructions(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_standard_responses_path(path) {
        return false;
    }
    if obj
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        return false;
    }
    obj.insert(
        "instructions".to_string(),
        Value::String(DEFAULT_CODEX_COMPAT_INSTRUCTIONS.to_string()),
    );
    true
}

fn ensure_client_metadata_installation_id(
    path: &str,
    obj: &mut Map<String, Value>,
    installation_id: Option<&str>,
) -> bool {
    if !is_standard_responses_path(path) {
        return false;
    }
    let Some(installation_id) = installation_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let client_metadata = obj
        .entry("client_metadata".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !client_metadata.is_object() {
        *client_metadata = Value::Object(Map::new());
    }
    let Some(client_metadata_obj) = client_metadata.as_object_mut() else {
        return false;
    };
    if client_metadata_obj
        .get(INSTALLATION_ID_KEY)
        .and_then(Value::as_str)
        == Some(installation_id)
    {
        return false;
    }
    client_metadata_obj.insert(
        INSTALLATION_ID_KEY.to_string(),
        Value::String(installation_id.to_string()),
    );
    true
}

fn ensure_input_list(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let Some(input) = obj.get_mut("input") else {
        return false;
    };
    match input {
        Value::String(text) => {
            *input = serde_json::json!([{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text }]
            }]);
            true
        }
        Value::Object(_) => {
            *input = Value::Array(vec![input.clone()]);
            true
        }
        _ => false,
    }
}

fn extract_instruction_text_from_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Array(parts) => {
            let texts = parts
                .iter()
                .filter_map(|part| {
                    let obj = part.as_object()?;
                    let kind = obj.get("type").and_then(Value::as_str)?;
                    if !matches!(kind, "input_text" | "output_text" | "text") {
                        return None;
                    }
                    let text = obj.get("text").and_then(Value::as_str)?.trim();
                    (!text.is_empty()).then(|| text.to_string())
                })
                .collect::<Vec<_>>();
            (!texts.is_empty()).then(|| texts.join("\n\n"))
        }
        _ => None,
    }
}

fn extract_instruction_text_from_message_item(item: &Value) -> Option<String> {
    let obj = item.as_object()?;
    let role = obj.get("role").and_then(Value::as_str)?;
    if !role.eq_ignore_ascii_case("developer") && !role.eq_ignore_ascii_case("system") {
        return None;
    }
    extract_instruction_text_from_content(obj.get("content")?)
}

fn promote_leading_instruction_messages_to_instructions(
    path: &str,
    obj: &mut Map<String, Value>,
) -> bool {
    if !is_standard_responses_path(path) {
        return false;
    }
    if obj
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .is_some()
    {
        return false;
    }
    let (instructions, consumed, single_object) = match obj.get("input") {
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            let mut consumed = 0usize;
            for item in items {
                let Some(text) = extract_instruction_text_from_message_item(item) else {
                    break;
                };
                parts.push(text);
                consumed += 1;
            }
            if parts.is_empty() {
                return false;
            }
            (parts.join("\n\n"), consumed, false)
        }
        Some(Value::Object(_)) => {
            let Some(text) = obj
                .get("input")
                .and_then(extract_instruction_text_from_message_item)
            else {
                return false;
            };
            (text, 1usize, true)
        }
        _ => return false,
    };
    if single_object {
        obj.insert("input".to_string(), Value::Array(Vec::new()));
    } else if let Some(input_array) = obj.get_mut("input").and_then(Value::as_array_mut) {
        input_array.drain(0..consumed);
    } else {
        return false;
    }
    obj.insert("instructions".to_string(), Value::String(instructions));
    true
}

fn ensure_stream_true(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_standard_responses_path(path) {
        return false;
    }
    let stream = obj
        .entry("stream".to_string())
        .or_insert(Value::Bool(false));
    if stream.as_bool() == Some(true) {
        return false;
    }
    *stream = Value::Bool(true);
    true
}

fn take_stream_passthrough_flag(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    obj.remove("stream_passthrough")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn ensure_store_false(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_standard_responses_path(path) {
        return false;
    }
    let store = obj.entry("store".to_string()).or_insert(Value::Bool(false));
    if store.as_bool() == Some(false) {
        return false;
    }
    *store = Value::Bool(false);
    true
}

fn ensure_prompt_cache_key(
    path: &str,
    obj: &mut Map<String, Value>,
    prompt_cache_key: Option<&str>,
    force_override: bool,
) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let Some(prompt_cache_key) = prompt_cache_key.map(str::trim).filter(|v| !v.is_empty()) else {
        return false;
    };
    let existing = obj
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if existing == Some(prompt_cache_key) {
        return false;
    }
    if existing.is_some() && !force_override {
        return false;
    }
    obj.insert(
        "prompt_cache_key".to_string(),
        Value::String(prompt_cache_key.to_string()),
    );
    true
}

fn normalize_compact_instructions(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_compact_path(path) {
        return false;
    }
    let Some(instructions) = obj.get("instructions") else {
        return false;
    };
    let should_remove = match instructions {
        Value::String(text) => text.is_empty(),
        Value::Null => true,
        _ => true,
    };
    if !should_remove {
        return false;
    }
    obj.remove("instructions");
    true
}

fn ensure_tool_choice_auto(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_standard_responses_path(path) {
        return false;
    }
    match obj.get("tool_choice") {
        Some(Value::Object(_)) => return false,
        Some(Value::String(existing)) if existing.eq_ignore_ascii_case("auto") => return false,
        Some(Value::String(existing)) if !existing.trim().is_empty() => return false,
        Some(_) => {}
        None => {}
    }
    obj.insert("tool_choice".to_string(), Value::String("auto".to_string()));
    true
}

fn ensure_tools_list(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    match obj.get("tools") {
        Some(Value::Array(_)) => false,
        _ => {
            obj.insert("tools".to_string(), Value::Array(Vec::new()));
            true
        }
    }
}

fn should_skip_image_generation_tool_for_model(obj: &Map<String, Value>) -> bool {
    obj.get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|model| model.to_ascii_lowercase().ends_with("spark"))
}

fn ensure_image_generation_tool(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    if !runtime_config::codex_image_generation_enabled()
        || !runtime_config::codex_image_generation_auto_inject_tool_enabled()
        || should_skip_image_generation_tool_for_model(obj)
    {
        return false;
    }

    let tools = obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !tools.is_array() {
        *tools = Value::Array(Vec::new());
    }
    let Some(tools_array) = tools.as_array_mut() else {
        return false;
    };
    if tools_array.iter().any(|tool| {
        tool.get("type")
            .and_then(Value::as_str)
            .is_some_and(|tool_type| tool_type == "image_generation")
    }) {
        return false;
    }

    tools_array.push(serde_json::json!({
        "type": "image_generation",
        "output_format": "png"
    }));
    true
}

fn ensure_parallel_tool_calls_bool(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    match obj.get("parallel_tool_calls") {
        Some(Value::Bool(_)) => false,
        _ => {
            obj.insert("parallel_tool_calls".to_string(), Value::Bool(false));
            true
        }
    }
}

fn ensure_include_list(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_standard_responses_path(path) {
        return false;
    }
    match obj.get("include") {
        Some(Value::Array(_)) => false,
        _ => {
            obj.insert("include".to_string(), Value::Array(Vec::new()));
            true
        }
    }
}

fn ensure_reasoning_include(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_standard_responses_path(path) || !obj.contains_key("reasoning") {
        return false;
    }
    let include = obj
        .entry("include".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !include.is_array() {
        *include = Value::Array(Vec::new());
    }
    let Some(include_array) = include.as_array_mut() else {
        return false;
    };
    if include_array.iter().any(|value| {
        value
            .as_str()
            .is_some_and(|item| item == "reasoning.encrypted_content")
    }) {
        return false;
    }
    include_array.push(Value::String("reasoning.encrypted_content".to_string()));
    true
}

fn normalize_service_tier(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !(is_standard_responses_path(path) || is_compact_path(path)) {
        return false;
    }
    let Some(service_tier) = obj.get_mut("service_tier") else {
        return false;
    };
    let Some(raw_value) = service_tier.as_str() else {
        return false;
    };
    if raw_value.eq_ignore_ascii_case("auto")
        || raw_value.eq_ignore_ascii_case("fast")
        || raw_value.eq_ignore_ascii_case("priority")
    {
        *service_tier = Value::String("priority".to_string());
    } else {
        obj.remove("service_tier");
    }
    true
}

fn normalize_codex_backend_service_tier(path: &str, obj: &mut Map<String, Value>) -> bool {
    normalize_service_tier(path, obj)
}

fn normalize_dynamic_tools_to_tools(path: &str, obj: &mut Map<String, Value>) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let dynamic_tools = obj
        .remove("dynamic_tools")
        .or_else(|| obj.remove("dynamicTools"));
    let Some(dynamic_tools) = dynamic_tools else {
        return false;
    };
    let dynamic_tools = dynamic_tools.as_array().cloned().unwrap_or_default();
    let tools = obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !tools.is_array() {
        *tools = Value::Array(Vec::new());
    }
    let Some(tools_array) = tools.as_array_mut() else {
        return true;
    };
    for dynamic_tool in dynamic_tools {
        let Some(tool_obj) = dynamic_tool.as_object() else {
            continue;
        };
        let Some(name) = tool_obj
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let mut mapped = Map::new();
        mapped.insert("type".to_string(), Value::String("function".to_string()));
        mapped.insert("name".to_string(), Value::String(name.to_string()));
        mapped.insert(
            "description".to_string(),
            tool_obj
                .get("description")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
        );
        mapped.insert(
            "parameters".to_string(),
            tool_obj
                .get("input_schema")
                .or_else(|| tool_obj.get("inputSchema"))
                .or_else(|| tool_obj.get("parameters"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} })),
        );
        if let Some(strict) = tool_obj.get("strict") {
            mapped.insert("strict".to_string(), strict.clone());
        }
        tools_array.push(Value::Object(mapped));
    }
    true
}

pub(crate) fn apply_reasoning_override(
    path: &str,
    obj: &mut Map<String, Value>,
    reasoning_effort: Option<&str>,
) -> bool {
    if !is_responses_path(path) {
        return false;
    }
    let Some(level) = reasoning_effort else {
        return false;
    };
    let reasoning = obj
        .entry("reasoning".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !reasoning.is_object() {
        *reasoning = Value::Object(Map::new());
    }
    let Some(reasoning_obj) = reasoning.as_object_mut() else {
        return false;
    };
    reasoning_obj.insert("effort".to_string(), Value::String(level.to_string()));
    true
}

fn retain_allowed_fields(path: &str, obj: &mut Map<String, Value>, allow: &[&str]) -> Vec<String> {
    if !is_responses_path(path) {
        return Vec::new();
    }
    let allow = allow
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let keys = obj.keys().cloned().collect::<Vec<_>>();
    let mut dropped = Vec::new();
    for key in keys {
        if !allow.contains(key.as_str()) {
            obj.remove(&key);
            dropped.push(key);
        }
    }
    dropped
}

pub(crate) fn retain_official_fields(path: &str, obj: &mut Map<String, Value>) -> Vec<String> {
    if is_compact_path(path) {
        return retain_allowed_fields(
            path,
            obj,
            &[
                "input",
                "instructions",
                "metadata",
                "model",
                "parallel_tool_calls",
                "prompt_cache_key",
                "reasoning",
                "service_tier",
                "text",
                "tools",
            ],
        );
    }
    retain_allowed_fields(
        path,
        obj,
        &[
            "include",
            "input",
            "instructions",
            "max_output_tokens",
            "metadata",
            "model",
            "parallel_tool_calls",
            "previous_response_id",
            "reasoning",
            "service_tier",
            "store",
            "stream",
            "temperature",
            "text",
            "tool_choice",
            "tools",
            "top_p",
            "truncation",
            "user",
        ],
    )
}

pub(crate) fn retain_codex_fields(path: &str, obj: &mut Map<String, Value>) -> Vec<String> {
    if is_compact_path(path) {
        return retain_allowed_fields(
            path,
            obj,
            &[
                "model",
                "instructions",
                "input",
                "tools",
                "parallel_tool_calls",
                "prompt_cache_key",
                "reasoning",
                "service_tier",
                "text",
            ],
        );
    }
    retain_allowed_fields(
        path,
        obj,
        &[
            "model",
            "instructions",
            "input",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "reasoning",
            "service_tier",
            "store",
            "stream",
            "include",
            "prompt_cache_key",
            "client_metadata",
            "text",
        ],
    )
}

pub(crate) fn apply_codex_http_request_rules(
    path: &str,
    obj: &mut Map<String, Value>,
    use_codex_compat_rewrite: bool,
    prompt_cache_key: Option<&str>,
    force_prompt_cache_key: bool,
    installation_id: Option<&str>,
) -> CodexResponsesRewriteResult {
    let mut result = CodexResponsesRewriteResult::default();

    if normalize_codex_backend_service_tier(path, obj) {
        result.changed = true;
    }
    if promote_leading_instruction_messages_to_instructions(path, obj) {
        result.changed = true;
    }
    if use_codex_compat_rewrite && ensure_non_empty_instructions(path, obj) {
        result.changed = true;
    }
    if normalize_compact_instructions(path, obj) {
        result.changed = true;
    }
    if ensure_client_metadata_installation_id(path, obj, installation_id) {
        result.changed = true;
    }
    if use_codex_compat_rewrite {
        if normalize_dynamic_tools_to_tools(path, obj) {
            result.changed = true;
        }
        if ensure_input_list(path, obj) {
            result.changed = true;
        }
        if ensure_tools_list(path, obj) {
            result.changed = true;
        }
        if ensure_parallel_tool_calls_bool(path, obj) {
            result.changed = true;
        }
    }
    if !is_compact_path(path) {
        let had_stream_passthrough = obj.contains_key("stream_passthrough");
        if use_codex_compat_rewrite {
            let stream_passthrough = take_stream_passthrough_flag(path, obj);
            if had_stream_passthrough {
                result.changed = true;
            }
            if !stream_passthrough && ensure_stream_true(path, obj) {
                result.changed = true;
            }
            if ensure_store_false(path, obj) {
                result.changed = true;
            }
            if ensure_tool_choice_auto(path, obj) {
                result.changed = true;
            }
            if ensure_include_list(path, obj) {
                result.changed = true;
            }
            if ensure_reasoning_include(path, obj) {
                result.changed = true;
            }
        } else if had_stream_passthrough {
            obj.remove("stream_passthrough");
            result.changed = true;
        }
        if (force_prompt_cache_key || use_codex_compat_rewrite)
            && ensure_prompt_cache_key(path, obj, prompt_cache_key, force_prompt_cache_key)
        {
            result.changed = true;
        }
    } else if (force_prompt_cache_key || use_codex_compat_rewrite)
        && ensure_prompt_cache_key(path, obj, prompt_cache_key, force_prompt_cache_key)
    {
        result.changed = true;
    }
    if ensure_image_generation_tool(path, obj) {
        result.changed = true;
    }

    let dropped = retain_codex_fields(path, obj);
    if !dropped.is_empty() {
        result.changed = true;
        result.dropped_keys = dropped;
    }

    result
}

pub(crate) fn normalize_official_responses_http_body(path: &str, body: Vec<u8>) -> Vec<u8> {
    if !(path == "/v1/responses"
        || path.starts_with("/v1/responses?")
        || path == "/v1/responses/compact"
        || path.starts_with("/v1/responses/compact?"))
    {
        return body;
    }

    let Ok(request) = serde_json::from_slice::<OfficialResponsesHttpRequest>(&body) else {
        return body;
    };
    serde_json::to_vec(&request).unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::{apply_codex_http_request_rules, normalize_official_responses_http_body};
    use serde_json::{json, Value};

    #[test]
    fn responses_http_normalizer_preserves_official_shape_and_unknown_fields() {
        let body = serde_json::to_vec(&json!({
            "model": "gpt-5.4",
            "instructions": "test",
            "input": [{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}],
            "tools": [],
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "store": true,
            "stream": true,
            "include": ["reasoning.encrypted_content"],
            "prompt_cache_key": "thread-1",
            "client_metadata": {"k":"v"},
            "custom_passthrough": true
        }))
        .expect("serialize body");

        let normalized = normalize_official_responses_http_body("/v1/responses", body);
        let value: serde_json::Value =
            serde_json::from_slice(&normalized).expect("parse normalized body");

        assert_eq!(value["model"], "gpt-5.4");
        assert_eq!(value["tool_choice"], "auto");
        assert_eq!(value["stream"], true);
        assert_eq!(value["custom_passthrough"], true);
    }

    #[test]
    fn codex_http_rules_promote_and_fill_standard_responses_defaults() {
        let mut obj = serde_json::json!({
            "model": "gpt-5.4",
            "input": [{
                "role": "developer",
                "content": [{"type":"input_text","text":"follow rules"}]
            }],
            "reasoning": {"effort":"high"}
        })
        .as_object()
        .cloned()
        .expect("object");

        let result = apply_codex_http_request_rules(
            "/v1/responses",
            &mut obj,
            true,
            Some("thread-1"),
            false,
            Some("install-1"),
        );

        assert!(result.changed);
        assert_eq!(
            obj.get("instructions").and_then(Value::as_str),
            Some("follow rules")
        );
        assert_eq!(obj.get("stream").and_then(Value::as_bool), Some(true));
        assert_eq!(obj.get("store").and_then(Value::as_bool), Some(false));
        assert_eq!(obj.get("tool_choice").and_then(Value::as_str), Some("auto"));
        assert_eq!(
            obj.get("prompt_cache_key").and_then(Value::as_str),
            Some("thread-1")
        );
        assert_eq!(
            obj.get("client_metadata")
                .and_then(Value::as_object)
                .and_then(|value| value.get("x-codex-installation-id"))
                .and_then(Value::as_str),
            Some("install-1")
        );
    }
}
