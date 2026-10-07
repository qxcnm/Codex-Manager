use crate::apikey_profile::{
    is_gemini_generate_content_request_path, resolve_gateway_protocol_type,
    PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_GEMINI_NATIVE, ROTATION_AGGREGATE_API,
};
use crate::gateway::request_helpers::ParsedRequestMetadata;
use crate::http::gateway_request::GatewayRequest as Request;
use base64::Engine;
use bytes::Bytes;
use codexmanager_core::storage::{ApiKey, ConversationBinding};
use reqwest::Method;
use serde_json::Value;

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

fn should_preserve_openai_images_request_fields(path: &str) -> bool {
    is_openai_images_generations_path(path) || is_openai_images_edits_path(path)
}

fn resolve_effective_request_overrides_for_path(
    api_key: &ApiKey,
    path: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    if should_preserve_openai_images_request_fields(path) {
        // A platform key's text-model, reasoning, and service-tier defaults are not valid Images
        // API overrides. Codex's extension explicitly sends gpt-image-1.5 on this request.
        return (None, None, None);
    }
    resolve_effective_request_overrides(api_key)
}

fn instruction_protocol_for_passthrough(
    protocol_type: &str,
) -> crate::models_v2::instructions::InstructionProtocolV2 {
    match protocol_type {
        PROTOCOL_ANTHROPIC_NATIVE => {
            crate::models_v2::instructions::InstructionProtocolV2::Anthropic
        }
        PROTOCOL_GEMINI_NATIVE => crate::models_v2::instructions::InstructionProtocolV2::Gemini,
        _ => crate::models_v2::instructions::InstructionProtocolV2::OpenAi,
    }
}

fn apply_model_instructions_policy(
    storage: &codexmanager_core::storage::Storage,
    model_slug: Option<&str>,
    body: Vec<u8>,
    protocol: crate::models_v2::instructions::InstructionProtocolV2,
) -> Result<Vec<u8>, LocalValidationError> {
    let Some(model_slug) = model_slug.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(body);
    };
    let model =
        crate::models_v2::enabled_model(storage, crate::models_v2::policy_catalog_slug(model_slug))
            .map_err(|err| {
                LocalValidationError::new(500, format!("model_catalog_v2_read_failed: {err}"))
            })?
            .ok_or_else(|| {
                LocalValidationError::new(404, format!("model_not_found: {model_slug}"))
            })?;
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return Ok(body);
    };
    crate::models_v2::instructions::apply_model_instructions_v2(&mut value, &model, protocol)
        .map_err(|err| LocalValidationError::new(400, err))?;
    serde_json::to_vec(&value).map_err(|err| {
        LocalValidationError::new(
            500,
            format!("serialize instructions policy body failed: {err}"),
        )
    })
}

fn apply_model_fast_policy(
    storage: &codexmanager_core::storage::Storage,
    model_slug: Option<&str>,
    body: Vec<u8>,
    client_service_tier: Option<&str>,
) -> Result<(Vec<u8>, bool), LocalValidationError> {
    let Some(model_slug) = model_slug.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok((body, false));
    };
    let model =
        crate::models_v2::enabled_model(storage, crate::models_v2::policy_catalog_slug(model_slug))
            .map_err(|err| {
                LocalValidationError::new(500, format!("model_catalog_v2_read_failed: {err}"))
            })?
            .ok_or_else(|| {
                LocalValidationError::new(404, format!("model_not_found: {model_slug}"))
            })?;
    crate::models_v2::fast_policy::apply(body, &model, client_service_tier).map_err(|_| {
        LocalValidationError::new(
            400,
            crate::gateway::bilingual_error(
                format!("模型 {model_slug} 不允许 Fast 请求（加速服务等级）"),
                format!(
                    "model {model_slug} does not allow Fast requests (accelerated service tier)"
                ),
            ),
        )
    })
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

    let models = crate::models_v2::models_response_with_storage(storage).map_err(|err| {
        LocalValidationError::new(
            500,
            crate::gateway::bilingual_error(
                "读取模型目录 V2 失败",
                format!("model catalog V2 read failed: {err}"),
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

fn should_adapt_openai_images_request(
    native_codex_client: bool,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
) -> bool {
    // Codex's image extension uses its own HTTP client. Unlike the main Responses client, that
    // request is only guaranteed to carry x-codex-image-turn-id (plus the thread originator), so
    // the general native-client detector can legitimately return false. The turn id is the
    // extension's canonical request signal and must keep the standalone Images API path intact.
    !native_codex_client && !incoming_headers.has_codex_image_turn_id()
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

fn is_codex_image_tool_model(
    storage: &codexmanager_core::storage::Storage,
    model: Option<&str>,
) -> bool {
    let Some(value) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    if value.eq_ignore_ascii_case(DEFAULT_IMAGES_TOOL_MODEL)
        || value.eq_ignore_ascii_case(
            super::super::runtime_config::current_codex_image_tool_model().as_str(),
        )
    {
        return true;
    }
    let normalized_value = value.to_ascii_lowercase();
    if normalized_value.starts_with("gpt-image-") || normalized_value.starts_with("chatgpt-image-")
    {
        return true;
    }
    crate::models_v2::managed_model(storage, crate::models_v2::policy_catalog_slug(value))
        .ok()
        .flatten()
        .is_some_and(|model| {
            crate::models_v2::supports_image_generation(&model)
                && !crate::models_v2::supports_text_generation(&model)
        })
}

fn is_openai_text_generation_path(normalized_path: &str) -> bool {
    normalized_path.starts_with("/v1/chat/completions")
        || normalized_path.starts_with("/v1/responses")
}

fn ensure_non_text_model_not_used_for_text_request(
    storage: &codexmanager_core::storage::Storage,
    normalized_path: &str,
    model: Option<&str>,
) -> Result<(), LocalValidationError> {
    if !is_openai_text_generation_path(normalized_path) {
        return Ok(());
    }

    let Some(model_slug) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };

    if is_codex_image_tool_model(storage, Some(model_slug)) {
        return Err(LocalValidationError::new(
            400,
            crate::gateway::bilingual_error(
                format!("模型 {model_slug} 只能用于图片接口"),
                format!(
                    "model {model_slug} is only supported on /v1/images/generations and /v1/images/edits"
                ),
            ),
        ));
    }

    let catalog_model =
        crate::models_v2::managed_model(storage, crate::models_v2::policy_catalog_slug(model_slug))
            .map_err(|err| {
                LocalValidationError::new(500, format!("model_catalog_v2_read_failed: {err}"))
            })?;
    if catalog_model
        .as_ref()
        .is_none_or(crate::models_v2::supports_text_generation)
    {
        // Unknown slugs remain compatible with external or not-yet-cataloged models.
        return Ok(());
    }

    Err(LocalValidationError::new(
        400,
        crate::gateway::bilingual_error(
            format!("模型 {model_slug} 不支持文本生成"),
            format!("model {model_slug} does not support text generation"),
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

/// Chat completions permits tool message content to be a plain string or an
/// array of content parts, while the Responses API `function_call_output`
/// expects a string `output`. Flatten array content (string parts joined with
/// newlines, image parts as placeholders) so compliant chat clients are not
/// rejected upstream with `output[0].type` errors.
fn chat_tool_content_to_output_string(content: &serde_json::Value) -> serde_json::Value {
    match content {
        serde_json::Value::String(_) => content.clone(),
        serde_json::Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(chat_tool_output_part_text)
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::Value::String(text)
        }
        serde_json::Value::Null => serde_json::Value::String(String::new()),
        other => serde_json::Value::String(other.to_string()),
    }
}

fn chat_tool_output_part_text(part: &serde_json::Value) -> Option<String> {
    match part {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(obj) => {
            let kind = obj
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("text");
            match kind {
                "text" | "input_text" | "output_text" => obj
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                "image_url" => Some("[image]".to_string()),
                _ => None,
            }
        }
        other => Some(other.to_string()),
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
                    .map(chat_tool_content_to_output_string)
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

fn default_omitted_responses_stream_to_true_snapshot(body: Vec<u8>) -> ParsedRequestBodySnapshot {
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
        obj.insert("stream".to_string(), serde_json::Value::Bool(true));
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

fn default_omitted_responses_stream_to_true(body: Vec<u8>) -> Vec<u8> {
    default_omitted_responses_stream_to_true_snapshot(body).body
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

    if raw_value.eq_ignore_ascii_case("fast") || raw_value.eq_ignore_ascii_case("priority") {
        *service_tier = serde_json::Value::String("priority".to_string());
    } else if raw_value.eq_ignore_ascii_case("flex") {
        *service_tier = serde_json::Value::String("flex".to_string());
    } else if raw_value.eq_ignore_ascii_case("ultrafast") {
        *service_tier = serde_json::Value::String("ultrafast".to_string());
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
        (Some(client), Some(effective))
            if crate::apikey::service_tier::service_tier_request_matches_log_value(
                client, effective,
            ) =>
        {
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

fn resolve_reasoning_source_for_log(
    client_value: Option<&str>,
    effective_value: Option<&str>,
    api_key_profile_value: Option<&str>,
) -> Option<String> {
    if api_key_profile_value
        .map(str::trim)
        .is_none_or(str::is_empty)
        && crate::reasoning_effort::is_ultra_to_max_normalization(client_value, effective_value)
    {
        return Some("client_request_normalized".to_string());
    }
    resolve_override_source_for_log(client_value, effective_value, api_key_profile_value)
}

fn resolve_preferred_client_prompt_cache_key(
    protocol_type: &str,
    _incoming_headers: &super::super::IncomingHeaderSnapshot,
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
    preferred
}

fn header_value_present(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| !value.is_empty())
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

fn resolve_route_conversation_id(
    protocol_type: &str,
    normalized_path: &str,
    platform_key_hash: &str,
    model: Option<&str>,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
    initial_request_meta: &ParsedRequestMetadata,
    client_request_meta: &ParsedRequestMetadata,
) -> Option<RouteConversationId> {
    if prompt_cache_route_binding_enabled(protocol_type, normalized_path) {
        if let Some(prompt_cache_key) =
            normalized_prompt_cache_key_for_route(initial_request_meta, client_request_meta)
        {
            let source = if initial_request_meta.has_previous_response_id
                || client_request_meta.has_previous_response_id
            {
                RouteConversationSource::PromptCacheKeyExistingOnly
            } else {
                RouteConversationSource::PromptCacheKey
            };
            return Some(RouteConversationId {
                id: super::super::conversation_binding::cache_affinity_route_id(
                    platform_key_hash,
                    protocol_type,
                    model,
                    super::super::conversation_binding::CacheAffinityKeySource::PromptCacheKey,
                    prompt_cache_key,
                ),
                source,
            });
        }
        if let Some(session_id) = incoming_headers
            .session_id()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let source = if initial_request_meta.has_previous_response_id
                || client_request_meta.has_previous_response_id
            {
                RouteConversationSource::SessionAffinityExistingOnly
            } else {
                RouteConversationSource::SessionAffinity
            };
            return Some(RouteConversationId {
                id: super::super::conversation_binding::cache_affinity_route_id(
                    platform_key_hash,
                    protocol_type,
                    model,
                    super::super::conversation_binding::CacheAffinityKeySource::SessionId,
                    session_id,
                ),
                source,
            });
        }
    }

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
    if route_conversation_source.is_some_and(|source| source.is_cache_affinity()) {
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
        && route_conversation_source.is_some_and(|source| source.is_cache_affinity())
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
    let preserve_images_fields = should_preserve_openai_images_request_fields(path);
    let (default_effective_model, effective_reasoning, effective_service_tier) =
        resolve_effective_request_overrides_for_path(api_key, path);
    let effective_model = model_override
        .map(str::to_string)
        .or(default_effective_model);
    // Aggregate and hybrid fallback requests share this body across candidates.
    // Candidate-specific Codex transport rules are applied only inside the
    // aggregate attempt loop after the actual upstream has been selected.
    let rewritten_body = super::super::apply_request_overrides_for_deferred_aggregate(
        path,
        body,
        effective_model.as_deref(),
        effective_reasoning.as_deref(),
        effective_service_tier.as_deref(),
    );
    let request_meta = super::super::parse_request_json_value(&rewritten_body)
        .as_ref()
        .map(super::super::parse_request_metadata_from_value)
        .unwrap_or_default();
    (
        rewritten_body,
        request_meta.model.or_else(|| {
            (!preserve_images_fields)
                .then(|| api_key.model_slug.clone())
                .flatten()
        }),
        request_meta.reasoning_effort.or_else(|| {
            (!preserve_images_fields)
                .then(|| api_key.reasoning_effort.clone())
                .flatten()
        }),
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
    client_body: Bytes,
    api_key: ApiKey,
) -> Result<LocalValidationResult, LocalValidationError> {
    // `client_body` is shared with the request log client capture; the
    // forwarding path takes owned copies only where it rewrites the body.
    // 按当前策略取消每次请求都更新 api_keys.last_used_at，减少并发写入冲突。
    let account_group_filter =
        crate::apikey::remote::group_filter(&storage, &api_key.id).map_err(|err| {
            LocalValidationError::new(
                500,
                crate::gateway::bilingual_error(
                    "读取 API Key 账号分组失败",
                    format!("read api key account group filter failed: {err}"),
                ),
            )
        })?;
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
    let initial_request_value = super::super::parse_request_json_value(&client_body);
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
    ensure_non_text_model_not_used_for_text_request(
        &storage,
        logical_path.as_str(),
        initial_request_meta.model.as_deref(),
    )?;
    ensure_non_text_model_not_used_for_text_request(
        &storage,
        logical_path.as_str(),
        api_key.model_slug.as_deref(),
    )?;

    if api_key.rotation_strategy == ROTATION_AGGREGATE_API {
        let (
            mut rewritten_body,
            model_for_log,
            reasoning_for_log,
            service_tier_for_log,
            _effective_service_tier_for_log,
            has_prompt_cache_key,
            request_shape,
        ) = apply_passthrough_request_overrides(
            &logical_path,
            client_body.to_vec(),
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
        let reasoning_source_for_log = resolve_reasoning_source_for_log(
            client_reasoning_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            api_key.reasoning_effort.as_deref(),
        );
        rewritten_body = apply_model_instructions_policy(
            &storage,
            model_for_log.as_deref(),
            rewritten_body,
            instruction_protocol_for_passthrough(effective_protocol_type),
        )?;
        let (next_body, fast_policy_applied) = apply_model_fast_policy(
            &storage,
            model_for_log.as_deref(),
            rewritten_body,
            initial_service_tier_diagnostic.raw_value.as_deref(),
        )?;
        rewritten_body = next_body;
        let effective_service_tier_for_log =
            super::super::parse_request_json_value(&rewritten_body)
                .as_ref()
                .map(super::super::parse_request_metadata_from_value)
                .and_then(|metadata| metadata.service_tier);
        let effective_service_tier_for_log =
            crate::apikey::service_tier::recover_omitted_standard_tier_for_log(
                effective_service_tier_for_log,
                api_key.service_tier.as_deref(),
                service_tier_for_log.as_deref(),
                fast_policy_applied,
            );
        let service_tier_source_for_log = if fast_policy_applied {
            Some("model_policy".to_string())
        } else {
            resolve_service_tier_source_for_log(
                initial_service_tier_diagnostic.raw_value.as_deref(),
                effective_service_tier_for_log.as_deref(),
                api_key.service_tier.as_deref(),
            )
        };
        let mut rewritten_body_value_for_validation = None;
        if is_non_native_openai_responses_api_request(
            effective_protocol_type,
            logical_path.as_str(),
            native_codex_client,
        ) {
            let snapshot = default_omitted_responses_stream_to_true_snapshot(rewritten_body);
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
            account_group_filter: account_group_filter.clone(),
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
        client_body.to_vec(),
        &api_key,
        initial_request_meta.service_tier.clone(),
        compact_model_override_for_logical_request.as_deref(),
    )
    .0;
    let passthrough_policy_model = super::super::parse_request_json_value(&passthrough_body)
        .as_ref()
        .map(super::super::parse_request_metadata_from_value)
        .and_then(|metadata| metadata.model);
    let passthrough_model_for_policy = passthrough_policy_model.as_deref().or_else(|| {
        compact_model_override_for_logical_request
            .as_deref()
            .or(api_key.model_slug.as_deref())
            .or(initial_request_meta.model.as_deref())
    });
    passthrough_body = apply_model_instructions_policy(
        &storage,
        passthrough_model_for_policy,
        passthrough_body,
        instruction_protocol_for_passthrough(effective_protocol_type),
    )?;
    passthrough_body = apply_model_fast_policy(
        &storage,
        passthrough_model_for_policy,
        passthrough_body,
        initial_service_tier_diagnostic.raw_value.as_deref(),
    )?
    .0;
    let mut passthrough_body_value_for_validation = None;
    if is_non_native_openai_responses_api_request(
        effective_protocol_type,
        logical_path.as_str(),
        native_codex_client,
    ) {
        let snapshot = default_omitted_responses_stream_to_true_snapshot(passthrough_body);
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
    let original_body = client_body;
    let mut body: Vec<u8> = original_body.to_vec();
    let (mut path, mut response_adapter, mut gemini_stream_output_mode, mut tool_name_restore_map) =
        if effective_protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
            && is_openai_images_generations_path(normalized_path.as_str())
            && should_adapt_openai_images_request(native_codex_client, &incoming_headers)
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
            && should_adapt_openai_images_request(native_codex_client, &incoming_headers)
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
        body = original_body.to_vec();
        response_adapter = super::super::ResponseAdapter::Passthrough;
        gemini_stream_output_mode = None;
        tool_name_restore_map.clear();
    }
    // 中文注释：下游调用方的 stream 语义必须来自原始客户端请求；
    // 否则协议适配（例如 Anthropic/Gemini 转 /responses 强制 stream=true）会污染响应模式判断。
    let client_request_meta = initial_request_meta.clone();
    let preserve_images_fields = should_preserve_openai_images_request_fields(path.as_str());
    let (mut effective_model, effective_reasoning, effective_service_tier) =
        resolve_effective_request_overrides_for_path(&api_key, path.as_str());
    effective_model = resolve_compact_model_override_for_request(
        normalized_path.as_str(),
        &incoming_headers,
        effective_model
            .as_deref()
            .or(initial_request_meta.model.as_deref()),
    )
    .or(effective_model);
    if crate::models_v2::should_preserve_luna_reserve_alias(
        initial_request_meta.model.as_deref(),
        effective_model.as_deref(),
    ) {
        effective_model = Some(codexmanager_core::usage::LUNA_RESERVE_MODEL_SLUG.to_string());
    }
    let instruction_model = effective_model
        .as_deref()
        .or(initial_request_meta.model.as_deref());
    body = apply_model_instructions_policy(
        &storage,
        instruction_model,
        body,
        crate::models_v2::instructions::InstructionProtocolV2::OpenAi,
    )?;
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
        instruction_model,
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
    let fallback_prompt_cache_key = if route_conversation_source.is_some_and(|source| {
        matches!(
            source,
            RouteConversationSource::SessionAffinity
                | RouteConversationSource::SessionAffinityExistingOnly
        )
    }) {
        incoming_headers
            .session_id()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    } else {
        effective_thread_anchor.clone()
    };
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
    } else if fallback_prompt_cache_key.is_some() {
        super::super::apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope(
            &path,
            body,
            effective_model.as_deref(),
            effective_reasoning.as_deref(),
            effective_service_tier.as_deref(),
            api_key.upstream_base_url.as_deref(),
            fallback_prompt_cache_key.as_deref(),
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
    let (next_body, fast_policy_applied) = apply_model_fast_policy(
        &storage,
        instruction_model,
        body,
        initial_service_tier_diagnostic.raw_value.as_deref(),
    )?;
    body = next_body;
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
    let model_for_log = request_meta.model.or_else(|| {
        (!preserve_images_fields)
            .then(|| api_key.model_slug.clone())
            .flatten()
    });
    let model_source_for_log = resolve_override_source_for_log(
        client_model_for_log.as_deref(),
        model_for_log.as_deref(),
        api_key.model_slug.as_deref(),
    );
    let client_reasoning_for_log = client_request_meta.reasoning_effort.clone();
    let reasoning_for_log = request_meta.reasoning_effort.or_else(|| {
        (!preserve_images_fields)
            .then(|| api_key.reasoning_effort.clone())
            .flatten()
    });
    let reasoning_source_for_log = resolve_reasoning_source_for_log(
        client_reasoning_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        api_key.reasoning_effort.as_deref(),
    );
    let service_tier_for_log = client_request_meta.service_tier;
    let effective_service_tier_for_log =
        crate::apikey::service_tier::recover_omitted_standard_tier_for_log(
            request_meta.service_tier,
            api_key.service_tier.as_deref(),
            service_tier_for_log.as_deref(),
            fast_policy_applied,
        );
    let service_tier_source_for_log = if fast_policy_applied {
        Some("model_policy".to_string())
    } else {
        resolve_service_tier_source_for_log(
            initial_service_tier_diagnostic.raw_value.as_deref(),
            effective_service_tier_for_log.as_deref(),
            api_key.service_tier.as_deref(),
        )
    };
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
        route_conversation_id,
        route_conversation_source,
        conversation_binding,
        rotation_strategy: api_key.rotation_strategy,
        aggregate_api_id: api_key.aggregate_api_id,
        account_group_filter,
        account_plan_filter: api_key.account_plan_filter,
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
