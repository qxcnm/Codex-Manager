use serde_json::Value;

use super::request_rewrite_shared::{
    path_matches_template, retain_fields_by_templates, TemplateAllowlist,
};

fn is_chat_completions_create_path(path: &str) -> bool {
    path_matches_template(path, "/v1/chat/completions")
}

fn is_stream_request(obj: &serde_json::Map<String, Value>) -> bool {
    obj.get("stream").and_then(Value::as_bool).unwrap_or(false)
}

pub(super) fn ensure_stream_usage_override(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> bool {
    if !is_chat_completions_create_path(path) {
        return false;
    }
    if !is_stream_request(obj) {
        return false;
    }
    let mut changed = false;
    let stream_options = obj
        .entry("stream_options".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !stream_options.is_object() {
        *stream_options = Value::Object(serde_json::Map::new());
        changed = true;
    }
    if let Some(stream_options_obj) = stream_options.as_object_mut() {
        let has_include_usage = stream_options_obj
            .get("include_usage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !has_include_usage {
            stream_options_obj.insert("include_usage".to_string(), Value::Bool(true));
            changed = true;
        }
    }
    changed
}

pub(super) fn ensure_reasoning_effort(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> bool {
    if !is_chat_completions_create_path(path) {
        return false;
    }

    let mut changed = false;
    if !obj.contains_key("reasoning_effort") {
        let effort = obj
            .get("reasoning")
            .and_then(|v| v.get("effort"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(effort) = effort {
            obj.insert("reasoning_effort".to_string(), Value::String(effort));
            changed = true;
        }
    }
    if obj.remove("reasoning").is_some() {
        changed = true;
    }
    changed
}

pub(super) fn apply_reasoning_override(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
    reasoning_effort: Option<&str>,
) -> bool {
    if !is_chat_completions_create_path(path) {
        return false;
    }
    let Some(level) = reasoning_effort else {
        return false;
    };
    obj.insert(
        "reasoning_effort".to_string(),
        Value::String(level.to_string()),
    );
    true
}

fn is_supported_openai_chat_completions_create_key(key: &str) -> bool {
    matches!(
        key,
        "messages"
            | "model"
            | "audio"
            | "frequency_penalty"
            | "function_call"
            | "functions"
            | "logit_bias"
            | "logprobs"
            | "max_completion_tokens"
            | "max_tokens"
            | "metadata"
            | "modalities"
            | "n"
            | "parallel_tool_calls"
            | "prediction"
            | "presence_penalty"
            | "reasoning_effort"
            | "response_format"
            | "seed"
            | "service_tier"
            | "stop"
            | "store"
            | "stream"
            | "stream_options"
            | "temperature"
            | "tool_choice"
            | "tools"
            | "top_logprobs"
            | "top_p"
            | "user"
            | "web_search_options"
    )
}

fn is_supported_openai_chat_completions_metadata_update_key(key: &str) -> bool {
    matches!(key, "metadata")
}

const CHAT_COMPLETIONS_ALLOWLISTS: &[TemplateAllowlist] = &[
    TemplateAllowlist {
        template: "/v1/chat/completions",
        allow: is_supported_openai_chat_completions_create_key,
    },
    TemplateAllowlist {
        template: "/v1/chat/completions/{completion_id}",
        allow: is_supported_openai_chat_completions_metadata_update_key,
    },
];

pub(super) fn retain_official_fields(
    path: &str,
    obj: &mut serde_json::Map<String, Value>,
) -> Vec<String> {
    retain_fields_by_templates(path, obj, CHAT_COMPLETIONS_ALLOWLISTS)
}
