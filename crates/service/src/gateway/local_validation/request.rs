use crate::apikey_profile::{
    is_gemini_generate_content_request_path, resolve_gateway_protocol_type,
    PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_GEMINI_NATIVE, ROTATION_AGGREGATE_API,
};
use crate::gateway::request_helpers::ParsedRequestMetadata;
use base64::Engine;
use bytes::Bytes;
use codexmanager_core::storage::ApiKey;
use reqwest::Method;
use tiny_http::Request;

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
    let model = base_model
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    super::super::resolve_compact_forwarded_model(model)
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
        return rewrite_path_preserving_query(path, super::super::current_compact_api_path().as_str());
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

fn default_omitted_responses_stream_to_true(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    let Some(obj) = payload.as_object_mut() else {
        return body;
    };
    if obj.contains_key("stream") {
        return body;
    }
    obj.insert("stream".to_string(), serde_json::Value::Bool(true));
    serde_json::to_vec(&payload).unwrap_or(body)
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

    if incoming_headers.conversation_id().is_some() || incoming_headers.turn_state().is_some() {
        // 中文注释：原生 Codex 已经提供稳定线程锚点时，prompt_cache_key 不能反客为主；
        // 否则会和 conversation_id / turn-state 冲突，导致 resume 线程异常。
        return None;
    }

    Some(preferred)
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

fn resolve_client_is_stream(
    protocol_type: &str,
    normalized_path: &str,
    client_is_stream: bool,
    client_stream_specified: bool,
    native_codex_client: bool,
) -> bool {
    client_is_stream
        || (is_non_native_openai_responses_api_request(
            protocol_type,
            normalized_path,
            native_codex_client,
        ) && !client_stream_specified)
        || (protocol_type == PROTOCOL_GEMINI_NATIVE
            && normalized_path.contains(":streamGenerateContent"))
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
            false,
        );
    let rewritten_body = super::super::normalize_official_responses_http_body(
        transport_request_path(path).as_str(),
        rewritten_body,
    );
    let request_meta = super::super::parse_request_metadata(&rewritten_body);
    (
        rewritten_body,
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
    let logical_path = resolve_logical_gateway_request_path(normalized_path.as_str(), &incoming_headers);
    let effective_protocol_type =
        resolve_gateway_protocol_type(api_key.protocol_type.as_str(), logical_path.as_str());
    let request_method = request.method().as_str().to_string();
    let method = Method::from_bytes(request_method.as_bytes()).map_err(|_| {
        LocalValidationError::new(
            405,
            crate::gateway::bilingual_error("不支持的请求方法", "unsupported method"),
        )
    })?;
    let initial_service_tier_diagnostic = super::super::inspect_service_tier_for_log(&body);
    super::super::log_client_service_tier(
        trace_id.as_str(),
        "http",
        logical_path.as_str(),
        initial_service_tier_diagnostic.has_field,
        initial_service_tier_diagnostic.raw_value.as_deref(),
        initial_service_tier_diagnostic.normalized_value.as_deref(),
    );
    let initial_request_meta = super::super::parse_request_metadata(&body);
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
        if is_non_native_openai_responses_api_request(
            effective_protocol_type,
            logical_path.as_str(),
            native_codex_client,
        ) {
            rewritten_body = default_omitted_responses_stream_to_true(rewritten_body);
        }
        let transport_path = transport_request_path(logical_path.as_str());
        super::super::validate_text_input_limit_for_path(&transport_path, &rewritten_body)
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
            conversation_binding: None,
            model_for_log,
            reasoning_for_log,
            service_tier_for_log,
            effective_service_tier_for_log,
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
    if is_non_native_openai_responses_api_request(
        effective_protocol_type,
        logical_path.as_str(),
        native_codex_client,
    ) {
        passthrough_body = default_omitted_responses_stream_to_true(passthrough_body);
    }
    let passthrough_transport_path = transport_request_path(logical_path.as_str());
    super::super::validate_text_input_limit_for_path(&passthrough_transport_path, &passthrough_body)
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
        body = default_omitted_responses_stream_to_true(body);
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
    let conversation_binding = super::super::conversation_binding::load_conversation_binding(
        &storage,
        api_key.key_hash.as_str(),
        local_conversation_id.as_deref(),
    )
    .map_err(|err| LocalValidationError::new(500, err))?;
    let effective_thread_anchor = super::super::resolve_fallback_thread_anchor(
        &incoming_headers,
        local_conversation_id.as_deref(),
        conversation_binding.as_ref(),
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
    body = super::super::normalize_official_responses_http_body(&normalized_transport_path, body);
    super::super::validate_text_input_limit_for_path(&normalized_transport_path, &body)
        .map_err(|err| LocalValidationError::new(400, err.message()))?;

    let request_meta = super::super::parse_request_metadata(&body);
    let model_for_log = request_meta.model.or(api_key.model_slug.clone());
    let reasoning_for_log = request_meta
        .reasoning_effort
        .or(api_key.reasoning_effort.clone());
    let service_tier_for_log = client_request_meta.service_tier;
    let effective_service_tier_for_log = request_meta.service_tier;
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
        conversation_binding,
        rotation_strategy: api_key.rotation_strategy,
        aggregate_api_id: api_key.aggregate_api_id,
        account_plan_filter: api_key.account_plan_filter,
        model_for_log,
        reasoning_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        gateway_mode_for_log: compact_gateway_mode,
        method,
    })
}

#[cfg(test)]
#[path = "tests/request_tests.rs"]
mod tests;

#[cfg(test)]
mod images_generation_tests {
    use super::{
        adapt_openai_images_edits_body_to_responses,
        adapt_openai_images_generations_body_to_responses,
        ensure_codex_image_tool_model_not_used_for_text_request,
    };
    use crate::gateway::ResponseAdapter;
    use serde_json::json;

    #[test]
    fn images_generation_request_builds_responses_image_generation_tool() {
        let body = json!({
            "model": "gpt-image-2",
            "prompt": "画一张极简风格的猫",
            "size": "1024x1024",
            "quality": "high",
            "background": "transparent",
            "output_format": "png",
            "response_format": "url",
            "stream": true,
            "partial_images": 1
        });

        let (mapped, adapter) = adapt_openai_images_generations_body_to_responses(
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt images request");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(adapter, ResponseAdapter::ImagesUrlFromResponses);
        assert_eq!(value["model"], "gpt-5.4-mini");
        assert_eq!(value["stream"], true);
        assert_eq!(value["store"], false);
        assert_eq!(value["tool_choice"]["type"], "image_generation");
        assert_eq!(value["tools"][0]["type"], "image_generation");
        assert_eq!(value["tools"][0]["model"], "gpt-image-2");
        assert_eq!(value["tools"][0]["size"], "1024x1024");
        assert_eq!(value["tools"][0]["quality"], "high");
        assert_eq!(value["tools"][0]["background"], "transparent");
        assert_eq!(value["tools"][0]["partial_images"], 1);
        assert_eq!(
            value["input"][0]["content"][0]["text"],
            "画一张极简风格的猫"
        );
    }

    #[test]
    fn images_generation_request_defaults_b64_json_and_tool_model() {
        let body = json!({ "prompt": "cat" });

        let (mapped, adapter) = adapt_openai_images_generations_body_to_responses(
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt images request");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(adapter, ResponseAdapter::ImagesB64JsonFromResponses);
        assert_eq!(value["tools"][0]["model"], "gpt-image-2");
        assert_eq!(value["tools"][0]["output_format"], "png");
    }

    #[test]
    fn images_generation_request_requires_prompt() {
        let body = json!({ "model": "gpt-image-2" });

        let err = adapt_openai_images_generations_body_to_responses(
            serde_json::to_vec(&body).expect("body"),
        )
        .expect_err("prompt should be required");

        assert!(err.contains("prompt is required"));
    }

    #[test]
    fn images_edits_json_request_builds_responses_with_input_images_and_mask() {
        let body = json!({
            "model": "gpt-image-2",
            "prompt": "把背景改成透明",
            "images": [{
                "image_url": "data:image/png;base64,aW1hZ2U="
            }],
            "mask": {
                "image_url": "data:image/png;base64,bWFzaw=="
            },
            "response_format": "b64_json"
        });

        let (mapped, adapter) = adapt_openai_images_edits_body_to_responses(
            serde_json::to_vec(&body).expect("body"),
            Some("application/json"),
        )
        .expect("adapt edits json request");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(adapter, ResponseAdapter::ImagesB64JsonFromResponses);
        assert_eq!(value["tools"][0]["type"], "image_generation");
        assert_eq!(value["tools"][0]["model"], "gpt-image-2");
        assert_eq!(
            value["tools"][0]["input_image_mask"]["image_url"],
            "data:image/png;base64,bWFzaw=="
        );
        assert_eq!(value["input"][0]["content"][0]["text"], "把背景改成透明");
        assert_eq!(
            value["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,aW1hZ2U="
        );
    }

    #[test]
    fn images_edits_json_rejects_file_id() {
        let body = json!({
            "prompt": "edit",
            "images": [{ "file_id": "file_123" }]
        });

        let err = adapt_openai_images_edits_body_to_responses(
            serde_json::to_vec(&body).expect("body"),
            Some("application/json"),
        )
        .expect_err("file_id should be rejected");

        assert!(err.contains("file_id is not supported"));
    }

    #[test]
    fn images_edits_json_rejects_invalid_base64_data_url() {
        let body = json!({
            "prompt": "edit",
            "images": [{ "image_url": "data:image/png;base64,***" }]
        });

        let err = adapt_openai_images_edits_body_to_responses(
            serde_json::to_vec(&body).expect("body"),
            Some("application/json"),
        )
        .expect_err("invalid base64 should be rejected");

        assert!(err.contains("invalid base64 image data"));
    }

    #[test]
    fn images_edits_multipart_request_builds_data_urls() {
        let body = concat!(
            "--test-boundary\r\n",
            "Content-Disposition: form-data; name=\"prompt\"\r\n\r\n",
            "修图\r\n",
            "--test-boundary\r\n",
            "Content-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\n",
            "Content-Type: image/png\r\n\r\n",
            "IMG\r\n",
            "--test-boundary\r\n",
            "Content-Disposition: form-data; name=\"mask\"; filename=\"m.png\"\r\n",
            "Content-Type: image/png\r\n\r\n",
            "MSK\r\n",
            "--test-boundary--\r\n"
        )
        .as_bytes()
        .to_vec();

        let (mapped, adapter) = adapt_openai_images_edits_body_to_responses(
            body,
            Some("multipart/form-data; boundary=test-boundary"),
        )
        .expect("adapt edits multipart request");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(adapter, ResponseAdapter::ImagesB64JsonFromResponses);
        assert_eq!(value["input"][0]["content"][0]["text"], "修图");
        assert_eq!(
            value["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,SU1H"
        );
        assert_eq!(
            value["tools"][0]["input_image_mask"]["image_url"],
            "data:image/png;base64,TVNL"
        );
    }

    #[test]
    fn rejects_gpt_image_model_on_text_generation_paths() {
        let err = ensure_codex_image_tool_model_not_used_for_text_request(
            "/v1/chat/completions",
            Some("gpt-image-2"),
        )
        .expect_err("text path should reject image tool model");

        assert_eq!(err.status_code, 400);
        assert!(err.message.contains("/v1/images/generations"));
    }

    #[test]
    fn allows_gpt_image_model_on_images_paths() {
        assert!(ensure_codex_image_tool_model_not_used_for_text_request(
            "/v1/images/generations",
            Some("gpt-image-2"),
        )
        .is_ok());
        assert!(ensure_codex_image_tool_model_not_used_for_text_request(
            "/v1/images/edits",
            Some("gpt-image-2"),
        )
        .is_ok());
    }
}

#[cfg(test)]
mod removed_path_tests {
    use super::is_removed_openai_compat_request_path;

    #[test]
    fn identifies_removed_openai_compat_paths() {
        assert!(!is_removed_openai_compat_request_path("/v1/responses"));
        assert!(!is_removed_openai_compat_request_path(
            "/v1/responses/compact"
        ));
        assert!(!is_removed_openai_compat_request_path(
            "/v1/chat/completions"
        ));
        assert!(is_removed_openai_compat_request_path("/v1/completions"));
        assert!(!is_removed_openai_compat_request_path("/v1/messages"));
        assert!(!is_removed_openai_compat_request_path(
            "/v1beta/models/gemini-2.5-pro:generateContent"
        ));
        assert!(!is_removed_openai_compat_request_path(
            "/v1/images/generations"
        ));
        assert!(!is_removed_openai_compat_request_path("/v1/images/edits"));
    }

    #[test]
    fn removed_openai_compat_paths_are_still_limited_to_legacy_completions() {
        assert!(is_removed_openai_compat_request_path("/v1/completions"));
        assert!(!is_removed_openai_compat_request_path("/v1/messages"));
        assert!(!is_removed_openai_compat_request_path(
            "/v1beta/models/gemini-2.5-pro:generateContent"
        ));
    }
}
