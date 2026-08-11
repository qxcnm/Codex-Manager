use crate::apikey_profile::{
    is_gemini_generate_content_request_path, resolve_gateway_protocol_type,
    PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_GEMINI_NATIVE, ROTATION_AGGREGATE_API,
};
use crate::gateway::request_helpers::ParsedRequestMetadata;
use base64::Engine;
use bytes::Bytes;
use codexmanager_core::storage::{ApiKey, ConversationBinding};
use reqwest::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tiny_http::Request;

use super::super::conversation_binding::RouteConversationSource;
use super::{LocalValidationError, LocalValidationResult};

const ENV_GATEWAY_BLOCKED_PATHS: &str = "CODEXMANAGER_GATEWAY_BLOCKED_PATHS";
const DEFAULT_GATEWAY_BLOCKED_PATHS: &[&str] = &["/v1/props"];

/// 函数 `resolve_effective_request_overrides`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - api_key: 参数 api_key
///
/// # 返回
/// 返回函数执行结果
fn resolve_effective_request_overrides(
    api_key: &ApiKey,
) -> (Option<String>, Option<String>, Option<String>) {
    let normalized_model = api_key
        .model_slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(super::super::resolve_builtin_forwarded_model)
        .or_else(|| {
            api_key
                .model_slug
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    let normalized_reasoning = api_key
        .reasoning_effort
        .as_deref()
        .and_then(crate::reasoning_effort::normalize_reasoning_effort)
        .map(str::to_string);
    let normalized_service_tier = api_key
        .service_tier
        .as_deref()
        .and_then(crate::apikey::service_tier::normalize_service_tier)
        .map(str::to_string);

    (
        normalized_model,
        normalized_reasoning,
        normalized_service_tier,
    )
}

fn is_removed_openai_compat_request_path(normalized_path: &str) -> bool {
    normalized_path.starts_with("/v1/completions")
}

fn ensure_api_key_policy_allows_model(
    policy: &codexmanager_core::storage::ApiKeyPolicy,
    model: Option<&str>,
) -> Result<(), LocalValidationError> {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if !policy.allowed_models.is_empty()
        && !policy
            .allowed_models
            .iter()
            .any(|allowed| allowed == "*" || allowed.eq_ignore_ascii_case(model))
    {
        return Err(LocalValidationError::new(
            403,
            crate::gateway::bilingual_error(
                "模型不在此 API Key 的白名单中",
                format!("model_not_allowed_by_key: {model}"),
            ),
        ));
    }
    let platform = if model.starts_with("kiro/") {
        Some("kiro")
    } else if model.starts_with("grok/") {
        Some("grok")
    } else if model.starts_with("codex/") || model.starts_with("gpt-") {
        Some("codex")
    } else {
        None
    };
    if let Some(platform) = platform {
        if !policy.allowed_platforms.is_empty()
            && !policy
                .allowed_platforms
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(platform))
        {
            return Err(LocalValidationError::new(
                403,
                crate::gateway::bilingual_error(
                    "此 API Key 不允许访问该平台",
                    format!("platform_not_allowed_by_key: {platform}"),
                ),
            ));
        }
    }
    Ok(())
}

fn configured_gateway_blocked_path_patterns() -> Vec<String> {
    let mut patterns: Vec<String> = DEFAULT_GATEWAY_BLOCKED_PATHS
        .iter()
        .map(|value| (*value).to_string())
        .collect();
    if let Ok(value) = std::env::var(ENV_GATEWAY_BLOCKED_PATHS) {
        patterns.extend(parse_gateway_blocked_path_patterns(value.as_str()));
    }
    patterns
}

fn parse_gateway_blocked_path_patterns(value: &str) -> Vec<String> {
    value
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn path_without_query(path: &str) -> &str {
    path.split_once('?')
        .map(|(prefix, _)| prefix)
        .unwrap_or(path)
}

fn gateway_blocked_path_matches(path: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    let path_only = path_without_query(path);
    if let Some(prefix) = pattern.strip_suffix('*') {
        return path.starts_with(prefix) || path_only.starts_with(prefix);
    }
    path == pattern || path_only == pattern
}

fn is_gateway_blocked_request_path(normalized_path: &str) -> bool {
    configured_gateway_blocked_path_patterns()
        .iter()
        .any(|pattern| gateway_blocked_path_matches(normalized_path, pattern))
}

/// 函数 `ensure_anthropic_model_is_listed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - protocol_type: 参数 protocol_type
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
fn ensure_anthropic_model_is_listed(
    storage: &codexmanager_core::storage::Storage,
    protocol_type: &str,
    model: Option<&str>,
) -> Result<(), LocalValidationError> {
    if protocol_type != PROTOCOL_ANTHROPIC_NATIVE {
        return Ok(());
    }

    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(LocalValidationError::new(
            400,
            crate::gateway::bilingual_error("Claude 模型必填", "claude model is required"),
        ));
    };

    let models = crate::apikey_models::read_model_options_from_storage(storage).map_err(|err| {
        LocalValidationError::new(
            500,
            crate::gateway::bilingual_error(
                "读取模型缓存失败",
                format!("model options cache read failed: {err}"),
            ),
        )
    })?;
    if models.is_empty() {
        return Err(LocalValidationError::new(
            400,
            crate::gateway::bilingual_error(
                "Claude 模型不在模型列表中",
                format!("claude model not found in model list: {model}"),
            ),
        ));
    }
    let found = models
        .models
        .iter()
        .any(|item| item.slug.trim().eq_ignore_ascii_case(model));
    if found {
        Ok(())
    } else {
        Err(LocalValidationError::new(
            400,
            crate::gateway::bilingual_error(
                "Claude 模型不在模型列表中",
                format!("claude model not found in model list: {model}"),
            ),
        ))
    }
}

/// 函数 `allow_openai_responses_path_rewrite`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - protocol_type: 参数 protocol_type
/// - normalized_path: 参数 normalized_path
///
/// # 返回
/// 返回函数执行结果
fn allow_compat_responses_path_rewrite(protocol_type: &str, normalized_path: &str) -> bool {
    (protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
        && (normalized_path.starts_with("/v1/chat/completions")
            || normalized_path.starts_with("/v1/responses")
            || normalized_path.starts_with("/v1/images/generations")
            || normalized_path.starts_with("/v1/images/edits")))
        || (protocol_type == PROTOCOL_GEMINI_NATIVE
            && is_gemini_generate_content_request_path(normalized_path))
}

fn allow_codex_compat_rewrite_for_client(
    protocol_type: &str,
    normalized_path: &str,
    native_codex_client: bool,
) -> bool {
    if protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
        && (normalized_path.starts_with("/v1/chat/completions")
            || normalized_path.starts_with("/v1/responses")
            || normalized_path.starts_with("/v1/images/generations")
            || normalized_path.starts_with("/v1/images/edits"))
    {
        return !native_codex_client;
    }
    allow_compat_responses_path_rewrite(protocol_type, normalized_path)
}

fn should_adapt_openai_chat_completions_to_responses(
    protocol_type: &str,
    normalized_path: &str,
    native_codex_client: bool,
) -> bool {
    protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
        && normalized_path.starts_with("/v1/chat/completions")
        && !native_codex_client
}

fn is_non_native_openai_responses_api_request(
    protocol_type: &str,
    normalized_path: &str,
    native_codex_client: bool,
) -> bool {
    protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
        && normalized_path.starts_with("/v1/responses")
        && !native_codex_client
}

fn is_compact_subagent_request(
    normalized_path: &str,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
) -> bool {
    if normalized_path == "/v1/responses/compact"
        || normalized_path.starts_with("/v1/responses/compact?")
    {
        return true;
    }
    normalized_path.starts_with("/v1/responses")
        && incoming_headers
            .subagent()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("compact"))
}

fn rewrite_path_preserving_query(path: &str, replacement_path: &str) -> String {
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .filter(|query| !query.trim().is_empty());
    match query {
        Some(query) => format!("{replacement_path}?{query}"),
        None => replacement_path.to_string(),
    }
}

fn resolve_logical_gateway_request_path(
    normalized_path: &str,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
) -> String {
    if is_compact_subagent_request(normalized_path, incoming_headers)
        && !(normalized_path == "/v1/responses/compact"
            || normalized_path.starts_with("/v1/responses/compact?"))
    {
        return rewrite_path_preserving_query(normalized_path, "/v1/responses/compact");
    }
    normalized_path.to_string()
}

fn resolve_compact_model_override_for_request(
    normalized_path: &str,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    base_model: Option<&str>,
) -> Option<String> {
    if !is_compact_subagent_request(normalized_path, incoming_headers)
        || normalized_path == "/v1/responses/compact"
        || normalized_path.starts_with("/v1/responses/compact?")
    {
        return None;
    }
    if let Some(explicit_override) = super::super::current_compact_model_override() {
        return Some(explicit_override);
    }
    let _ = base_model;
    None
}

fn maybe_wrap_compact_response_adapter(
    path: &str,
    response_adapter: super::super::ResponseAdapter,
) -> super::super::ResponseAdapter {
    if (path == "/v1/responses/compact" || path.starts_with("/v1/responses/compact?"))
        && super::super::compact_api_path_uses_chat_completions()
    {
        return super::super::ResponseAdapter::CompactFromChatCompletions;
    }
    response_adapter
}

fn transport_request_path(path: &str) -> String {
    if (path == "/v1/responses/compact" || path.starts_with("/v1/responses/compact?"))
        && super::super::compact_api_path_uses_chat_completions()
    {
        return rewrite_path_preserving_query(
            path,
            super::super::current_compact_api_path().as_str(),
        );
    }
    path.to_string()
}

fn is_codex_image_tool_model(model: Option<&str>) -> bool {
    let Some(value) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    if value.eq_ignore_ascii_case(DEFAULT_IMAGES_TOOL_MODEL) {
        return true;
    }
    value.eq_ignore_ascii_case(
        super::super::runtime_config::current_codex_image_tool_model().as_str(),
    )
}

fn is_openai_text_generation_path(normalized_path: &str) -> bool {
    normalized_path.starts_with("/v1/chat/completions")
        || normalized_path.starts_with("/v1/responses")
}

fn ensure_codex_image_tool_model_not_used_for_text_request(
    normalized_path: &str,
    model: Option<&str>,
) -> Result<(), LocalValidationError> {
    if !is_openai_text_generation_path(normalized_path) || !is_codex_image_tool_model(model) {
        return Ok(());
    }

    Err(LocalValidationError::new(
        400,
        crate::gateway::bilingual_error(
            "gpt-image-2 只能用于图片接口",
            "model gpt-image-2 is only supported on /v1/images/generations and /v1/images/edits",
        ),
    ))
}

fn chat_content_to_responses_parts(
    content: &serde_json::Value,
    assistant: bool,
) -> Vec<serde_json::Value> {
    let text_type = if assistant {
        "output_text"
    } else {
        "input_text"
    };
    match content {
        serde_json::Value::String(text) => vec![serde_json::json!({
            "type": text_type,
            "text": text
        })],
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                let obj = part.as_object()?;
                let kind = obj.get("type").and_then(serde_json::Value::as_str)?;
                match kind {
                    "text" | "input_text" | "output_text" => obj
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .map(|text| serde_json::json!({ "type": text_type, "text": text })),
                    "image_url" => obj.get("image_url").map(|image_url| {
                        let url = image_url
                            .as_object()
                            .and_then(|value| value.get("url"))
                            .cloned()
                            .unwrap_or_else(|| image_url.clone());
                        serde_json::json!({ "type": "input_image", "image_url": url })
                    }),
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn chat_tool_to_responses_tool(tool: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = tool.as_object()?;
    if obj.get("type").and_then(serde_json::Value::as_str) != Some("function") {
        return Some(tool.clone());
    }
    let function = obj.get("function").and_then(serde_json::Value::as_object)?;
    let name = function.get("name")?.clone();
    let mut mapped = serde_json::Map::new();
    mapped.insert(
        "type".to_string(),
        serde_json::Value::String("function".to_string()),
    );
    mapped.insert("name".to_string(), name);
    if let Some(description) = function.get("description") {
        mapped.insert("description".to_string(), description.clone());
    }
    mapped.insert(
        "parameters".to_string(),
        function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} })),
    );
    if let Some(strict) = function.get("strict") {
        mapped.insert("strict".to_string(), strict.clone());
    }
    Some(serde_json::Value::Object(mapped))
}

fn chat_tool_choice_to_responses(value: &serde_json::Value) -> serde_json::Value {
    let Some(obj) = value.as_object() else {
        return value.clone();
    };
    if obj.get("type").and_then(serde_json::Value::as_str) != Some("function") {
        return value.clone();
    }
    let Some(name) = obj
        .get("function")
        .and_then(serde_json::Value::as_object)
        .and_then(|function| function.get("name"))
        .cloned()
    else {
        return value.clone();
    };
    serde_json::json!({ "type": "function", "name": name })
}

fn chat_response_format_to_responses_text_format(
    value: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "response_format must be an object".to_string())?;
    let format_type = obj
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "response_format.type is required".to_string())?;

    match format_type {
        "text" => Ok(serde_json::json!({ "type": "text" })),
        "json_object" => Ok(serde_json::json!({ "type": "json_object" })),
        "json_schema" => {
            let schema = obj
                .get("json_schema")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| {
                    "response_format.json_schema must be an object for json_schema".to_string()
                })?;
            let mut mapped = serde_json::Map::new();
            mapped.insert(
                "type".to_string(),
                serde_json::Value::String("json_schema".to_string()),
            );
            for (key, value) in schema {
                mapped.insert(key.clone(), value.clone());
            }
            Ok(serde_json::Value::Object(mapped))
        }
        other => Err(format!("unsupported response_format.type: {other}")),
    }
}

fn chat_text_config_with_response_format(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let has_response_format = obj.contains_key("response_format");
    let mut text = match obj.get("text") {
        Some(serde_json::Value::Null) | None => serde_json::Map::new(),
        Some(serde_json::Value::Object(existing)) => existing.clone(),
        Some(_) if has_response_format => {
            return Err("text must be an object when provided with response_format".to_string());
        }
        Some(_) => serde_json::Map::new(),
    };

    if let Some(response_format) = obj.get("response_format") {
        text.insert(
            "format".to_string(),
            chat_response_format_to_responses_text_format(response_format)?,
        );
    }

    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::Value::Object(text)))
    }
}

const DEFAULT_CHAT_RESPONSES_REASONING_EFFORT: &str = "medium";
const DEFAULT_CHAT_RESPONSES_REASONING_SUMMARY: &str = "auto";

fn chat_reasoning_config_for_responses(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    if let Some(reasoning) = obj.get("reasoning") {
        let mut reasoning = reasoning.clone();
        if let Some(reasoning_obj) = reasoning.as_object_mut() {
            reasoning_obj
                .entry("summary".to_string())
                .or_insert_with(|| {
                    serde_json::Value::String(DEFAULT_CHAT_RESPONSES_REASONING_SUMMARY.to_string())
                });
        }
        return reasoning;
    }

    let effort = obj.get("reasoning_effort").cloned().unwrap_or_else(|| {
        serde_json::Value::String(DEFAULT_CHAT_RESPONSES_REASONING_EFFORT.to_string())
    });
    serde_json::json!({
        "effort": effort,
        "summary": DEFAULT_CHAT_RESPONSES_REASONING_SUMMARY
    })
}

fn adapt_openai_chat_completions_body_to_responses(body: Vec<u8>) -> Result<Vec<u8>, String> {
    let payload = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|err| format!("invalid chat completions request json: {err}"))?;
    let obj = payload
        .as_object()
        .ok_or_else(|| "chat completions request body must be an object".to_string())?;
    let mut rewritten = serde_json::Map::new();
    if let Some(model) = obj.get("model") {
        rewritten.insert("model".to_string(), model.clone());
    }
    let mut input = Vec::new();
    if let Some(messages) = obj.get("messages").and_then(serde_json::Value::as_array) {
        for message in messages {
            let Some(message_obj) = message.as_object() else {
                continue;
            };
            let role = message_obj
                .get("role")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("user");
            if role == "tool" {
                let output = message_obj
                    .get("content")
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::String(String::new()));
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": message_obj
                        .get("tool_call_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                    "output": output
                }));
                continue;
            }
            let responses_role = match role {
                "system" | "developer" => "developer",
                "assistant" => "assistant",
                _ => "user",
            };
            if let Some(content) = message_obj.get("content") {
                let content =
                    chat_content_to_responses_parts(content, responses_role == "assistant");
                if !content.is_empty() {
                    input.push(serde_json::json!({
                        "type": "message",
                        "role": responses_role,
                        "content": content
                    }));
                }
            }
            if let Some(tool_calls) = message_obj
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
            {
                for tool_call in tool_calls {
                    let Some(tool_call_obj) = tool_call.as_object() else {
                        continue;
                    };
                    let Some(function) = tool_call_obj
                        .get("function")
                        .and_then(serde_json::Value::as_object)
                    else {
                        continue;
                    };
                    input.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": tool_call_obj
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default(),
                        "name": function
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default(),
                        "arguments": function
                            .get("arguments")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("{}")
                    }));
                }
            }
        }
    }
    rewritten.insert("input".to_string(), serde_json::Value::Array(input));
    if let Some(stream) = obj.get("stream") {
        rewritten.insert("stream".to_string(), stream.clone());
    }
    rewritten.insert(
        "reasoning".to_string(),
        chat_reasoning_config_for_responses(obj),
    );
    if let Some(tools) = obj.get("tools").and_then(serde_json::Value::as_array) {
        rewritten.insert(
            "tools".to_string(),
            serde_json::Value::Array(
                tools
                    .iter()
                    .filter_map(chat_tool_to_responses_tool)
                    .collect(),
            ),
        );
    }
    if let Some(tool_choice) = obj.get("tool_choice") {
        rewritten.insert(
            "tool_choice".to_string(),
            chat_tool_choice_to_responses(tool_choice),
        );
    }
    if let Some(parallel_tool_calls) = obj.get("parallel_tool_calls") {
        rewritten.insert(
            "parallel_tool_calls".to_string(),
            parallel_tool_calls.clone(),
        );
    }
    if let Some(service_tier) = obj.get("service_tier") {
        rewritten.insert("service_tier".to_string(), service_tier.clone());
    }
    if let Some(metadata) = obj.get("metadata") {
        rewritten.insert("metadata".to_string(), metadata.clone());
    }
    if let Some(text) = chat_text_config_with_response_format(obj)? {
        rewritten.insert("text".to_string(), text);
    }
    serde_json::to_vec(&serde_json::Value::Object(rewritten))
        .map_err(|err| format!("serialize responses compatibility request failed: {err}"))
}

fn default_omitted_responses_stream_to_false_snapshot(body: Vec<u8>) -> ParsedRequestBodySnapshot {
    let Some(mut payload) = super::super::parse_request_json_value(&body) else {
        return ParsedRequestBodySnapshot {
            body,
            value: None,
            metadata: ParsedRequestMetadata::default(),
        };
    };
    let Some(obj) = payload.as_object_mut() else {
        let metadata = super::super::parse_request_metadata_from_value(&payload);
        return ParsedRequestBodySnapshot {
            body,
            value: Some(payload),
            metadata,
        };
    };
    let mut rewritten_body = body;
    if !obj.contains_key("stream") {
        obj.insert("stream".to_string(), serde_json::Value::Bool(false));
        if let Ok(serialized) = serde_json::to_vec(&payload) {
            rewritten_body = serialized;
        }
    }
    let metadata = super::super::parse_request_metadata_from_value(&payload);
    ParsedRequestBodySnapshot {
        body: rewritten_body,
        value: Some(payload),
        metadata,
    }
}

fn default_omitted_responses_stream_to_false(body: Vec<u8>) -> Vec<u8> {
    default_omitted_responses_stream_to_false_snapshot(body).body
}

const DEFAULT_IMAGES_TOOL_MODEL: &str = "gpt-image-2";

fn is_openai_images_generations_path(path: &str) -> bool {
    path == "/v1/images/generations" || path.starts_with("/v1/images/generations?")
}

fn is_openai_images_edits_path(path: &str) -> bool {
    path == "/v1/images/edits" || path.starts_with("/v1/images/edits?")
}

fn request_content_type(request: &Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Content-Type"))
        .map(|header| header.value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_images_response_format(value: Option<&serde_json::Value>) -> &'static str {
    match value.and_then(serde_json::Value::as_str).map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("url") => "url",
        _ => "b64_json",
    }
}

fn copy_tool_field(
    source: &serde_json::Map<String, serde_json::Value>,
    target: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
) {
    if let Some(value) = source.get(field) {
        if !value.is_null() {
            target.insert(field.to_string(), value.clone());
        }
    }
}

fn adapt_openai_images_generations_body_to_responses(
    body: Vec<u8>,
) -> Result<(Vec<u8>, super::super::ResponseAdapter), String> {
    let value = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|err| format!("invalid images generation request JSON: {err}"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| "images generation request must be a JSON object".to_string())?;
    let prompt = obj
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Invalid request: prompt is required".to_string())?;
    let response_format = normalize_images_response_format(obj.get("response_format"));
    let response_adapter = if response_format == "url" {
        super::super::ResponseAdapter::ImagesUrlFromResponses
    } else {
        super::super::ResponseAdapter::ImagesB64JsonFromResponses
    };

    let image_model = obj
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(super::super::runtime_config::current_codex_image_tool_model);

    let mut tool = serde_json::Map::new();
    tool.insert(
        "type".to_string(),
        serde_json::Value::String("image_generation".to_string()),
    );
    tool.insert("model".to_string(), serde_json::Value::String(image_model));
    if !obj.contains_key("output_format") {
        tool.insert(
            "output_format".to_string(),
            serde_json::Value::String("png".to_string()),
        );
    }
    for field in [
        "size",
        "quality",
        "background",
        "output_format",
        "output_compression",
        "partial_images",
    ] {
        copy_tool_field(obj, &mut tool, field);
    }

    let stream = obj
        .get("stream")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let responses = serde_json::json!({
        "model": super::super::runtime_config::current_codex_image_main_model(),
        "instructions": "",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": prompt
            }]
        }],
        "tools": [serde_json::Value::Object(tool)],
        "tool_choice": {
            "type": "image_generation"
        },
        "stream": true,
        "store": false,
        "reasoning": {
            "effort": "medium",
            "summary": "auto"
        },
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"],
        "metadata": {
            "codexmanager_original_path": "/v1/images/generations",
            "codexmanager_client_stream": stream
        }
    });

    serde_json::to_vec(&responses)
        .map(|body| (body, response_adapter))
        .map_err(|err| format!("serialize images generation request failed: {err}"))
}

fn images_response_adapter_from_request(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> super::super::ResponseAdapter {
    if normalize_images_response_format(obj.get("response_format")) == "url" {
        super::super::ResponseAdapter::ImagesUrlFromResponses
    } else {
        super::super::ResponseAdapter::ImagesB64JsonFromResponses
    }
}

fn build_images_tool_from_request(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let image_model = obj
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(super::super::runtime_config::current_codex_image_tool_model);

    let mut tool = serde_json::Map::new();
    tool.insert(
        "type".to_string(),
        serde_json::Value::String("image_generation".to_string()),
    );
    tool.insert("model".to_string(), serde_json::Value::String(image_model));
    if !obj.contains_key("output_format") {
        tool.insert(
            "output_format".to_string(),
            serde_json::Value::String("png".to_string()),
        );
    }
    for field in [
        "size",
        "quality",
        "background",
        "output_format",
        "output_compression",
        "partial_images",
    ] {
        copy_tool_field(obj, &mut tool, field);
    }
    tool
}

fn build_images_responses_request(
    prompt: &str,
    images: &[String],
    mut tool: serde_json::Map<String, serde_json::Value>,
    stream: bool,
) -> Result<Vec<u8>, String> {
    let mut content = vec![serde_json::json!({
        "type": "input_text",
        "text": prompt
    })];
    for image in images {
        content.push(serde_json::json!({
            "type": "input_image",
            "image_url": image
        }));
    }
    if !tool.contains_key("type") {
        tool.insert(
            "type".to_string(),
            serde_json::Value::String("image_generation".to_string()),
        );
    }
    let responses = serde_json::json!({
        "model": super::super::runtime_config::current_codex_image_main_model(),
        "instructions": "",
        "input": [{
            "type": "message",
            "role": "user",
            "content": content
        }],
        "tools": [serde_json::Value::Object(tool)],
        "tool_choice": {
            "type": "image_generation"
        },
        "stream": true,
        "store": false,
        "reasoning": {
            "effort": "medium",
            "summary": "auto"
        },
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"],
        "metadata": {
            "codexmanager_client_stream": stream
        }
    });
    serde_json::to_vec(&responses).map_err(|err| format!("serialize images request failed: {err}"))
}

fn validate_data_image_url(image_url: &str, field: &str) -> Result<String, String> {
    let trimmed = image_url.trim();
    if trimmed.is_empty() {
        return Err(format!("Invalid request: {field} is required"));
    }
    if !trimmed
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
    {
        return Ok(trimmed.to_string());
    }
    let Some((_, b64)) = trimmed.split_once(";base64,") else {
        return Err(format!(
            "Invalid request: {field} must be a base64 data URL"
        ));
    };
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|_| format!("Invalid request: {field} contains invalid base64 image data"))?;
    Ok(trimmed.to_string())
}

fn adapt_openai_images_edits_json_body_to_responses(
    body: Vec<u8>,
) -> Result<(Vec<u8>, super::super::ResponseAdapter), String> {
    let value = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|err| format!("invalid images edits request JSON: {err}"))?;
    let obj = value
        .as_object()
        .ok_or_else(|| "images edits request must be a JSON object".to_string())?;
    let prompt = obj
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Invalid request: prompt is required".to_string())?;
    let response_adapter = images_response_adapter_from_request(obj);
    let mut images = Vec::new();
    let images_value = obj
        .get("images")
        .or_else(|| obj.get("image"))
        .ok_or_else(|| "Invalid request: images[].image_url is required".to_string())?;
    match images_value {
        serde_json::Value::Array(items) => {
            for item in items {
                let image_url = item
                    .get("image_url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if item.get("file_id").is_some() {
                    return Err(
                        "Invalid request: images[].file_id is not supported (use image_url)"
                            .to_string(),
                    );
                }
                if let Some(image_url) = image_url {
                    images.push(validate_data_image_url(image_url, "images[].image_url")?);
                }
            }
        }
        serde_json::Value::Object(item) => {
            if item.get("file_id").is_some() {
                return Err(
                    "Invalid request: image.file_id is not supported (use image_url)".to_string(),
                );
            }
            if let Some(image_url) = item
                .get("image_url")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                images.push(validate_data_image_url(image_url, "image.image_url")?);
            }
        }
        _ => {}
    }
    if images.is_empty() {
        return Err("Invalid request: images[].image_url is required".to_string());
    }

    let mut tool = build_images_tool_from_request(obj);
    if let Some(mask) = obj.get("mask") {
        if mask.get("file_id").is_some() {
            return Err(
                "Invalid request: mask.file_id is not supported (use mask.image_url)".to_string(),
            );
        }
        if let Some(mask_url) = mask
            .get("image_url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            tool.insert(
                "input_image_mask".to_string(),
                serde_json::json!({ "image_url": validate_data_image_url(mask_url, "mask.image_url")? }),
            );
        }
    }
    let stream = obj
        .get("stream")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mapped = build_images_responses_request(prompt, &images, tool, stream)?;
    Ok((mapped, response_adapter))
}

#[derive(Debug)]
struct MultipartPart {
    name: String,
    content_type: Option<String>,
    data: Vec<u8>,
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|part| {
        let part = part.trim();
        let value = part.strip_prefix("boundary=")?;
        Some(value.trim_matches('"').to_string())
    })
}

fn parse_content_disposition_name(value: &str) -> Option<String> {
    value.split(';').find_map(|part| {
        let part = part.trim();
        let value = part.strip_prefix("name=")?;
        Some(value.trim_matches('"').to_string())
    })
}

fn parse_multipart_form(body: &[u8], boundary: &str) -> Result<Vec<MultipartPart>, String> {
    let marker = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();
    for raw_section in split_bytes(body, marker.as_slice()).into_iter().skip(1) {
        let section = trim_prefix_bytes(raw_section, b"\r\n");
        if section.starts_with(b"--") {
            break;
        }
        let Some(header_end) = find_bytes(section, b"\r\n\r\n") else {
            continue;
        };
        let headers_raw = &section[..header_end];
        let mut data_raw = &section[header_end + 4..];
        data_raw = trim_suffix_bytes(data_raw, b"\r\n");
        data_raw = trim_suffix_bytes(data_raw, b"--");
        let mut name = None;
        let mut content_type = None;
        let headers_text = String::from_utf8_lossy(headers_raw);
        for header in headers_text.lines() {
            if let Some((field, value)) = header.split_once(':') {
                if field.trim().eq_ignore_ascii_case("content-disposition") {
                    name = parse_content_disposition_name(value.trim());
                } else if field.trim().eq_ignore_ascii_case("content-type") {
                    let value = value.trim();
                    if !value.is_empty() {
                        content_type = Some(value.to_string());
                    }
                }
            }
        }
        let Some(name) = name else {
            continue;
        };
        parts.push(MultipartPart {
            name,
            content_type,
            data: data_raw.to_vec(),
        });
    }
    if parts.is_empty() {
        Err("Invalid multipart request: no form parts found".to_string())
    } else {
        Ok(parts)
    }
}

fn split_bytes<'a>(data: &'a [u8], marker: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    while let Some(pos) = find_bytes(&data[start..], marker) {
        parts.push(&data[start..start + pos]);
        start += pos + marker.len();
    }
    parts.push(&data[start..]);
    parts
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn trim_prefix_bytes<'a>(value: &'a [u8], prefix: &[u8]) -> &'a [u8] {
    value.strip_prefix(prefix).unwrap_or(value)
}

fn trim_suffix_bytes<'a>(value: &'a [u8], suffix: &[u8]) -> &'a [u8] {
    value.strip_suffix(suffix).unwrap_or(value)
}

fn data_url_from_part(part: &MultipartPart) -> String {
    let mime = part
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("image/png");
    let b64 = base64::engine::general_purpose::STANDARD.encode(part.data.as_slice());
    format!("data:{mime};base64,{b64}")
}

fn adapt_openai_images_edits_multipart_body_to_responses(
    body: Vec<u8>,
    content_type: &str,
) -> Result<(Vec<u8>, super::super::ResponseAdapter), String> {
    let boundary = multipart_boundary(content_type)
        .ok_or_else(|| "Invalid multipart request: missing boundary".to_string())?;
    let parts = parse_multipart_form(&body, boundary.as_str())?;
    let mut obj = serde_json::Map::new();
    let mut images = Vec::new();
    let mut mask = None;
    let mut prompt = None;
    for part in parts {
        match part.name.as_str() {
            "prompt" => {
                prompt = Some(
                    String::from_utf8_lossy(part.data.as_slice())
                        .trim()
                        .to_string(),
                );
            }
            "image" | "image[]" => images.push(data_url_from_part(&part)),
            "mask" => mask = Some(data_url_from_part(&part)),
            "model" | "size" | "quality" | "background" | "output_format" | "response_format" => {
                obj.insert(
                    part.name,
                    serde_json::Value::String(
                        String::from_utf8_lossy(part.data.as_slice())
                            .trim()
                            .to_string(),
                    ),
                );
            }
            "output_compression" | "partial_images" => {
                let raw = String::from_utf8_lossy(part.data.as_slice())
                    .trim()
                    .to_string();
                if let Ok(number) = raw.parse::<i64>() {
                    obj.insert(part.name, serde_json::Value::Number(number.into()));
                }
            }
            "stream" => {
                let raw = String::from_utf8_lossy(part.data.as_slice())
                    .trim()
                    .to_string();
                obj.insert(
                    "stream".to_string(),
                    serde_json::Value::Bool(raw.eq_ignore_ascii_case("true")),
                );
            }
            _ => {}
        }
    }
    let prompt = prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Invalid request: prompt is required".to_string())?;
    if images.is_empty() {
        return Err("Invalid request: image is required".to_string());
    }
    let response_adapter = images_response_adapter_from_request(&obj);
    let mut tool = build_images_tool_from_request(&obj);
    if let Some(mask) = mask {
        tool.insert(
            "input_image_mask".to_string(),
            serde_json::json!({ "image_url": mask }),
        );
    }
    let stream = obj
        .get("stream")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mapped = build_images_responses_request(prompt, &images, tool, stream)?;
    Ok((mapped, response_adapter))
}

fn adapt_openai_images_edits_body_to_responses(
    body: Vec<u8>,
    content_type: Option<&str>,
) -> Result<(Vec<u8>, super::super::ResponseAdapter), String> {
    if content_type
        .map(|value| {
            value
                .to_ascii_lowercase()
                .starts_with("multipart/form-data")
        })
        .unwrap_or(false)
    {
        return adapt_openai_images_edits_multipart_body_to_responses(
            body,
            content_type.unwrap_or_default(),
        );
    }
    adapt_openai_images_edits_json_body_to_responses(body)
}

/// 函数 `should_derive_compat_conversation_anchor`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - protocol_type: 参数 protocol_type
/// - normalized_path: 参数 normalized_path
///
/// # 返回
/// 返回函数执行结果
fn should_derive_compat_conversation_anchor(protocol_type: &str, normalized_path: &str) -> bool {
    (protocol_type == PROTOCOL_ANTHROPIC_NATIVE && normalized_path.starts_with("/v1/messages"))
        || allow_compat_responses_path_rewrite(protocol_type, normalized_path)
}

/// 函数 `is_native_codex_client_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-16
///
/// # 参数
/// - incoming_headers: 参数 incoming_headers
///
/// # 返回
/// 返回函数执行结果
fn is_native_codex_client_request(incoming_headers: &super::super::IncomingHeaderSnapshot) -> bool {
    let user_agent = incoming_headers
        .user_agent()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let originator = incoming_headers
        .originator()
        .unwrap_or_default()
        .to_ascii_lowercase();

    let has_codex_header_signals = incoming_headers.client_request_id().is_some()
        || incoming_headers.subagent().is_some()
        || incoming_headers.beta_features().is_some()
        || incoming_headers.window_id().is_some()
        || incoming_headers.turn_metadata().is_some()
        || incoming_headers.turn_state().is_some()
        || incoming_headers.parent_thread_id().is_some();

    user_agent.contains("codex_cli_rs")
        || originator.contains("codex_cli_rs")
        || user_agent.contains("codex_exec")
        || originator.contains("codex_exec")
        || has_codex_header_signals
}

fn should_normalize_compat_service_tier_for_codex_backend(
    protocol_type: &str,
    normalized_path: &str,
    adapted_path: &str,
) -> bool {
    adapted_path.starts_with("/v1/responses")
        && ((protocol_type == PROTOCOL_ANTHROPIC_NATIVE
            && normalized_path.starts_with("/v1/messages"))
            || allow_compat_responses_path_rewrite(protocol_type, normalized_path))
}

fn normalize_compat_service_tier_for_codex_backend(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    let Some(obj) = payload.as_object_mut() else {
        return body;
    };
    let Some(service_tier) = obj.get_mut("service_tier") else {
        return body;
    };
    let Some(raw_value) = service_tier.as_str() else {
        return body;
    };

    if raw_value.eq_ignore_ascii_case("auto")
        || raw_value.eq_ignore_ascii_case("fast")
        || raw_value.eq_ignore_ascii_case("priority")
    {
        *service_tier = serde_json::Value::String("priority".to_string());
    } else {
        obj.remove("service_tier");
    }

    serde_json::to_vec(&payload).unwrap_or(body)
}

fn resolve_service_tier_source_for_log(
    client_service_tier: Option<&str>,
    effective_service_tier: Option<&str>,
    api_key_service_tier: Option<&str>,
) -> Option<String> {
    match (client_service_tier, effective_service_tier) {
        (Some(client), Some(effective)) if client.eq_ignore_ascii_case(effective) => {
            Some("client_request".to_string())
        }
        (Some(_), Some(_)) => Some("gateway_override".to_string()),
        (None, Some(_)) => {
            if api_key_service_tier
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                Some("api_key_profile".to_string())
            } else {
                Some("gateway_config".to_string())
            }
        }
        (Some(_), None) => Some("client_request".to_string()),
        (None, None) => Some("unset".to_string()),
    }
}

fn resolve_override_source_for_log(
    client_value: Option<&str>,
    effective_value: Option<&str>,
    api_key_profile_value: Option<&str>,
) -> Option<String> {
    match (client_value, effective_value) {
        (Some(client), Some(effective)) if client.eq_ignore_ascii_case(effective) => {
            Some("client_request".to_string())
        }
        (Some(_), Some(_)) => Some("gateway_override".to_string()),
        (None, Some(_)) => {
            if api_key_profile_value
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                Some("api_key_profile".to_string())
            } else {
                Some("gateway_config".to_string())
            }
        }
        (Some(_), None) => Some("client_request".to_string()),
        (None, None) => Some("unset".to_string()),
    }
}

fn resolve_preferred_client_prompt_cache_key(
    protocol_type: &str,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    initial_request_meta: &ParsedRequestMetadata,
    client_request_meta: &ParsedRequestMetadata,
) -> Option<String> {
    if protocol_type == PROTOCOL_ANTHROPIC_NATIVE {
        return None;
    }

    let preferred = initial_request_meta.prompt_cache_key.clone().or_else(|| {
        if client_request_meta.has_prompt_cache_key {
            client_request_meta.prompt_cache_key.clone()
        } else {
            None
        }
    });
    let Some(preferred) = preferred else {
        return None;
    };

    if has_complete_native_thread_anchor(incoming_headers) {
        // 中文注释：原生 Codex 已经提供稳定线程锚点时，prompt_cache_key 不能反客为主；
        // 否则会和 conversation_id / 完整 session+turn-state 冲突，导致 resume 线程异常。
        return None;
    }

    Some(preferred)
}

fn header_value_present(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| !value.is_empty())
}

fn has_complete_native_thread_anchor(
    incoming_headers: &super::super::IncomingHeaderSnapshot,
) -> bool {
    header_value_present(incoming_headers.conversation_id())
        || (header_value_present(incoming_headers.session_id())
            && header_value_present(incoming_headers.turn_state()))
}

fn is_turn_state_only_anchor(incoming_headers: &super::super::IncomingHeaderSnapshot) -> bool {
    !header_value_present(incoming_headers.conversation_id())
        && !header_value_present(incoming_headers.session_id())
        && header_value_present(incoming_headers.turn_state())
}

/// 函数 `resolve_local_conversation_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - protocol_type: 参数 protocol_type
/// - normalized_path: 参数 normalized_path
/// - incoming_headers: 参数 incoming_headers
/// - client_has_prompt_cache_key: 参数 client_has_prompt_cache_key
///
/// # 返回
/// 返回函数执行结果
fn resolve_local_conversation_id(
    protocol_type: &str,
    normalized_path: &str,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    client_has_prompt_cache_key: bool,
) -> Option<String> {
    super::super::resolve_local_conversation_id_with_sticky_fallback(
        incoming_headers,
        !client_has_prompt_cache_key
            && should_derive_compat_conversation_anchor(protocol_type, normalized_path),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteConversationId {
    id: String,
    source: RouteConversationSource,
}

fn prompt_cache_route_binding_enabled(protocol_type: &str, normalized_path: &str) -> bool {
    protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
        && super::super::official_responses_http::is_responses_path(normalized_path)
}

fn normalized_prompt_cache_key_for_route<'a>(
    initial_request_meta: &'a ParsedRequestMetadata,
    client_request_meta: &'a ParsedRequestMetadata,
) -> Option<&'a str> {
    initial_request_meta
        .prompt_cache_key
        .as_deref()
        .or(client_request_meta.prompt_cache_key.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn prompt_cache_route_id(
    platform_key_hash: &str,
    protocol_type: &str,
    prompt_cache_key: &str,
) -> String {
    let digest = Sha256::digest(
        format!(
            "pck:v1\0{platform_key_hash}\0{protocol_type}\0{}",
            prompt_cache_key.trim()
        )
        .as_bytes(),
    );
    format!(
        "pck:v1:{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
        digest[8], digest[9], digest[10], digest[11], digest[12], digest[13], digest[14], digest[15]
    )
}

fn resolve_route_conversation_id(
    protocol_type: &str,
    normalized_path: &str,
    platform_key_hash: &str,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    initial_request_meta: &ParsedRequestMetadata,
    client_request_meta: &ParsedRequestMetadata,
) -> Option<RouteConversationId> {
    if let Some(conversation_id) = incoming_headers
        .conversation_id()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(RouteConversationId {
            id: conversation_id.to_string(),
            source: RouteConversationSource::NativeConversation,
        });
    }

    if prompt_cache_route_binding_enabled(protocol_type, normalized_path) {
        if let Some(prompt_cache_key) =
            normalized_prompt_cache_key_for_route(initial_request_meta, client_request_meta)
        {
            if !header_value_present(incoming_headers.turn_state())
                || !header_value_present(incoming_headers.session_id())
            {
                let source = if initial_request_meta.has_previous_response_id
                    || client_request_meta.has_previous_response_id
                {
                    RouteConversationSource::PromptCacheKeyExistingOnly
                } else {
                    RouteConversationSource::PromptCacheKey
                };
                return Some(RouteConversationId {
                    id: prompt_cache_route_id(platform_key_hash, protocol_type, prompt_cache_key),
                    source,
                });
            }
        }
    }

    if header_value_present(incoming_headers.turn_state()) {
        return None;
    }

    super::super::resolve_local_conversation_id_with_sticky_fallback(
        incoming_headers,
        should_derive_compat_conversation_anchor(protocol_type, normalized_path),
    )
    .map(|id| RouteConversationId {
        id,
        source: RouteConversationSource::StickyFallback,
    })
}

fn conversation_binding_for_thread_anchor<'a>(
    route_conversation_source: Option<RouteConversationSource>,
    conversation_binding: Option<&'a ConversationBinding>,
) -> Option<&'a ConversationBinding> {
    if route_conversation_source.is_some_and(|source| source.is_prompt_cache_key()) {
        None
    } else {
        conversation_binding
    }
}

fn log_anchor_mode_diagnostic(
    trace_id: &str,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    initial_request_meta: &ParsedRequestMetadata,
    client_request_meta: &ParsedRequestMetadata,
    route_conversation_source: Option<RouteConversationSource>,
) {
    if !is_turn_state_only_anchor(incoming_headers) {
        return;
    }
    let prompt_cache_key_present =
        normalized_prompt_cache_key_for_route(initial_request_meta, client_request_meta).is_some();
    let anchor_mode = if prompt_cache_key_present
        && route_conversation_source.is_some_and(|source| source.is_prompt_cache_key())
    {
        "turn_state_only_prompt_cache_route"
    } else {
        "turn_state_only_no_prompt_cache_route"
    };
    log::info!(
        "event=gateway_anchor_mode trace_id={} anchor_mode={} conversation_present={} session_present={} turn_state_present={} prompt_cache_key_present={}",
        trace_id,
        anchor_mode,
        if header_value_present(incoming_headers.conversation_id()) { "true" } else { "false" },
        if header_value_present(incoming_headers.session_id()) { "true" } else { "false" },
        if header_value_present(incoming_headers.turn_state()) { "true" } else { "false" },
        if prompt_cache_key_present { "true" } else { "false" },
    );
}

fn resolve_client_is_stream(
    protocol_type: &str,
    normalized_path: &str,
    client_is_stream: bool,
    _client_stream_specified: bool,
    _native_codex_client: bool,
) -> bool {
    client_is_stream
        || (protocol_type == PROTOCOL_GEMINI_NATIVE
            && normalized_path.contains(":streamGenerateContent"))
}

struct ParsedRequestBodySnapshot {
    body: Vec<u8>,
    value: Option<Value>,
    metadata: ParsedRequestMetadata,
}

fn normalize_official_responses_body_snapshot(
    path: &str,
    body: Vec<u8>,
) -> ParsedRequestBodySnapshot {
    let Some(value) = super::super::parse_request_json_value(&body) else {
        return ParsedRequestBodySnapshot {
            body,
            value: None,
            metadata: ParsedRequestMetadata::default(),
        };
    };
    let (body, value) =
        super::super::normalize_official_responses_http_body_with_value(path, body, value);
    let metadata = value
        .as_ref()
        .map(super::super::parse_request_metadata_from_value)
        .unwrap_or_default();
    ParsedRequestBodySnapshot {
        body,
        value,
        metadata,
    }
}

fn validate_text_input_limit_for_snapshot(
    path: &str,
    value: Option<&Value>,
) -> Result<(), super::super::request_helpers::InputSizeLimitError> {
    let Some(value) = value else {
        return Ok(());
    };
    super::super::validate_text_input_limit_for_value(path, value)
}

fn validate_text_input_limit_for_body_or_snapshot(
    path: &str,
    body: &[u8],
    value: Option<&Value>,
) -> Result<(), super::super::request_helpers::InputSizeLimitError> {
    if let Some(value) = value {
        return super::super::validate_text_input_limit_for_value(path, value);
    }
    super::super::validate_text_input_limit_for_path(path, body)
}

/// 函数 `apply_passthrough_request_overrides`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
/// - body: 参数 body
/// - api_key: 参数 api_key
///
/// # 返回
/// 返回函数执行结果
fn apply_passthrough_request_overrides(
    path: &str,
    body: Vec<u8>,
    api_key: &ApiKey,
    explicit_service_tier_for_log: Option<String>,
    model_override: Option<&str>,
) -> (
    Vec<u8>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
) {
    let (default_effective_model, effective_reasoning, effective_service_tier) =
        resolve_effective_request_overrides(api_key);
    let effective_model = model_override
        .map(str::to_string)
        .or(default_effective_model);
    let rewritten_body =
        super::super::apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
            path,
            body,
            effective_model.as_deref(),
            effective_reasoning.as_deref(),
            effective_service_tier.as_deref(),
            api_key.upstream_base_url.as_deref(),
            None,
            true,
        );
    let normalized = normalize_official_responses_body_snapshot(
        transport_request_path(path).as_str(),
        rewritten_body,
    );
    let request_meta = normalized.metadata;
    (
        normalized.body,
        request_meta.model.or(api_key.model_slug.clone()),
        request_meta
            .reasoning_effort
            .or(api_key.reasoning_effort.clone()),
        explicit_service_tier_for_log,
        request_meta.service_tier,
        request_meta.has_prompt_cache_key,
        request_meta.request_shape,
    )
}

/// 函数 `build_local_validation_result`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn build_local_validation_result(
    request: &Request,
    trace_id: String,
    incoming_headers: super::super::IncomingHeaderSnapshot,
    storage: crate::storage_helpers::StorageHandle,
    mut body: Vec<u8>,
    api_key: ApiKey,
) -> Result<LocalValidationResult, LocalValidationError> {
    // 按当前策略取消每次请求都更新 api_keys.last_used_at，减少并发写入冲突。
    let normalized_path = super::super::normalize_models_path(request.url());
    if is_removed_openai_compat_request_path(normalized_path.as_str()) {
        return Err(LocalValidationError::new(
            410,
            crate::gateway::bilingual_error(
                "OpenAI 兼容请求链路已移除",
                format!("removed request path: {normalized_path}"),
            ),
        ));
    }
    if is_gateway_blocked_request_path(normalized_path.as_str()) {
        return Err(LocalValidationError::new(
            404,
            crate::gateway::bilingual_error(
                "请求路径已被本地屏蔽",
                format!("blocked request path: {normalized_path}"),
            ),
        ));
    }
    let logical_path =
        resolve_logical_gateway_request_path(normalized_path.as_str(), &incoming_headers);
    let effective_protocol_type =
        resolve_gateway_protocol_type(api_key.protocol_type.as_str(), logical_path.as_str());
    let request_method = request.method().as_str().to_string();
    let method = Method::from_bytes(request_method.as_bytes()).map_err(|_| {
        LocalValidationError::new(
            405,
            crate::gateway::bilingual_error("不支持的请求方法", "unsupported method"),
        )
    })?;
    let initial_request_value = super::super::parse_request_json_value(&body);
    let initial_service_tier_diagnostic = initial_request_value
        .as_ref()
        .map(|value| super::super::inspect_service_tier_value(value.get("service_tier")))
        .unwrap_or_default();
    super::super::log_client_service_tier(
        trace_id.as_str(),
        "http",
        logical_path.as_str(),
        initial_service_tier_diagnostic.has_field,
        initial_service_tier_diagnostic.raw_value.as_deref(),
        initial_service_tier_diagnostic.normalized_value.as_deref(),
    );
    let initial_request_meta = initial_request_value
        .as_ref()
        .map(super::super::parse_request_metadata_from_value)
        .unwrap_or_default();
    let api_key_policy = storage
        .find_api_key_policy(&api_key.id)
        .map_err(|err| {
            LocalValidationError::new(500, format!("read api key policy failed: {err}"))
        })?
        .unwrap_or_else(|| codexmanager_core::storage::ApiKeyPolicy {
            key_id: api_key.id.clone(),
            ..Default::default()
        });
    ensure_api_key_policy_allows_model(
        &api_key_policy,
        api_key
            .model_slug
            .as_deref()
            .or(initial_request_meta.model.as_deref()),
    )?;
    let native_codex_client = is_native_codex_client_request(&incoming_headers);
    let compact_gateway_mode =
        is_compact_subagent_request(normalized_path.as_str(), &incoming_headers)
            .then_some("compact".to_string());
    let compact_model_override_for_logical_request = resolve_compact_model_override_for_request(
        normalized_path.as_str(),
        &incoming_headers,
        initial_request_meta
            .model
            .as_deref()
            .or(api_key.model_slug.as_deref()),
    );
    log::debug!(
        "event=gateway_client_profile trace_id={} path={} originator={} user_agent={} session_affinity={} native_codex={}",
        trace_id.as_str(),
        normalized_path.as_str(),
        incoming_headers.originator().unwrap_or("-"),
        incoming_headers.user_agent().unwrap_or("-"),
        incoming_headers.session_affinity().unwrap_or("-"),
        if native_codex_client {
            "true"
        } else {
            "false"
        }
    );
    let initial_local_conversation_id = resolve_local_conversation_id(
        effective_protocol_type,
        logical_path.as_str(),
        &incoming_headers,
        initial_request_meta.has_prompt_cache_key,
    );
    ensure_codex_image_tool_model_not_used_for_text_request(
        logical_path.as_str(),
        initial_request_meta
            .model
            .as_deref()
            .or(api_key.model_slug.as_deref()),
    )?;

    if api_key.rotation_strategy == ROTATION_AGGREGATE_API {
        let (
            mut rewritten_body,
            model_for_log,
            reasoning_for_log,
            service_tier_for_log,
            effective_service_tier_for_log,
            has_prompt_cache_key,
            request_shape,
        ) = apply_passthrough_request_overrides(
            &logical_path,
            body,
            &api_key,
            initial_request_meta.service_tier.clone(),
            compact_model_override_for_logical_request.as_deref(),
        );
        let client_model_for_log = initial_request_meta.model.clone();
        let model_source_for_log = resolve_override_source_for_log(
            client_model_for_log.as_deref(),
            model_for_log.as_deref(),
            api_key.model_slug.as_deref(),
        );
        let client_reasoning_for_log = initial_request_meta.reasoning_effort.clone();
        let reasoning_source_for_log = resolve_override_source_for_log(
            client_reasoning_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            api_key.reasoning_effort.as_deref(),
        );
        let service_tier_source_for_log = resolve_service_tier_source_for_log(
            service_tier_for_log.as_deref(),
            effective_service_tier_for_log.as_deref(),
            api_key.service_tier.as_deref(),
        );
        let mut rewritten_body_value_for_validation = None;
        if is_non_native_openai_responses_api_request(
            effective_protocol_type,
            logical_path.as_str(),
            native_codex_client,
        ) {
            let snapshot = default_omitted_responses_stream_to_false_snapshot(rewritten_body);
            rewritten_body = snapshot.body;
            rewritten_body_value_for_validation = snapshot.value;
        }
        let transport_path = transport_request_path(logical_path.as_str());
        validate_text_input_limit_for_body_or_snapshot(
            &transport_path,
            &rewritten_body,
            rewritten_body_value_for_validation.as_ref(),
        )
        .map_err(|err| LocalValidationError::new(400, err.message()))?;
        let incoming_headers = incoming_headers
            .with_conversation_id_override(initial_local_conversation_id.as_deref());
        let is_stream = resolve_client_is_stream(
            effective_protocol_type,
            logical_path.as_str(),
            initial_request_meta.is_stream,
            initial_request_meta.stream_specified,
            native_codex_client,
        );
        ensure_api_key_policy_allows_model(&api_key_policy, model_for_log.as_deref())?;
        return Ok(LocalValidationResult {
            trace_id,
            incoming_headers,
            storage,
            original_path: normalized_path.clone(),
            passthrough_path: logical_path.clone(),
            path: logical_path.clone(),
            passthrough_body: Bytes::from(rewritten_body.clone()),
            body: Bytes::from(rewritten_body),
            is_stream,
            has_prompt_cache_key,
            request_shape,
            protocol_type: effective_protocol_type.to_string(),
            rotation_strategy: ROTATION_AGGREGATE_API.to_string(),
            aggregate_api_id: api_key.aggregate_api_id,
            account_plan_filter: api_key.account_plan_filter,
            allowed_platforms: api_key_policy.allowed_platforms,
            concurrency_limit: api_key_policy.concurrency_limit,
            response_adapter: maybe_wrap_compact_response_adapter(
                logical_path.as_str(),
                super::super::ResponseAdapter::Passthrough,
            ),
            gemini_stream_output_mode: None,
            tool_name_restore_map: super::super::ToolNameRestoreMap::default(),
            request_method,
            key_id: api_key.id,
            platform_key_hash: api_key.key_hash,
            local_conversation_id: initial_local_conversation_id,
            route_conversation_id: None,
            route_conversation_source: None,
            conversation_binding: None,
            client_model_for_log,
            model_for_log,
            model_source_for_log,
            client_reasoning_for_log,
            reasoning_for_log,
            reasoning_source_for_log,
            service_tier_for_log,
            effective_service_tier_for_log,
            service_tier_source_for_log,
            gateway_mode_for_log: compact_gateway_mode,
            method,
        });
    }

    let passthrough_path = logical_path.clone();
    let mut passthrough_body = apply_passthrough_request_overrides(
        &logical_path,
        body.clone(),
        &api_key,
        initial_request_meta.service_tier.clone(),
        compact_model_override_for_logical_request.as_deref(),
    )
    .0;
    let mut passthrough_body_value_for_validation = None;
    if is_non_native_openai_responses_api_request(
        effective_protocol_type,
        logical_path.as_str(),
        native_codex_client,
    ) {
        let snapshot = default_omitted_responses_stream_to_false_snapshot(passthrough_body);
        passthrough_body = snapshot.body;
        passthrough_body_value_for_validation = snapshot.value;
    }
    let passthrough_transport_path = transport_request_path(logical_path.as_str());
    validate_text_input_limit_for_body_or_snapshot(
        &passthrough_transport_path,
        &passthrough_body,
        passthrough_body_value_for_validation.as_ref(),
    )
    .map_err(|err| LocalValidationError::new(400, err.message()))?;
    let original_body = body.clone();
    let (mut path, mut response_adapter, mut gemini_stream_output_mode, mut tool_name_restore_map) =
        if effective_protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
            && is_openai_images_generations_path(normalized_path.as_str())
            && !native_codex_client
        {
            if !super::super::runtime_config::codex_image_generation_enabled() {
                return Err(LocalValidationError::new(
                    404,
                    crate::gateway::bilingual_error(
                        "OpenAI Images 兼容接口未启用",
                        "OpenAI Images compatible API is disabled",
                    ),
                ));
            }
            let (rewritten_body, response_adapter) =
                adapt_openai_images_generations_body_to_responses(body).map_err(|err| {
                    LocalValidationError::new(
                        400,
                        crate::gateway::bilingual_error("OpenAI Images 兼容适配失败", err),
                    )
                })?;
            body = rewritten_body;
            (
                "/v1/responses".to_string(),
                response_adapter,
                None,
                super::super::ToolNameRestoreMap::default(),
            )
        } else if effective_protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
            && is_openai_images_edits_path(normalized_path.as_str())
            && !native_codex_client
        {
            if !super::super::runtime_config::codex_image_generation_enabled() {
                return Err(LocalValidationError::new(
                    404,
                    crate::gateway::bilingual_error(
                        "OpenAI Images 兼容接口未启用",
                        "OpenAI Images compatible API is disabled",
                    ),
                ));
            }
            let content_type = request_content_type(request);
            let (rewritten_body, response_adapter) =
                adapt_openai_images_edits_body_to_responses(body, content_type.as_deref())
                    .map_err(|err| {
                        LocalValidationError::new(
                            400,
                            crate::gateway::bilingual_error("OpenAI Images 编辑兼容适配失败", err),
                        )
                    })?;
            body = rewritten_body;
            (
                "/v1/responses".to_string(),
                response_adapter,
                None,
                super::super::ToolNameRestoreMap::default(),
            )
        } else {
            let adapted = super::super::adapt_request_for_protocol(
                effective_protocol_type,
                &logical_path,
                body,
            )
            .map_err(|err| {
                LocalValidationError::new(
                    400,
                    crate::gateway::bilingual_error("请求协议适配失败", err),
                )
            })?;
            body = adapted.body;
            (
                adapted.path,
                adapted.response_adapter,
                adapted.gemini_stream_output_mode,
                adapted.tool_name_restore_map,
            )
        };
    if should_adapt_openai_chat_completions_to_responses(
        effective_protocol_type,
        normalized_path.as_str(),
        native_codex_client,
    ) {
        body = adapt_openai_chat_completions_body_to_responses(body).map_err(|err| {
            LocalValidationError::new(
                400,
                crate::gateway::bilingual_error("OpenAI Chat Completions 兼容适配失败", err),
            )
        })?;
        path = "/v1/responses".to_string();
        response_adapter = super::super::ResponseAdapter::ChatCompletionsFromResponses;
    }
    if is_non_native_openai_responses_api_request(
        effective_protocol_type,
        logical_path.as_str(),
        native_codex_client,
    ) {
        body = default_omitted_responses_stream_to_false(body);
    }
    if effective_protocol_type != PROTOCOL_ANTHROPIC_NATIVE
        && !normalized_path.starts_with("/v1/responses")
        && path.starts_with("/v1/responses")
        && !allow_compat_responses_path_rewrite(effective_protocol_type, &normalized_path)
    {
        // 中文注释：防回归保护：仅已登记的兼容协议路径允许改写到 /v1/responses；
        // 其余协议和路径一律保持原路径透传，避免客户端按原生协议却拿到错误的流格式。
        log::warn!(
            "event=gateway_protocol_adapt_guard protocol_type={} from_path={} to_path={} action=force_passthrough",
            effective_protocol_type,
            normalized_path,
            path
        );
        path = normalized_path.clone();
        body = original_body;
        response_adapter = super::super::ResponseAdapter::Passthrough;
        gemini_stream_output_mode = None;
        tool_name_restore_map.clear();
    }
    // 中文注释：下游调用方的 stream 语义必须来自原始客户端请求；
    // 否则协议适配（例如 Anthropic/Gemini 转 /responses 强制 stream=true）会污染响应模式判断。
    let client_request_meta = initial_request_meta.clone();
    let (mut effective_model, effective_reasoning, effective_service_tier) =
        resolve_effective_request_overrides(&api_key);
    effective_model = resolve_compact_model_override_for_request(
        normalized_path.as_str(),
        &incoming_headers,
        effective_model
            .as_deref()
            .or(initial_request_meta.model.as_deref()),
    )
    .or(effective_model);
    let preferred_prompt_cache_key = resolve_preferred_client_prompt_cache_key(
        effective_protocol_type,
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );
    let local_conversation_id = initial_local_conversation_id.clone();
    let allow_codex_compat_rewrite = allow_codex_compat_rewrite_for_client(
        effective_protocol_type,
        logical_path.as_str(),
        native_codex_client,
    );
    let route_conversation = resolve_route_conversation_id(
        effective_protocol_type,
        logical_path.as_str(),
        api_key.key_hash.as_str(),
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );
    let route_conversation_id = route_conversation.as_ref().map(|route| route.id.clone());
    let route_conversation_source = route_conversation.as_ref().map(|route| route.source);
    log_anchor_mode_diagnostic(
        trace_id.as_str(),
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
        route_conversation_source,
    );
    let conversation_binding = super::super::conversation_binding::load_conversation_binding(
        &storage,
        api_key.key_hash.as_str(),
        route_conversation_id.as_deref(),
    )
    .map_err(|err| LocalValidationError::new(500, err))?;
    let binding_for_thread_anchor = conversation_binding_for_thread_anchor(
        route_conversation_source,
        conversation_binding.as_ref(),
    );
    let effective_thread_anchor = super::super::resolve_fallback_thread_anchor(
        &incoming_headers,
        local_conversation_id.as_deref(),
        binding_for_thread_anchor,
    );
    // 中文注释：保留原始 local conversation_id 作为对外会话标识；
    // 线程世代只参与 prompt_cache_key 与路由绑定，不直接污染对外请求头。
    let incoming_headers =
        incoming_headers.with_conversation_id_override(local_conversation_id.as_deref());
    let should_normalize_compat_service_tier =
        should_normalize_compat_service_tier_for_codex_backend(
            effective_protocol_type,
            logical_path.as_str(),
            path.as_str(),
        );
    body = if preferred_prompt_cache_key.is_some() {
        super::super::apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
            &path,
            body,
            effective_model.as_deref(),
            effective_reasoning.as_deref(),
            effective_service_tier.as_deref(),
            api_key.upstream_base_url.as_deref(),
            preferred_prompt_cache_key.as_deref(),
            allow_codex_compat_rewrite,
        )
    } else if effective_thread_anchor.is_some() {
        super::super::apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope(
            &path,
            body,
            effective_model.as_deref(),
            effective_reasoning.as_deref(),
            effective_service_tier.as_deref(),
            api_key.upstream_base_url.as_deref(),
            effective_thread_anchor.as_deref(),
            allow_codex_compat_rewrite,
        )
    } else {
        super::super::apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
            &path,
            body,
            effective_model.as_deref(),
            effective_reasoning.as_deref(),
            effective_service_tier.as_deref(),
            api_key.upstream_base_url.as_deref(),
            None,
            allow_codex_compat_rewrite,
        )
    };
    if should_normalize_compat_service_tier {
        body = normalize_compat_service_tier_for_codex_backend(body);
    }
    response_adapter = maybe_wrap_compact_response_adapter(path.as_str(), response_adapter);
    let normalized_transport_path = transport_request_path(path.as_str());
    let normalized_body =
        normalize_official_responses_body_snapshot(&normalized_transport_path, body);
    validate_text_input_limit_for_snapshot(
        &normalized_transport_path,
        normalized_body.value.as_ref(),
    )
    .map_err(|err| LocalValidationError::new(400, err.message()))?;

    let request_meta = normalized_body.metadata;
    body = normalized_body.body;
    let client_model_for_log = client_request_meta.model.clone();
    let model_for_log = request_meta.model.or(api_key.model_slug.clone());
    let model_source_for_log = resolve_override_source_for_log(
        client_model_for_log.as_deref(),
        model_for_log.as_deref(),
        api_key.model_slug.as_deref(),
    );
    let client_reasoning_for_log = client_request_meta.reasoning_effort.clone();
    let reasoning_for_log = request_meta
        .reasoning_effort
        .or(api_key.reasoning_effort.clone());
    let reasoning_source_for_log = resolve_override_source_for_log(
        client_reasoning_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        api_key.reasoning_effort.as_deref(),
    );
    let service_tier_for_log = client_request_meta.service_tier;
    let effective_service_tier_for_log = request_meta.service_tier;
    let service_tier_source_for_log = resolve_service_tier_source_for_log(
        service_tier_for_log.as_deref(),
        effective_service_tier_for_log.as_deref(),
        api_key.service_tier.as_deref(),
    );
    let is_stream = resolve_client_is_stream(
        effective_protocol_type,
        logical_path.as_str(),
        client_request_meta.is_stream,
        client_request_meta.stream_specified,
        native_codex_client,
    );
    let has_prompt_cache_key = request_meta.has_prompt_cache_key;
    let request_shape = client_request_meta.request_shape;

    ensure_anthropic_model_is_listed(&storage, effective_protocol_type, model_for_log.as_deref())?;
    ensure_api_key_policy_allows_model(&api_key_policy, model_for_log.as_deref())?;

    Ok(LocalValidationResult {
        trace_id,
        incoming_headers,
        storage,
        original_path: normalized_path,
        passthrough_path,
        path,
        passthrough_body: Bytes::from(passthrough_body),
        body: Bytes::from(body),
        is_stream,
        has_prompt_cache_key,
        request_shape,
        protocol_type: effective_protocol_type.to_string(),
        response_adapter,
        gemini_stream_output_mode,
        tool_name_restore_map,
        request_method,
        key_id: api_key.id,
        platform_key_hash: api_key.key_hash,
        local_conversation_id,
        route_conversation_id,
        route_conversation_source,
        conversation_binding,
        rotation_strategy: api_key.rotation_strategy,
        aggregate_api_id: api_key.aggregate_api_id,
        account_plan_filter: api_key.account_plan_filter,
        allowed_platforms: api_key_policy.allowed_platforms,
        concurrency_limit: api_key_policy.concurrency_limit,
        client_model_for_log,
        model_for_log,
        model_source_for_log,
        client_reasoning_for_log,
        reasoning_for_log,
        reasoning_source_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        service_tier_source_for_log,
        gateway_mode_for_log: compact_gateway_mode,
        method,
    })
}

#[cfg(test)]
#[path = "tests/request_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/request_images_tests.rs"]
mod images_generation_tests;

#[cfg(test)]
#[path = "tests/request_removed_path_tests.rs"]
mod removed_path_tests;
