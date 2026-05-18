use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Request, Response, StatusCode};

use crate::gateway::error_log::GatewayErrorLogInput;
use crate::gateway::upstream::GatewayStreamResponse;

use super::super::{GeminiStreamOutputMode, ResponseAdapter, ToolNameRestoreMap};
use super::{
    build_images_api_response, collect_image_generation_chat_images,
    collect_non_stream_json_from_sse_bytes, extract_error_hint_from_body,
    extract_error_message_from_json, looks_like_sse_payload, merge_usage, parse_usage_from_json,
    push_trace_id_header, usage_has_signal, AnthropicSseReader,
    ChatCompletionsFromResponsesSseReader, GeminiSseReader, ImagesFromResponsesSseReader,
    ImagesResponseFormat, OpenAIResponsesPassthroughSseReader, PassthroughSseCollector,
    PassthroughSseProtocol, PassthroughSseUsageReader, SseKeepAliveFrame,
    UpstreamResponseBridgeResult, UpstreamResponseUsage,
};

const REQUEST_ID_HEADER_CANDIDATES: &[&str] = &["x-request-id", "x-oai-request-id"];
const CF_RAY_HEADER_NAME: &str = "cf-ray";
const AUTH_ERROR_HEADER_NAME: &str = "x-openai-authorization-error";

/// 函数 `is_compact_request_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn is_compact_request_path(path: &str) -> bool {
    path == "/v1/responses/compact" || path.starts_with("/v1/responses/compact?")
}

/// 函数 `first_upstream_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - names: 参数 names
///
/// # 返回
/// 返回函数执行结果
fn first_upstream_header(headers: &reqwest::header::HeaderMap, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

/// 函数 `compact_debug_suffix`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - kind: 参数 kind
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn compact_debug_suffix(
    kind: Option<&str>,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> String {
    let mut details = Vec::new();
    if let Some(kind) = kind.map(str::trim).filter(|value| !value.is_empty()) {
        details.push(format!("kind={kind}"));
    }
    if let Some(request_id) = request_id.map(str::trim).filter(|value| !value.is_empty()) {
        details.push(format!("request_id={request_id}"));
    }
    if let Some(cf_ray) = cf_ray.map(str::trim).filter(|value| !value.is_empty()) {
        details.push(format!("cf_ray={cf_ray}"));
    }
    if let Some(auth_error) = auth_error.map(str::trim).filter(|value| !value.is_empty()) {
        details.push(format!("auth_error={auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(format!("identity_error_code={identity_error_code}"));
    }
    if details.is_empty() {
        String::new()
    } else {
        format!(" [{}]", details.join(", "))
    }
}

/// 函数 `with_upstream_debug_suffix`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - message: 参数 message
/// - kind: 参数 kind
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn with_upstream_debug_suffix(
    message: Option<String>,
    kind: Option<&str>,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> Option<String> {
    let message = message?;
    let suffix = compact_debug_suffix(kind, request_id, cf_ray, auth_error, identity_error_code);
    if suffix.is_empty() {
        Some(message)
    } else {
        Some(format!("{message}{suffix}"))
    }
}

/// 函数 `should_suppress_deactivation_delivery`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - upstream_error_hint: 参数 upstream_error_hint
/// - allow_failover_for_deactivation: 参数 allow_failover_for_deactivation
///
/// # 返回
/// 返回函数执行结果
fn should_suppress_deactivation_delivery(
    upstream_error_hint: Option<&str>,
    allow_failover_for_deactivation: bool,
) -> bool {
    allow_failover_for_deactivation
        && upstream_error_hint.is_some_and(|message| {
            crate::account_status::deactivation_reason_from_message(message).is_some()
        })
}

/// 函数 `looks_like_blocked_marker`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn looks_like_blocked_marker(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.contains("blocked")
        || normalized.contains("unsupported_country_region_territory")
        || normalized.contains("unsupported_country")
        || normalized.contains("region_restricted")
}

fn body_as_trimmed_text(body: &[u8]) -> Option<&str> {
    std::str::from_utf8(body)
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn text_looks_like_html(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    normalized.contains("<html")
        || normalized.contains("<!doctype html")
        || normalized.contains("<body")
        || normalized.contains("</html>")
}

fn body_looks_like_html(body: &[u8]) -> bool {
    body_as_trimmed_text(body).is_some_and(text_looks_like_html)
}

fn body_looks_like_cloudflare_challenge(status_code: u16, body: &[u8]) -> bool {
    body_as_trimmed_text(body).is_some_and(|text| {
        let normalized = text.to_ascii_lowercase();
        let looks_like_challenge = normalized.contains("cloudflare")
            || normalized.contains("cf-chl")
            || normalized.contains("just a moment")
            || normalized.contains("attention required")
            || normalized.contains("captcha")
            || normalized.contains("security check")
            || normalized.contains("access denied")
            || normalized.contains("waf");
        looks_like_challenge || (text_looks_like_html(text) && matches!(status_code, 401 | 403))
    })
}

fn anthropic_usage_from_responses(value: &Value) -> Value {
    let usage = value.get("usage").cloned().unwrap_or(Value::Null);
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let cache_read_input_tokens = usage
        .get("input_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64);
    let reasoning_output_tokens = usage
        .get("output_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_i64);

    let mut obj = serde_json::Map::new();
    obj.insert("input_tokens".to_string(), Value::from(input_tokens));
    obj.insert("output_tokens".to_string(), Value::from(output_tokens));
    if let Some(value) = cache_read_input_tokens {
        obj.insert("cache_read_input_tokens".to_string(), Value::from(value));
    }
    if let Some(value) = reasoning_output_tokens {
        obj.insert("reasoning_output_tokens".to_string(), Value::from(value));
    }
    Value::Object(obj)
}

fn gemini_usage_from_responses(value: &Value) -> Value {
    let usage = value.get("usage").cloned().unwrap_or(Value::Null);
    let prompt = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let candidates = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let total = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(prompt + candidates);
    let cached = usage
        .get("input_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64);
    let reasoning = usage
        .get("output_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_i64);
    let mut obj = serde_json::Map::new();
    obj.insert("promptTokenCount".to_string(), Value::from(prompt));
    obj.insert("candidatesTokenCount".to_string(), Value::from(candidates));
    obj.insert("totalTokenCount".to_string(), Value::from(total));
    if let Some(value) = cached {
        obj.insert("cachedContentTokenCount".to_string(), Value::from(value));
    }
    if let Some(value) = reasoning {
        obj.insert("thoughtsTokenCount".to_string(), Value::from(value));
    }
    Value::Object(obj)
}

fn restore_tool_name(name: &str, tool_name_restore_map: Option<&ToolNameRestoreMap>) -> String {
    tool_name_restore_map
        .and_then(|map| map.get(name))
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn convert_responses_body_to_anthropic_messages(
    body: &[u8],
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_codexmanager");
    let model = value.get("model").and_then(Value::as_str).unwrap_or("");
    let mut content = Vec::new();
    let mut stop_reason = "end_turn";
    if let Some(output_items) = value.get("output").and_then(Value::as_array) {
        for item in output_items {
            let Some(item_obj) = item.as_object() else {
                continue;
            };
            match item_obj
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "reasoning" => {
                    let thinking = item_obj
                        .get("summary")
                        .and_then(Value::as_array)
                        .map(|parts| {
                            parts
                                .iter()
                                .filter_map(|part| part.get("text").and_then(Value::as_str))
                                .collect::<String>()
                        })
                        .filter(|text| !text.trim().is_empty())
                        .or_else(|| {
                            item_obj
                                .get("content")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                        .unwrap_or_default();
                    if !thinking.trim().is_empty() {
                        let mut block = json!({
                            "type": "thinking",
                            "thinking": thinking,
                        });
                        if let Some(signature) = item_obj
                            .get("encrypted_content")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                        {
                            block["signature"] = Value::String(signature.to_string());
                        }
                        content.push(block);
                    }
                }
                "message" => {
                    if let Some(parts) = item_obj.get("content").and_then(Value::as_array) {
                        for part in parts {
                            if matches!(
                                part.get("type").and_then(Value::as_str),
                                Some("output_text" | "text")
                            ) {
                                if let Some(text) = part.get("text").and_then(Value::as_str) {
                                    if !text.trim().is_empty() {
                                        content.push(json!({
                                            "type": "text",
                                            "text": text,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
                "function_call" | "custom_tool_call" => {
                    stop_reason = "tool_use";
                    content.push(json!({
                        "type": "tool_use",
                        "id": item_obj
                            .get("call_id")
                            .or_else(|| item_obj.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_unknown"),
                        "name": item_obj
                            .get("name")
                            .and_then(Value::as_str)
                            .map(|name| restore_tool_name(name, tool_name_restore_map))
                            .unwrap_or_else(|| "tool".to_string()),
                        "input": parse_json_string_or_value(
                            item_obj.get("arguments").or_else(|| item_obj.get("input"))
                        ),
                    }));
                }
                _ => {}
            }
        }
    }
    let payload = json!({
        "id": response_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": anthropic_usage_from_responses(&value),
    });
    serde_json::to_vec(&payload).ok()
}

fn convert_responses_body_to_gemini_generate_content(
    body: &[u8],
    wrap_response_envelope: bool,
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let mut parts = Vec::new();
    if let Some(output_items) = value.get("output").and_then(Value::as_array) {
        for item in output_items {
            let Some(item_obj) = item.as_object() else {
                continue;
            };
            match item_obj
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "reasoning" => {
                    let thinking = item_obj
                        .get("summary")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|part| part.get("text").and_then(Value::as_str))
                                .collect::<String>()
                        })
                        .filter(|text| !text.trim().is_empty());
                    if let Some(text) = thinking {
                        parts.push(json!({ "text": text, "thought": true }));
                    }
                }
                "message" => {
                    if let Some(content_items) = item_obj.get("content").and_then(Value::as_array) {
                        for content_item in content_items {
                            if matches!(
                                content_item.get("type").and_then(Value::as_str),
                                Some("output_text" | "text")
                            ) {
                                if let Some(text) = content_item.get("text").and_then(Value::as_str)
                                {
                                    if !text.trim().is_empty() {
                                        parts.push(json!({ "text": text }));
                                    }
                                }
                            }
                        }
                    }
                }
                "function_call" | "custom_tool_call" => {
                    let mut function_call = Map::new();
                    function_call.insert(
                        "name".to_string(),
                        Value::String(
                            item_obj
                                .get("name")
                                .and_then(Value::as_str)
                                .map(|name| restore_tool_name(name, tool_name_restore_map))
                                .unwrap_or_else(|| "tool".to_string()),
                        ),
                    );
                    function_call.insert(
                        "args".to_string(),
                        parse_json_string_or_value(
                            item_obj.get("arguments").or_else(|| item_obj.get("input")),
                        ),
                    );
                    let id_key = if item_obj.get("type").and_then(Value::as_str)
                        == Some("custom_tool_call")
                    {
                        "id"
                    } else {
                        "call_id"
                    };
                    if let Some(call_id) = item_obj
                        .get(id_key)
                        .or_else(|| item_obj.get("call_id"))
                        .or_else(|| item_obj.get("id"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|current| !current.is_empty())
                    {
                        function_call.insert("id".to_string(), Value::String(call_id.to_string()));
                    }
                    parts.push(json!({ "functionCall": Value::Object(function_call) }));
                }
                _ => {}
            }
        }
    }
    let mut payload = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": parts,
            },
            "finishReason": "STOP",
            "index": 0,
        }],
        "usageMetadata": gemini_usage_from_responses(&value),
    });
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        payload["modelVersion"] = Value::String(model.to_string());
    }
    if let Some(response_id) = value.get("id").and_then(Value::as_str) {
        payload["responseId"] = Value::String(response_id.to_string());
    }
    if let Some(create_time) = value
        .get("created_at")
        .and_then(Value::as_i64)
        .and_then(format_unix_timestamp_rfc3339)
    {
        payload["createTime"] = Value::String(create_time);
    }
    if let Some(function_calls) = build_gemini_function_calls(&parts) {
        payload["functionCalls"] = function_calls;
    }
    let body = if wrap_response_envelope {
        json!({ "response": payload })
    } else {
        payload
    };
    serde_json::to_vec(&body).ok()
}

fn build_gemini_function_calls(parts: &[Value]) -> Option<Value> {
    let mut function_calls = Vec::new();
    for part in parts {
        let Some(function_call) = part.get("functionCall").and_then(Value::as_object) else {
            continue;
        };
        let Some(name) = function_call
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|current| !current.is_empty())
        else {
            continue;
        };
        let mut item = Map::new();
        item.insert("name".to_string(), Value::String(name.to_string()));
        item.insert(
            "args".to_string(),
            function_call
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({})),
        );
        if let Some(call_id) = function_call
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|current| !current.is_empty())
        {
            item.insert("id".to_string(), Value::String(call_id.to_string()));
        }
        function_calls.push(Value::Object(item));
    }
    (!function_calls.is_empty()).then(|| Value::Array(function_calls))
}

fn format_unix_timestamp_rfc3339(seconds: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0).map(|value| value.to_rfc3339())
}

fn parse_json_string_or_value(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => {
            parse_json_string_lenient(text).unwrap_or_else(|| Value::String(text.clone()))
        }
        Some(other) => other.clone(),
        None => json!({}),
    }
}

fn parse_json_string_lenient(raw: &str) -> Option<Value> {
    let mut current = raw.trim().to_string();
    for _ in 0..3 {
        let parsed = serde_json::from_str::<Value>(&current).ok()?;
        if let Value::String(inner) = parsed {
            let trimmed = inner.trim();
            if trimmed.is_empty() || trimmed == current {
                return Some(Value::String(inner));
            }
            current = trimmed.to_string();
        } else {
            return Some(parsed);
        }
    }
    None
}

fn convert_upstream_error_to_anthropic_body(message: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "type": "error",
        "error": {
            "type": "api_error",
            "message": message,
        }
    }))
    .unwrap_or_else(|_| {
        b"{\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"unknown error\"}}"
            .to_vec()
    })
}

fn convert_upstream_error_to_gemini_body(message: &str) -> Vec<u8> {
    crate::gateway::build_gemini_error_body(message)
}

fn extract_error_message_from_json_bytes(body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    extract_error_message_from_json(&value)
}

fn replace_content_type_header(headers: &mut Vec<Header>, content_type: &str) {
    headers.retain(|header| {
        !header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("Content-Type")
    });
    if let Ok(header) = Header::from_bytes(b"Content-Type".as_slice(), content_type.as_bytes()) {
        headers.push(header);
    }
}

fn force_openai_responses_stream_content_type(
    headers: &mut Vec<Header>,
    request_path: &str,
    is_stream: bool,
) {
    if is_stream && request_path.starts_with("/v1/responses") {
        replace_content_type_header(headers, "text/event-stream");
    }
}

fn convert_success_body_for_adapter(
    response_adapter: ResponseAdapter,
    body: &[u8],
    _request_path: &str,
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
) -> Option<Vec<u8>> {
    match response_adapter {
        ResponseAdapter::AnthropicMessagesFromResponses => {
            convert_responses_body_to_anthropic_messages(body, tool_name_restore_map)
        }
        ResponseAdapter::ChatCompletionsFromResponses => {
            convert_responses_body_to_chat_completions(body)
        }
        ResponseAdapter::CompactFromChatCompletions => {
            convert_chat_completions_body_to_compact(body)
        }
        ResponseAdapter::ImagesB64JsonFromResponses => {
            convert_responses_body_to_images(body, ImagesResponseFormat::B64Json)
        }
        ResponseAdapter::ImagesUrlFromResponses => {
            convert_responses_body_to_images(body, ImagesResponseFormat::Url)
        }
        ResponseAdapter::GeminiJson => {
            convert_responses_body_to_gemini_generate_content(body, false, tool_name_restore_map)
        }
        ResponseAdapter::GeminiCliJson => {
            convert_responses_body_to_gemini_generate_content(body, true, tool_name_restore_map)
        }
        ResponseAdapter::GeminiSse | ResponseAdapter::GeminiCliSse => None,
        ResponseAdapter::Passthrough => None,
    }
}

fn collect_chat_output_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(text),
        Value::Array(items) => {
            for item in items {
                collect_chat_output_text(item, out);
            }
        }
        Value::Object(obj) => {
            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
            if let Some(content) = obj.get("content") {
                collect_chat_output_text(content, out);
            }
            if let Some(output) = obj.get("output") {
                collect_chat_output_text(output, out);
            }
        }
        _ => {}
    }
}

fn responses_usage_to_chat_usage(usage: &Value) -> Value {
    let prompt_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(prompt_tokens + completion_tokens);
    let mut mapped = json!({
        "prompt_tokens": prompt_tokens.max(0),
        "completion_tokens": completion_tokens.max(0),
        "total_tokens": total_tokens.max(0)
    });
    if let Some(details) = usage
        .get("prompt_tokens_details")
        .or_else(|| usage.get("input_tokens_details"))
    {
        mapped["prompt_tokens_details"] = details.clone();
    }
    if let Some(details) = usage
        .get("completion_tokens_details")
        .or_else(|| usage.get("output_tokens_details"))
    {
        mapped["completion_tokens_details"] = details.clone();
    }
    mapped
}

fn convert_responses_body_to_chat_completions(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response = value.get("response").unwrap_or(&value);
    let mut text = String::new();
    if let Some(output_text) = response.get("output_text").and_then(Value::as_str) {
        text.push_str(output_text);
    }
    if text.is_empty() {
        if let Some(output) = response.get("output") {
            collect_chat_output_text(output, &mut text);
        }
    }
    let id = response
        .get("id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl_codexmanager");
    let model = response
        .get("model")
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)
        .unwrap_or("gpt-5.4");
    let created = response
        .get("created_at")
        .or_else(|| response.get("created"))
        .or_else(|| value.get("created_at"))
        .or_else(|| value.get("created"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let usage = response
        .get("usage")
        .or_else(|| value.get("usage"))
        .map(responses_usage_to_chat_usage);
    let mut completion = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": "stop"
        }]
    });
    if let Some(usage) = usage {
        completion["usage"] = usage;
    }
    let images = collect_image_generation_chat_images(response);
    if !images.is_empty() {
        completion["choices"][0]["message"]["images"] = Value::Array(images);
    }
    serde_json::to_vec(&completion).ok()
}

fn collect_chat_completion_message_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(text),
        Value::Array(items) => {
            for item in items {
                collect_chat_completion_message_text(item, out);
            }
        }
        Value::Object(obj) => {
            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                out.push_str(text);
            } else if let Some(content) = obj.get("content") {
                collect_chat_completion_message_text(content, out);
            }
        }
        _ => {}
    }
}

fn convert_chat_completions_body_to_compact(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let mut text = String::new();
    if let Some(message_content) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    {
        collect_chat_completion_message_text(message_content, &mut text);
    }
    if text.trim().is_empty() {
        return None;
    }
    serde_json::to_vec(&json!({
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        }]
    }))
    .ok()
}

fn convert_responses_body_to_images(
    body: &[u8],
    response_format: ImagesResponseFormat,
) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response = value.get("response").unwrap_or(&value);
    serde_json::to_vec(&build_images_api_response(response, response_format)).ok()
}

fn images_response_body_to_sse(body: &[u8], response_format: ImagesResponseFormat) -> Vec<u8> {
    let value = serde_json::from_slice::<Value>(body).unwrap_or_else(|_| json!({}));
    let response = value.get("response").unwrap_or(&value);
    let api_response = build_images_api_response(response, response_format);
    let mut out = Vec::new();
    if let Some(items) = api_response.get("data").and_then(Value::as_array) {
        for item in items {
            let mut payload = item.clone();
            if let Some(payload_obj) = payload.as_object_mut() {
                payload_obj.insert(
                    "type".to_string(),
                    Value::String("image_generation.completed".to_string()),
                );
                if let Some(usage) = api_response.get("usage") {
                    payload_obj.insert("usage".to_string(), usage.clone());
                }
            }
            out.extend_from_slice(
                format!(
                    "event: image_generation.completed\ndata: {}\n\n",
                    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
                )
                .as_bytes(),
            );
        }
    }
    out
}

fn chat_completion_body_to_single_sse(body: &[u8]) -> Vec<u8> {
    let value = serde_json::from_slice::<Value>(body).unwrap_or_else(|_| json!({}));
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl_codexmanager");
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("gpt-5.4");
    let created = value.get("created").and_then(Value::as_i64).unwrap_or(0);
    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let usage = value.get("usage").cloned();
    let chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": { "content": content },
            "finish_reason": null
        }]
    });
    let mut final_chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    });
    if let Some(usage) = usage {
        final_chunk["usage"] = usage;
    }
    format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        serde_json::to_string(&chunk).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string(&final_chunk).unwrap_or_else(|_| "{}".to_string())
    )
    .into_bytes()
}

fn convert_error_body_for_adapter(response_adapter: ResponseAdapter, message: &str) -> Vec<u8> {
    match response_adapter {
        ResponseAdapter::AnthropicMessagesFromResponses => {
            convert_upstream_error_to_anthropic_body(message)
        }
        ResponseAdapter::ChatCompletionsFromResponses => serde_json::to_vec(&json!({
            "error": {
                "message": message,
                "type": "upstream_error",
                "code": "upstream_error"
            }
        }))
        .unwrap_or_else(|_| message.as_bytes().to_vec()),
        ResponseAdapter::CompactFromChatCompletions => serde_json::to_vec(&json!({
            "error": {
                "message": message,
                "type": "upstream_error",
                "code": "upstream_error"
            }
        }))
        .unwrap_or_else(|_| message.as_bytes().to_vec()),
        ResponseAdapter::ImagesB64JsonFromResponses | ResponseAdapter::ImagesUrlFromResponses => {
            serde_json::to_vec(&json!({
                "error": {
                    "message": message,
                    "type": "upstream_error",
                    "code": "upstream_error"
                }
            }))
            .unwrap_or_else(|_| message.as_bytes().to_vec())
        }
        ResponseAdapter::GeminiJson
        | ResponseAdapter::GeminiCliJson
        | ResponseAdapter::GeminiSse
        | ResponseAdapter::GeminiCliSse => convert_upstream_error_to_gemini_body(message),
        ResponseAdapter::Passthrough => message.as_bytes().to_vec(),
    }
}

fn compatibility_stream_content_type(
    response_adapter: ResponseAdapter,
    gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
) -> &'static str {
    match response_adapter {
        ResponseAdapter::AnthropicMessagesFromResponses => "text/event-stream",
        ResponseAdapter::ChatCompletionsFromResponses => "text/event-stream",
        ResponseAdapter::CompactFromChatCompletions => "application/json",
        ResponseAdapter::ImagesB64JsonFromResponses | ResponseAdapter::ImagesUrlFromResponses => {
            "text/event-stream"
        }
        ResponseAdapter::GeminiJson | ResponseAdapter::GeminiCliJson => "application/json",
        ResponseAdapter::GeminiSse | ResponseAdapter::GeminiCliSse => {
            match gemini_stream_output_mode {
                Some(GeminiStreamOutputMode::Raw) => "application/json",
                _ => "text/event-stream",
            }
        }
        ResponseAdapter::Passthrough => "text/event-stream",
    }
}

fn gemini_cli_wrap_response_envelope(response_adapter: ResponseAdapter) -> bool {
    matches!(
        response_adapter,
        ResponseAdapter::GeminiCliJson | ResponseAdapter::GeminiCliSse
    )
}

/// 函数 `classify_compact_invalid_success_kind`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn classify_compact_invalid_success_kind(
    body: &[u8],
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> &'static str {
    if auth_error.is_some_and(looks_like_blocked_marker)
        || identity_error_code.is_some_and(looks_like_blocked_marker)
    {
        return "cloudflare_blocked";
    }
    if body_looks_like_cloudflare_challenge(502, body) {
        return "cloudflare_challenge";
    }
    if body_looks_like_html(body) {
        return "html";
    }
    if identity_error_code.is_some() {
        return "identity_error";
    }
    if auth_error.is_some() {
        return "auth_error";
    }
    if cf_ray.is_some() {
        return "cloudflare_edge";
    }
    if serde_json::from_slice::<Value>(body).is_ok() {
        "invalid_success_body"
    } else if body.is_empty() {
        "empty"
    } else {
        "non_json"
    }
}

/// 函数 `classify_compact_non_success_kind`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status_code: 参数 status_code
/// - content_type: 参数 content_type
/// - body: 参数 body
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn classify_compact_non_success_kind(
    status_code: u16,
    content_type: Option<&str>,
    body: &[u8],
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> &'static str {
    if auth_error.is_some_and(looks_like_blocked_marker)
        || identity_error_code.is_some_and(looks_like_blocked_marker)
    {
        return "cloudflare_blocked";
    }
    if body_looks_like_cloudflare_challenge(status_code, body) {
        return "cloudflare_challenge";
    }
    if body_looks_like_html(body) {
        return "html";
    }
    if content_type
        .map(crate::gateway::is_html_content_type)
        .unwrap_or(false)
    {
        return "html";
    }
    if identity_error_code.is_some() {
        return "identity_error";
    }
    if auth_error.is_some() {
        return "auth_error";
    }
    if cf_ray.is_some() {
        return "cloudflare_edge";
    }
    if serde_json::from_slice::<Value>(body).is_ok() {
        "json_error"
    } else if body.is_empty() {
        "empty"
    } else {
        "non_json"
    }
}

/// 函数 `compact_success_body_is_valid`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn compact_success_body_is_valid(body: &[u8]) -> bool {
    serde_json::from_slice::<CompactHistoryResponse>(body).is_ok()
}

#[derive(Debug, Deserialize)]
struct CompactHistoryResponse {
    #[allow(dead_code)]
    output: Vec<ResponseItem>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponseItem {
    Message {
        #[allow(dead_code)]
        role: String,
        #[allow(dead_code)]
        content: Vec<Value>,
    },
    Reasoning {
        #[allow(dead_code)]
        summary: Vec<Value>,
        #[allow(dead_code)]
        encrypted_content: Option<String>,
        #[allow(dead_code)]
        content: Option<Vec<Value>>,
    },
    LocalShellCall {
        #[allow(dead_code)]
        call_id: Option<String>,
        #[allow(dead_code)]
        status: Value,
        #[allow(dead_code)]
        action: Value,
    },
    FunctionCall {
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        namespace: Option<String>,
        #[allow(dead_code)]
        arguments: String,
        #[allow(dead_code)]
        call_id: String,
    },
    ToolSearchCall {
        #[allow(dead_code)]
        call_id: Option<String>,
        #[allow(dead_code)]
        status: Option<String>,
        #[allow(dead_code)]
        execution: String,
        #[allow(dead_code)]
        arguments: Value,
    },
    FunctionCallOutput {
        #[allow(dead_code)]
        call_id: String,
        #[allow(dead_code)]
        output: Value,
    },
    CustomToolCall {
        #[allow(dead_code)]
        status: Option<String>,
        #[allow(dead_code)]
        call_id: String,
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        input: String,
    },
    CustomToolCallOutput {
        #[allow(dead_code)]
        call_id: String,
        #[allow(dead_code)]
        name: Option<String>,
        #[allow(dead_code)]
        output: Value,
    },
    ToolSearchOutput {
        #[allow(dead_code)]
        call_id: Option<String>,
        #[allow(dead_code)]
        status: String,
        #[allow(dead_code)]
        execution: String,
        #[allow(dead_code)]
        tools: Vec<Value>,
    },
    WebSearchCall {
        #[allow(dead_code)]
        status: Option<String>,
        #[allow(dead_code)]
        action: Option<Value>,
    },
    ImageGenerationCall {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        status: String,
        #[allow(dead_code)]
        revised_prompt: Option<String>,
        #[allow(dead_code)]
        result: String,
    },
    #[serde(alias = "compaction_summary")]
    Compaction {
        #[allow(dead_code)]
        encrypted_content: String,
    },
    CompactionTrigger,
    ContextCompaction {
        #[allow(dead_code)]
        encrypted_content: Option<String>,
    },
    #[serde(other)]
    Other,
}

/// 函数 `build_invalid_compact_success_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn build_invalid_compact_success_message(
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> String {
    let kind = classify_compact_invalid_success_kind(body, cf_ray, auth_error, identity_error_code);
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(message) = extract_error_message_from_json(&value) {
            return format!(
                "invalid upstream compact response: {message}{}",
                compact_debug_suffix(
                    Some(kind),
                    request_id,
                    cf_ray,
                    auth_error,
                    identity_error_code
                )
            );
        }
    }
    if let Some(hint) = extract_error_hint_from_body(502, body) {
        return format!(
            "invalid upstream compact response: {hint}{}",
            compact_debug_suffix(
                Some(kind),
                request_id,
                cf_ray,
                auth_error,
                identity_error_code
            )
        );
    }
    format!(
        "invalid upstream compact response: cannot parse compact output{}",
        compact_debug_suffix(
            Some(kind),
            request_id,
            cf_ray,
            auth_error,
            identity_error_code
        )
    )
}

/// 函数 `compact_non_success_body_should_be_normalized`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status_code: 参数 status_code
/// - content_type: 参数 content_type
/// - body: 参数 body
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn non_success_body_should_be_normalized(
    status_code: u16,
    content_type: Option<&str>,
    body: &[u8],
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> bool {
    if status_code < 400 {
        return false;
    }
    if auth_error
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
        || identity_error_code
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    {
        return true;
    }
    if content_type
        .map(crate::gateway::is_html_content_type)
        .unwrap_or(false)
    {
        return true;
    }
    body_looks_like_cloudflare_challenge(status_code, body) || body_looks_like_html(body)
}

fn compact_non_success_body_should_be_normalized(
    status_code: u16,
    content_type: Option<&str>,
    body: &[u8],
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> bool {
    non_success_body_should_be_normalized(
        status_code,
        content_type,
        body,
        auth_error,
        identity_error_code,
    )
}

fn build_passthrough_non_success_message(
    status_code: u16,
    content_type: Option<&str>,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> String {
    let kind = classify_compact_non_success_kind(
        status_code,
        content_type,
        body,
        cf_ray,
        auth_error,
        identity_error_code,
    );
    if let Some(hint) = extract_error_hint_from_body(status_code, body).or_else(|| {
        header_only_cloudflare_challenge_hint(
            status_code,
            content_type,
            body,
            cf_ray,
            auth_error,
            identity_error_code,
        )
    }) {
        return format!(
            "upstream server error: {hint}{}",
            compact_debug_suffix(
                Some(kind),
                request_id,
                cf_ray,
                auth_error,
                identity_error_code
            )
        );
    }
    format!(
        "upstream server error: status={status_code}{}",
        compact_debug_suffix(
            Some(kind),
            request_id,
            cf_ray,
            auth_error,
            identity_error_code
        )
    )
}

fn header_only_cloudflare_challenge_hint(
    status_code: u16,
    content_type: Option<&str>,
    body: &[u8],
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> Option<String> {
    if status_code < 400 || !body.is_empty() {
        return None;
    }
    if auth_error.is_some_and(looks_like_blocked_marker)
        || identity_error_code.is_some_and(looks_like_blocked_marker)
    {
        return Some("Cloudflare 安全验证页".to_string());
    }
    let is_html = content_type
        .map(crate::gateway::is_html_content_type)
        .unwrap_or(false);
    if cf_ray.is_some() || (is_html && matches!(status_code, 401 | 403)) {
        return Some("Cloudflare 安全验证页".to_string());
    }
    None
}

fn extract_error_hint_from_body_or_headers(
    status_code: u16,
    content_type: Option<&str>,
    body: &[u8],
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> Option<String> {
    extract_error_hint_from_body(status_code, body).or_else(|| {
        header_only_cloudflare_challenge_hint(
            status_code,
            content_type,
            body,
            cf_ray,
            auth_error,
            identity_error_code,
        )
    })
}

/// 函数 `respond_synthesized_compact_error_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
/// - status_code: 参数 status_code
/// - usage: 参数 usage
/// - message: 参数 message
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - trace_id: 参数 trace_id
///
/// # 返回
/// 返回函数执行结果
fn respond_synthesized_compact_error_body(
    request: Request,
    status_code: u16,
    usage: UpstreamResponseUsage,
    message: String,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    let response_message = crate::gateway::error_message_for_client(
        crate::gateway::prefers_raw_errors_for_tiny_http_request(&request),
        message.as_str(),
    );
    let response = crate::gateway::error_response::terminal_text_response(
        status_code,
        response_message,
        trace_id,
    );
    let delivery_error = request.respond(response).err().map(|err| err.to_string());
    UpstreamResponseBridgeResult {
        usage,
        stream_terminal_seen: true,
        stream_terminal_error: None,
        delivery_error,
        upstream_error_hint: Some(message),
        delivered_status_code: Some(status_code),
        upstream_request_id: request_id.map(str::to_string),
        upstream_cf_ray: cf_ray.map(str::to_string),
        upstream_auth_error: None,
        upstream_identity_error_code: None,
        upstream_content_type: Some("application/json".to_string()),
        last_sse_event_type: None,
    }
}

/// 函数 `with_bridge_debug_meta`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - result: 参数 result
/// - upstream_request_id: 参数 upstream_request_id
/// - upstream_cf_ray: 参数 upstream_cf_ray
/// - upstream_auth_error: 参数 upstream_auth_error
/// - upstream_identity_error_code: 参数 upstream_identity_error_code
/// - upstream_content_type: 参数 upstream_content_type
/// - last_sse_event_type: 参数 last_sse_event_type
///
/// # 返回
/// 返回函数执行结果
fn with_bridge_debug_meta(
    mut result: UpstreamResponseBridgeResult,
    upstream_request_id: &Option<String>,
    upstream_cf_ray: &Option<String>,
    upstream_auth_error: &Option<String>,
    upstream_identity_error_code: &Option<String>,
    upstream_content_type: &Option<String>,
    last_sse_event_type: Option<String>,
) -> UpstreamResponseBridgeResult {
    result.upstream_request_id = upstream_request_id.clone();
    result.upstream_cf_ray = upstream_cf_ray.clone();
    result.upstream_auth_error = upstream_auth_error.clone();
    result.upstream_identity_error_code = upstream_identity_error_code.clone();
    result.upstream_content_type = upstream_content_type.clone();
    result.last_sse_event_type = last_sse_event_type;
    result
}

fn log_bridge_stream_diagnostics(
    response_adapter: ResponseAdapter,
    request_path: &str,
    result: &UpstreamResponseBridgeResult,
) {
    if result.delivery_error.is_none()
        && result.stream_terminal_seen
        && result.stream_terminal_error.is_none()
    {
        return;
    }

    log::warn!(
        "event=gateway_bridge_stream_diagnostics adapter={:?} path={} stream_terminal_seen={} stream_terminal_error={} delivery_error={} upstream_error_hint={} last_sse_event_type={} upstream_request_id={} upstream_cf_ray={} upstream_content_type={}",
        response_adapter,
        request_path,
        if result.stream_terminal_seen { "true" } else { "false" },
        result.stream_terminal_error.as_deref().unwrap_or("-"),
        result.delivery_error.as_deref().unwrap_or("-"),
        result.upstream_error_hint.as_deref().unwrap_or("-"),
        result.last_sse_event_type.as_deref().unwrap_or("-"),
        result.upstream_request_id.as_deref().unwrap_or("-"),
        result.upstream_cf_ray.as_deref().unwrap_or("-"),
        result.upstream_content_type.as_deref().unwrap_or("-"),
    );
}

/// 函数 `respond_invalid_compact_success_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
/// - usage: 参数 usage
/// - body: 参数 body
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
/// - trace_id: 参数 trace_id
///
/// # 返回
/// 返回函数执行结果
fn respond_invalid_compact_success_body(
    request: Request,
    usage: UpstreamResponseUsage,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    with_bridge_debug_meta(
        respond_synthesized_compact_error_body(
            request,
            502,
            usage,
            build_invalid_compact_success_message(
                body,
                request_id,
                cf_ray,
                auth_error,
                identity_error_code,
            ),
            request_id,
            cf_ray,
            trace_id,
        ),
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        &Some("application/json".to_string()),
        None,
    )
}

fn respond_compact_success_body(
    request: Request,
    status: StatusCode,
    headers: Vec<Header>,
    usage: UpstreamResponseUsage,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    content_type: &Option<String>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    if !compact_success_body_is_valid(body) {
        return respond_invalid_compact_success_body(
            request,
            usage,
            body,
            request_id,
            cf_ray,
            auth_error,
            identity_error_code,
            trace_id,
        );
    }

    let len = Some(body.len());
    let response = Response::new(
        status,
        headers,
        std::io::Cursor::new(body.to_vec()),
        len,
        None,
    );
    let delivery_error = request.respond(response).err().map(|err| err.to_string());
    with_bridge_debug_meta(
        UpstreamResponseBridgeResult {
            usage,
            stream_terminal_seen: true,
            stream_terminal_error: None,
            delivery_error,
            upstream_error_hint: None,
            delivered_status_code: None,
            upstream_request_id: None,
            upstream_cf_ray: None,
            upstream_auth_error: None,
            upstream_identity_error_code: None,
            upstream_content_type: None,
            last_sse_event_type: None,
        },
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        content_type,
        None,
    )
}

/// 函数 `respond_invalid_compact_non_success_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
/// - status_code: 参数 status_code
/// - usage: 参数 usage
/// - body: 参数 body
/// - content_type: 参数 content_type
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
/// - trace_id: 参数 trace_id
///
/// # 返回
/// 返回函数执行结果
fn respond_invalid_compact_non_success_body(
    request: Request,
    status_code: u16,
    usage: UpstreamResponseUsage,
    body: &[u8],
    content_type: Option<&str>,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    let gateway_status_code = 502;
    let request_method = request.method().as_str().to_string();
    let request_path = request.url().to_string();
    let error_kind = classify_compact_non_success_kind(
        status_code,
        content_type,
        body,
        cf_ray,
        auth_error,
        identity_error_code,
    );
    let message = build_passthrough_non_success_message(
        status_code,
        content_type,
        body,
        request_id,
        cf_ray,
        auth_error,
        identity_error_code,
    );
    crate::gateway::write_gateway_error_log(GatewayErrorLogInput {
        trace_id,
        request_path: request_path.as_str(),
        method: request_method.as_str(),
        stage: "compact_bridge_non_success",
        error_kind: Some(error_kind),
        cf_ray,
        status_code: Some(gateway_status_code),
        compression_enabled: false,
        compression_retry_attempted: false,
        message: message.as_str(),
        ..GatewayErrorLogInput::default()
    });

    with_bridge_debug_meta(
        respond_synthesized_compact_error_body(
            request,
            gateway_status_code,
            usage,
            message,
            request_id,
            cf_ray,
            trace_id,
        ),
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        &Some("application/json".to_string()),
        None,
    )
}

fn respond_normalized_passthrough_non_success_body(
    request: Request,
    usage: UpstreamResponseUsage,
    body: &[u8],
    content_type: Option<&str>,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    let request_method = request.method().as_str().to_string();
    let request_path = request.url().to_string();
    let error_kind = classify_compact_non_success_kind(
        502,
        content_type,
        body,
        cf_ray,
        auth_error,
        identity_error_code,
    );
    let message = build_passthrough_non_success_message(
        502,
        content_type,
        body,
        request_id,
        cf_ray,
        auth_error,
        identity_error_code,
    );
    crate::gateway::write_gateway_error_log(GatewayErrorLogInput {
        trace_id,
        request_path: request_path.as_str(),
        method: request_method.as_str(),
        stage: "passthrough_bridge_non_success",
        error_kind: Some(error_kind),
        cf_ray,
        status_code: Some(502),
        compression_enabled: false,
        compression_retry_attempted: false,
        message: message.as_str(),
        ..GatewayErrorLogInput::default()
    });

    with_bridge_debug_meta(
        respond_synthesized_compact_error_body(
            request, 502, usage, message, request_id, cf_ray, trace_id,
        ),
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        &Some("application/json".to_string()),
        None,
    )
}

/// 函数 `respond_with_upstream`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn respond_with_upstream(
    request: Request,
    upstream: reqwest::blocking::Response,
    _inflight_guard: super::super::AccountInFlightGuard,
    response_adapter: ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
    is_stream: bool,
    allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    let keepalive_frame = resolve_stream_keepalive_frame(response_adapter, request_path);
    let passthrough_sse_protocol =
        passthrough_sse_protocol.unwrap_or(PassthroughSseProtocol::Generic);
    let upstream_request_id =
        first_upstream_header(upstream.headers(), REQUEST_ID_HEADER_CANDIDATES);
    let upstream_cf_ray = first_upstream_header(upstream.headers(), &[CF_RAY_HEADER_NAME]);
    let upstream_auth_error = first_upstream_header(upstream.headers(), &[AUTH_ERROR_HEADER_NAME]);
    let upstream_identity_error_code =
        crate::gateway::extract_identity_error_code_from_headers(upstream.headers());
    let upstream_content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    if response_adapter != ResponseAdapter::Passthrough {
        let status = StatusCode(upstream.status().as_u16());
        let mut headers = Vec::new();
        for (name, value) in upstream.headers().iter() {
            let name_str = name.as_str();
            if name_str.eq_ignore_ascii_case("transfer-encoding")
                || name_str.eq_ignore_ascii_case("content-length")
                || name_str.eq_ignore_ascii_case("connection")
            {
                continue;
            }
            if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                headers.push(header);
            }
        }
        if let Some(trace_id) = trace_id {
            push_trace_id_header(&mut headers, trace_id);
        }
        let is_sse = upstream_content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
            .unwrap_or(false);
        let is_json = upstream_content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains("application/json"))
            .unwrap_or(false);

        if !is_stream {
            let upstream_body = upstream
                .bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let detected_sse =
                is_sse || (!is_json && looks_like_sse_payload(upstream_body.as_ref()));
            let (body, usage) = if detected_sse {
                let (synthesized, mut usage) =
                    collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let body = synthesized.unwrap_or_else(|| upstream_body.to_vec());
                if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                    merge_usage(&mut usage, parse_usage_from_json(&value));
                }
                (body, usage)
            } else {
                let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                    .ok()
                    .map(|value| parse_usage_from_json(&value))
                    .unwrap_or_default();
                (upstream_body.to_vec(), usage)
            };
            let response_body = if status.0 >= 400 {
                let message = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        &body,
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                    .or_else(|| extract_error_message_from_json_bytes(&body)),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                )
                .unwrap_or_else(|| "upstream compatibility bridge failed".to_string());
                convert_error_body_for_adapter(response_adapter, &message)
            } else {
                convert_success_body_for_adapter(
                    response_adapter,
                    &body,
                    request_path,
                    tool_name_restore_map,
                )
                .unwrap_or_else(|| body.clone())
            };
            replace_content_type_header(&mut headers, "application/json");
            let len = Some(response_body.len());
            let response = Response::new(
                status,
                headers,
                std::io::Cursor::new(response_body),
                len,
                None,
            );
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            return Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint: None,
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ));
        }

        if status.0 >= 400 && !is_sse {
            let upstream_body = upstream
                .bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let message = with_upstream_debug_suffix(
                extract_error_hint_from_body_or_headers(
                    status.0,
                    upstream_content_type.as_deref(),
                    upstream_body.as_ref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                )
                .or_else(|| extract_error_message_from_json_bytes(upstream_body.as_ref())),
                None,
                upstream_request_id.as_deref(),
                upstream_cf_ray.as_deref(),
                upstream_auth_error.as_deref(),
                upstream_identity_error_code.as_deref(),
            )
            .unwrap_or_else(|| "upstream compatibility bridge failed".to_string());
            let response_body = convert_error_body_for_adapter(response_adapter, &message);
            replace_content_type_header(&mut headers, "application/json");
            let len = Some(response_body.len());
            let response = Response::new(
                status,
                headers,
                std::io::Cursor::new(response_body),
                len,
                None,
            );
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            return Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage: UpstreamResponseUsage::default(),
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint: Some(message),
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ));
        }

        replace_content_type_header(
            &mut headers,
            compatibility_stream_content_type(response_adapter, gemini_stream_output_mode),
        );
        match response_adapter {
            ResponseAdapter::AnthropicMessagesFromResponses => {
                let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
                let response_body: Box<dyn std::io::Read + Send> =
                    Box::new(AnthropicSseReader::new(
                        upstream,
                        Arc::clone(&usage_collector),
                        fallback_model,
                        tool_name_restore_map.cloned(),
                        request_started_at,
                    ));
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let usage = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint: None,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::ChatCompletionsFromResponses => {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<dyn std::io::Read + Send> =
                    Box::new(ChatCompletionsFromResponsesSseReader::new(
                        upstream,
                        Arc::clone(&usage_collector),
                        request_started_at,
                    ));
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage: collector.usage,
                        stream_terminal_seen: collector.saw_terminal,
                        stream_terminal_error: collector.terminal_error,
                        delivery_error,
                        upstream_error_hint: collector.upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: collector.last_event_type,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::CompactFromChatCompletions => unreachable!(),
            ResponseAdapter::ImagesB64JsonFromResponses
            | ResponseAdapter::ImagesUrlFromResponses => {
                let response_format = if response_adapter == ResponseAdapter::ImagesUrlFromResponses
                {
                    ImagesResponseFormat::Url
                } else {
                    ImagesResponseFormat::B64Json
                };
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<dyn std::io::Read + Send> =
                    Box::new(ImagesFromResponsesSseReader::new(
                        upstream,
                        Arc::clone(&usage_collector),
                        request_started_at,
                        response_format,
                    ));
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage: collector.usage,
                        stream_terminal_seen: collector.saw_terminal,
                        stream_terminal_error: collector.terminal_error,
                        delivery_error,
                        upstream_error_hint: collector.upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: collector.last_event_type,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::GeminiJson | ResponseAdapter::GeminiCliJson => unreachable!(),
            ResponseAdapter::GeminiSse | ResponseAdapter::GeminiCliSse => {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<dyn std::io::Read + Send> = Box::new(GeminiSseReader::new(
                    upstream,
                    Arc::clone(&usage_collector),
                    tool_name_restore_map.cloned(),
                    gemini_stream_output_mode.unwrap_or(GeminiStreamOutputMode::Sse),
                    gemini_cli_wrap_response_envelope(response_adapter),
                    request_started_at,
                ));
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage: collector.usage,
                        stream_terminal_seen: collector.saw_terminal,
                        stream_terminal_error: collector.terminal_error,
                        delivery_error,
                        upstream_error_hint: collector.upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: collector.last_event_type,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::Passthrough => {}
        }
    }
    match response_adapter {
        ResponseAdapter::Passthrough => {
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = Vec::new();
            for (name, value) in upstream.headers().iter() {
                let name_str = name.as_str();
                if name_str.eq_ignore_ascii_case("transfer-encoding")
                    || name_str.eq_ignore_ascii_case("content-length")
                    || name_str.eq_ignore_ascii_case("connection")
                {
                    continue;
                }
                if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                    headers.push(header);
                }
            }
            if let Some(trace_id) = trace_id {
                push_trace_id_header(&mut headers, trace_id);
            }
            let is_json = upstream_content_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase().contains("application/json"))
                .unwrap_or(false);
            let is_sse = upstream_content_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
                .unwrap_or(false);
            if !is_stream {
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let detected_sse =
                    is_sse || (!is_json && looks_like_sse_payload(upstream_body.as_ref()));
                let is_compact_request = is_compact_request_path(request_path);
                if detected_sse {
                    let (synthesized_body, mut usage) =
                        collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                    let synthesized_response = synthesized_body.is_some();
                    let body = synthesized_body.unwrap_or_else(|| upstream_body.to_vec());
                    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                        merge_usage(&mut usage, parse_usage_from_json(&value));
                    }
                    let upstream_error_hint = with_upstream_debug_suffix(
                        extract_error_hint_from_body_or_headers(
                            status.0,
                            upstream_content_type.as_deref(),
                            &body,
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        ),
                        None,
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    );
                    if should_suppress_deactivation_delivery(
                        upstream_error_hint.as_deref(),
                        allow_failover_for_deactivation,
                    ) {
                        return Ok(with_bridge_debug_meta(
                            UpstreamResponseBridgeResult {
                                usage,
                                stream_terminal_seen: true,
                                stream_terminal_error: None,
                                delivery_error: None,
                                upstream_error_hint,
                                delivered_status_code: None,
                                upstream_request_id: None,
                                upstream_cf_ray: None,
                                upstream_auth_error: None,
                                upstream_identity_error_code: None,
                                upstream_content_type: None,
                                last_sse_event_type: None,
                            },
                            &upstream_request_id,
                            &upstream_cf_ray,
                            &upstream_auth_error,
                            &upstream_identity_error_code,
                            &upstream_content_type,
                            None,
                        ));
                    }
                    if synthesized_response {
                        headers.retain(|header| {
                            !header
                                .field
                                .as_str()
                                .as_str()
                                .eq_ignore_ascii_case("Content-Type")
                        });
                        if let Ok(content_type_header) = Header::from_bytes(
                            b"Content-Type".as_slice(),
                            b"application/json".as_slice(),
                        ) {
                            headers.push(content_type_header);
                        }
                    }
                    if status.0 < 400
                        && is_compact_request
                        && !compact_success_body_is_valid(body.as_ref())
                    {
                        return Ok(respond_invalid_compact_success_body(
                            request,
                            usage,
                            body.as_ref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        ));
                    }
                    if is_compact_request
                        && compact_non_success_body_should_be_normalized(
                            status.0,
                            upstream_content_type.as_deref(),
                            body.as_ref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        )
                    {
                        return Ok(respond_invalid_compact_non_success_body(
                            request,
                            status.0,
                            usage,
                            body.as_ref(),
                            upstream_content_type.as_deref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        ));
                    }
                    if status.0 >= 400
                        && non_success_body_should_be_normalized(
                            status.0,
                            upstream_content_type.as_deref(),
                            body.as_ref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        )
                    {
                        return Ok(respond_normalized_passthrough_non_success_body(
                            request,
                            usage,
                            body.as_ref(),
                            upstream_content_type.as_deref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        ));
                    }
                    let len = Some(body.len());
                    let response =
                        Response::new(status, headers, std::io::Cursor::new(body), len, None);
                    let delivery_error = request.respond(response).err().map(|err| err.to_string());
                    return Ok(with_bridge_debug_meta(
                        UpstreamResponseBridgeResult {
                            usage,
                            stream_terminal_seen: true,
                            stream_terminal_error: None,
                            delivery_error,
                            upstream_error_hint,
                            delivered_status_code: None,
                            upstream_request_id: None,
                            upstream_cf_ray: None,
                            upstream_auth_error: None,
                            upstream_identity_error_code: None,
                            upstream_content_type: None,
                            last_sse_event_type: None,
                        },
                        &upstream_request_id,
                        &upstream_cf_ray,
                        &upstream_auth_error,
                        &upstream_identity_error_code,
                        &upstream_content_type,
                        None,
                    ));
                }

                let (_, sse_usage) = collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else if usage_has_signal(&sse_usage) {
                    sse_usage
                } else {
                    UpstreamResponseUsage::default()
                };
                if status.0 < 400
                    && is_compact_request
                    && !compact_success_body_is_valid(upstream_body.as_ref())
                {
                    return Ok(respond_invalid_compact_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    ));
                }
                if is_compact_request
                    && compact_non_success_body_should_be_normalized(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                {
                    return Ok(respond_invalid_compact_non_success_body(
                        request,
                        status.0,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    ));
                }
                if status.0 >= 400
                    && non_success_body_should_be_normalized(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                {
                    return Ok(respond_normalized_passthrough_non_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    ));
                }
                let upstream_error_hint = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    ),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                if should_suppress_deactivation_delivery(
                    upstream_error_hint.as_deref(),
                    allow_failover_for_deactivation,
                ) {
                    return Ok(with_bridge_debug_meta(
                        UpstreamResponseBridgeResult {
                            usage,
                            stream_terminal_seen: true,
                            stream_terminal_error: None,
                            delivery_error: None,
                            upstream_error_hint,
                            delivered_status_code: None,
                            upstream_request_id: None,
                            upstream_cf_ray: None,
                            upstream_auth_error: None,
                            upstream_identity_error_code: None,
                            upstream_content_type: None,
                            last_sse_event_type: None,
                        },
                        &upstream_request_id,
                        &upstream_cf_ray,
                        &upstream_auth_error,
                        &upstream_identity_error_code,
                        &upstream_content_type,
                        None,
                    ));
                }
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            if is_stream && !is_sse && status.0 >= 400 {
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else {
                    UpstreamResponseUsage::default()
                };
                if non_success_body_should_be_normalized(
                    status.0,
                    upstream_content_type.as_deref(),
                    upstream_body.as_ref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                ) {
                    return Ok(respond_normalized_passthrough_non_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    ));
                }
                let upstream_error_hint = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    ),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            if is_stream && !is_sse && status.0 < 400 && is_compact_request_path(request_path) {
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else {
                    UpstreamResponseUsage::default()
                };
                return Ok(respond_compact_success_body(
                    request,
                    status,
                    headers,
                    usage,
                    upstream_body.as_ref(),
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                    &upstream_content_type,
                    trace_id,
                ));
            }
            if is_sse || is_stream {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<dyn std::io::Read + Send> =
                    if request_path.starts_with("/v1/responses") {
                        Box::new(OpenAIResponsesPassthroughSseReader::new(
                            upstream,
                            Arc::clone(&usage_collector),
                            keepalive_frame,
                            request_started_at,
                        ))
                    } else {
                        Box::new(PassthroughSseUsageReader::new(
                            upstream,
                            Arc::clone(&usage_collector),
                            keepalive_frame,
                            passthrough_sse_protocol,
                            request_started_at,
                        ))
                    };
                force_openai_responses_stream_content_type(&mut headers, request_path, is_stream);
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let last_sse_event_type = collector.last_event_type.clone();
                let result = with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage: collector.usage,
                        stream_terminal_seen: collector.saw_terminal,
                        stream_terminal_error: collector.terminal_error,
                        delivery_error,
                        upstream_error_hint: with_upstream_debug_suffix(
                            collector.upstream_error_hint,
                            None,
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        ),
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    last_sse_event_type,
                );
                log_bridge_stream_diagnostics(response_adapter, request_path, &result);
                return Ok(result);
            }
            let len = upstream.content_length().map(|v| v as usize);
            let response = Response::new(status, headers, upstream, len, None);
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage: UpstreamResponseUsage::default(),
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint: None,
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ))
        }
        ResponseAdapter::CompactFromChatCompletions => {
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = Vec::new();
            for (name, value) in upstream.headers().iter() {
                let name_str = name.as_str();
                if name_str.eq_ignore_ascii_case("transfer-encoding")
                    || name_str.eq_ignore_ascii_case("content-length")
                    || name_str.eq_ignore_ascii_case("connection")
                {
                    continue;
                }
                if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                    headers.push(header);
                }
            }
            replace_content_type_header(&mut headers, "application/json");
            if let Some(trace_id) = trace_id {
                push_trace_id_header(&mut headers, trace_id);
            }
            let upstream_body = upstream
                .bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                .ok()
                .map(|value| parse_usage_from_json(&value))
                .unwrap_or_default();
            let response_body = if status.0 < 400 {
                convert_chat_completions_body_to_compact(upstream_body.as_ref())
                    .unwrap_or_else(|| upstream_body.to_vec())
            } else {
                upstream_body.to_vec()
            };
            let upstream_error_hint = (status.0 >= 400)
                .then(|| {
                    with_upstream_debug_suffix(
                        extract_error_hint_from_body(status.0, upstream_body.as_ref()),
                        None,
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                })
                .flatten();
            let len = Some(response_body.len());
            let response = Response::new(
                status,
                headers,
                std::io::Cursor::new(response_body),
                len,
                None,
            );
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint,
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ))
        }
        ResponseAdapter::AnthropicMessagesFromResponses
        | ResponseAdapter::ChatCompletionsFromResponses
        | ResponseAdapter::ImagesB64JsonFromResponses
        | ResponseAdapter::ImagesUrlFromResponses
        | ResponseAdapter::GeminiJson
        | ResponseAdapter::GeminiCliJson
        | ResponseAdapter::GeminiSse
        | ResponseAdapter::GeminiCliSse => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn respond_with_stream_upstream(
    request: Request,
    upstream: GatewayStreamResponse,
    _inflight_guard: super::super::AccountInFlightGuard,
    response_adapter: ResponseAdapter,
    _passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
    is_stream: bool,
    _allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    let keepalive_frame = resolve_stream_keepalive_frame(response_adapter, request_path);
    let upstream_request_id =
        first_upstream_header(upstream.headers(), REQUEST_ID_HEADER_CANDIDATES);
    let upstream_cf_ray = first_upstream_header(upstream.headers(), &[CF_RAY_HEADER_NAME]);
    let upstream_auth_error = first_upstream_header(upstream.headers(), &[AUTH_ERROR_HEADER_NAME]);
    let upstream_identity_error_code =
        crate::gateway::extract_identity_error_code_from_headers(upstream.headers());
    let upstream_content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    if response_adapter != ResponseAdapter::Passthrough {
        let status = StatusCode(upstream.status().as_u16());
        let mut headers = Vec::new();
        for (name, value) in upstream.headers().iter() {
            let name_str = name.as_str();
            if name_str.eq_ignore_ascii_case("transfer-encoding")
                || name_str.eq_ignore_ascii_case("content-length")
                || name_str.eq_ignore_ascii_case("connection")
            {
                continue;
            }
            if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                headers.push(header);
            }
        }
        if let Some(trace_id) = trace_id {
            push_trace_id_header(&mut headers, trace_id);
        }
        let is_sse = upstream_content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
            .unwrap_or(false);
        let is_json = upstream_content_type
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains("application/json"))
            .unwrap_or(false);

        if !is_stream {
            let upstream_body = upstream
                .read_all_bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let detected_sse =
                is_sse || (!is_json && looks_like_sse_payload(upstream_body.as_ref()));
            let (body, usage) = if detected_sse {
                let (synthesized, mut usage) =
                    collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let body = synthesized.unwrap_or_else(|| upstream_body.to_vec());
                if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                    merge_usage(&mut usage, parse_usage_from_json(&value));
                }
                (body, usage)
            } else {
                let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                    .ok()
                    .map(|value| parse_usage_from_json(&value))
                    .unwrap_or_default();
                (upstream_body.to_vec(), usage)
            };
            let response_body = if status.0 >= 400 {
                let message = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        &body,
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                    .or_else(|| extract_error_message_from_json_bytes(&body)),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                )
                .unwrap_or_else(|| "upstream compatibility bridge failed".to_string());
                convert_error_body_for_adapter(response_adapter, &message)
            } else {
                convert_success_body_for_adapter(
                    response_adapter,
                    &body,
                    request_path,
                    tool_name_restore_map,
                )
                .unwrap_or_else(|| body.clone())
            };
            replace_content_type_header(&mut headers, "application/json");
            let len = Some(response_body.len());
            let response = Response::new(
                status,
                headers,
                std::io::Cursor::new(response_body),
                len,
                None,
            );
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            return Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint: None,
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ));
        }

        if status.0 >= 400 && !is_sse {
            let upstream_body = upstream
                .read_all_bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let message = with_upstream_debug_suffix(
                extract_error_hint_from_body_or_headers(
                    status.0,
                    upstream_content_type.as_deref(),
                    upstream_body.as_ref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                )
                .or_else(|| extract_error_message_from_json_bytes(upstream_body.as_ref())),
                None,
                upstream_request_id.as_deref(),
                upstream_cf_ray.as_deref(),
                upstream_auth_error.as_deref(),
                upstream_identity_error_code.as_deref(),
            )
            .unwrap_or_else(|| "upstream compatibility bridge failed".to_string());
            let response_body = convert_error_body_for_adapter(response_adapter, &message);
            replace_content_type_header(&mut headers, "application/json");
            let len = Some(response_body.len());
            let response = Response::new(
                status,
                headers,
                std::io::Cursor::new(response_body),
                len,
                None,
            );
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            return Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage: UpstreamResponseUsage::default(),
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint: Some(message),
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ));
        }

        replace_content_type_header(
            &mut headers,
            compatibility_stream_content_type(response_adapter, gemini_stream_output_mode),
        );
        match response_adapter {
            ResponseAdapter::AnthropicMessagesFromResponses => {
                let upstream_body = upstream
                    .read_all_bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
                let response_body: Box<dyn std::io::Read + Send> =
                    Box::new(AnthropicSseReader::from_reader(
                        std::io::Cursor::new(upstream_body.to_vec()),
                        Arc::clone(&usage_collector),
                        fallback_model,
                        tool_name_restore_map.cloned(),
                        request_started_at,
                    ));
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let usage = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint: None,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::ChatCompletionsFromResponses => {
                let upstream_body = upstream
                    .read_all_bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let (synthesized, mut usage) =
                    collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let body = synthesized.unwrap_or_else(|| upstream_body.to_vec());
                if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                    merge_usage(&mut usage, parse_usage_from_json(&value));
                }
                let chat_body =
                    convert_responses_body_to_chat_completions(&body).unwrap_or_else(|| body);
                let response_body = chat_completion_body_to_single_sse(&chat_body);
                let len = Some(response_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(response_body),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint: None,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::CompactFromChatCompletions => unreachable!(),
            ResponseAdapter::ImagesB64JsonFromResponses
            | ResponseAdapter::ImagesUrlFromResponses => {
                let response_format = if response_adapter == ResponseAdapter::ImagesUrlFromResponses
                {
                    ImagesResponseFormat::Url
                } else {
                    ImagesResponseFormat::B64Json
                };
                let upstream_body = upstream
                    .read_all_bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let (synthesized, mut usage) =
                    collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let body = synthesized.unwrap_or_else(|| upstream_body.to_vec());
                if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                    merge_usage(&mut usage, parse_usage_from_json(&value));
                }
                let response_body = images_response_body_to_sse(&body, response_format);
                let len = Some(response_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(response_body),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint: None,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::GeminiJson | ResponseAdapter::GeminiCliJson => unreachable!(),
            ResponseAdapter::GeminiSse | ResponseAdapter::GeminiCliSse => {
                let upstream_body = upstream
                    .read_all_bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<dyn std::io::Read + Send> =
                    Box::new(GeminiSseReader::from_reader(
                        std::io::Cursor::new(upstream_body.to_vec()),
                        Arc::clone(&usage_collector),
                        tool_name_restore_map.cloned(),
                        gemini_stream_output_mode.unwrap_or(GeminiStreamOutputMode::Sse),
                        gemini_cli_wrap_response_envelope(response_adapter),
                        request_started_at,
                    ));
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage: collector.usage,
                        stream_terminal_seen: collector.saw_terminal,
                        stream_terminal_error: collector.terminal_error,
                        delivery_error,
                        upstream_error_hint: collector.upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: collector.last_event_type,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }
            ResponseAdapter::Passthrough => {}
        }
    }

    match response_adapter {
        ResponseAdapter::Passthrough => {
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = Vec::new();
            for (name, value) in upstream.headers().iter() {
                let name_str = name.as_str();
                if name_str.eq_ignore_ascii_case("transfer-encoding")
                    || name_str.eq_ignore_ascii_case("content-length")
                    || name_str.eq_ignore_ascii_case("connection")
                {
                    continue;
                }
                if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                    headers.push(header);
                }
            }
            if let Some(trace_id) = trace_id {
                push_trace_id_header(&mut headers, trace_id);
            }
            let is_sse = upstream_content_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase().starts_with("text/event-stream"))
                .unwrap_or(false);
            let is_json = upstream_content_type
                .as_deref()
                .map(|value| value.to_ascii_lowercase().contains("application/json"))
                .unwrap_or(false);

            if !is_stream {
                let upstream_body = upstream
                    .read_all_bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let detected_sse =
                    is_sse || (!is_json && looks_like_sse_payload(upstream_body.as_ref()));
                let is_compact_request = is_compact_request_path(request_path);
                if detected_sse {
                    let (synthesized_body, mut usage) =
                        collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                    let synthesized_response = synthesized_body.is_some();
                    let body = synthesized_body.unwrap_or_else(|| upstream_body.to_vec());
                    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                        merge_usage(&mut usage, parse_usage_from_json(&value));
                    }
                    let upstream_error_hint = with_upstream_debug_suffix(
                        extract_error_hint_from_body_or_headers(
                            status.0,
                            upstream_content_type.as_deref(),
                            &body,
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        ),
                        None,
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    );
                    if should_suppress_deactivation_delivery(
                        upstream_error_hint.as_deref(),
                        _allow_failover_for_deactivation,
                    ) {
                        return Ok(with_bridge_debug_meta(
                            UpstreamResponseBridgeResult {
                                usage,
                                stream_terminal_seen: true,
                                stream_terminal_error: None,
                                delivery_error: None,
                                upstream_error_hint,
                                delivered_status_code: None,
                                upstream_request_id: None,
                                upstream_cf_ray: None,
                                upstream_auth_error: None,
                                upstream_identity_error_code: None,
                                upstream_content_type: None,
                                last_sse_event_type: None,
                            },
                            &upstream_request_id,
                            &upstream_cf_ray,
                            &upstream_auth_error,
                            &upstream_identity_error_code,
                            &upstream_content_type,
                            None,
                        ));
                    }
                    if synthesized_response {
                        headers.retain(|header| {
                            !header
                                .field
                                .as_str()
                                .as_str()
                                .eq_ignore_ascii_case("Content-Type")
                        });
                        if let Ok(content_type_header) = Header::from_bytes(
                            b"Content-Type".as_slice(),
                            b"application/json".as_slice(),
                        ) {
                            headers.push(content_type_header);
                        }
                    }
                    if status.0 < 400
                        && is_compact_request
                        && !compact_success_body_is_valid(body.as_ref())
                    {
                        return Ok(respond_invalid_compact_success_body(
                            request,
                            usage,
                            body.as_ref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        ));
                    }
                    if is_compact_request
                        && compact_non_success_body_should_be_normalized(
                            status.0,
                            upstream_content_type.as_deref(),
                            body.as_ref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        )
                    {
                        return Ok(respond_invalid_compact_non_success_body(
                            request,
                            status.0,
                            usage,
                            body.as_ref(),
                            upstream_content_type.as_deref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        ));
                    }
                    if status.0 >= 400
                        && non_success_body_should_be_normalized(
                            status.0,
                            upstream_content_type.as_deref(),
                            body.as_ref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        )
                    {
                        return Ok(respond_normalized_passthrough_non_success_body(
                            request,
                            usage,
                            body.as_ref(),
                            upstream_content_type.as_deref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        ));
                    }
                    let len = Some(body.len());
                    let response =
                        Response::new(status, headers, std::io::Cursor::new(body), len, None);
                    let delivery_error = request.respond(response).err().map(|err| err.to_string());
                    return Ok(with_bridge_debug_meta(
                        UpstreamResponseBridgeResult {
                            usage,
                            stream_terminal_seen: true,
                            stream_terminal_error: None,
                            delivery_error,
                            upstream_error_hint,
                            delivered_status_code: None,
                            upstream_request_id: None,
                            upstream_cf_ray: None,
                            upstream_auth_error: None,
                            upstream_identity_error_code: None,
                            upstream_content_type: None,
                            last_sse_event_type: None,
                        },
                        &upstream_request_id,
                        &upstream_cf_ray,
                        &upstream_auth_error,
                        &upstream_identity_error_code,
                        &upstream_content_type,
                        None,
                    ));
                }

                let (_, sse_usage) = collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else if usage_has_signal(&sse_usage) {
                    sse_usage
                } else {
                    UpstreamResponseUsage::default()
                };
                if status.0 >= 400
                    && non_success_body_should_be_normalized(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                {
                    return Ok(respond_normalized_passthrough_non_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    ));
                }
                let upstream_error_hint = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    ),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                if should_suppress_deactivation_delivery(
                    upstream_error_hint.as_deref(),
                    _allow_failover_for_deactivation,
                ) {
                    return Ok(with_bridge_debug_meta(
                        UpstreamResponseBridgeResult {
                            usage,
                            stream_terminal_seen: true,
                            stream_terminal_error: None,
                            delivery_error: None,
                            upstream_error_hint,
                            delivered_status_code: None,
                            upstream_request_id: None,
                            upstream_cf_ray: None,
                            upstream_auth_error: None,
                            upstream_identity_error_code: None,
                            upstream_content_type: None,
                            last_sse_event_type: None,
                        },
                        &upstream_request_id,
                        &upstream_cf_ray,
                        &upstream_auth_error,
                        &upstream_identity_error_code,
                        &upstream_content_type,
                        None,
                    ));
                }
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }

            if is_stream && !is_sse && status.0 >= 400 {
                let upstream_body = upstream
                    .read_all_bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = UpstreamResponseUsage::default();
                if non_success_body_should_be_normalized(
                    status.0,
                    upstream_content_type.as_deref(),
                    upstream_body.as_ref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                ) {
                    return Ok(respond_normalized_passthrough_non_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    ));
                }
                let upstream_error_hint = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    ),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                    None,
                );
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                return Ok(with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage,
                        stream_terminal_seen: true,
                        stream_terminal_error: None,
                        delivery_error,
                        upstream_error_hint,
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    None,
                ));
            }

            if is_stream && !is_sse && status.0 < 400 && is_compact_request_path(request_path) {
                let upstream_body = upstream
                    .read_all_bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = UpstreamResponseUsage::default();
                return Ok(respond_compact_success_body(
                    request,
                    status,
                    headers,
                    usage,
                    upstream_body.as_ref(),
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                    &upstream_content_type,
                    trace_id,
                ));
            }

            if is_sse || is_stream {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<dyn std::io::Read + Send> =
                    if request_path.starts_with("/v1/responses") {
                        Box::new(OpenAIResponsesPassthroughSseReader::from_stream_response(
                            upstream,
                            Arc::clone(&usage_collector),
                            keepalive_frame,
                            request_started_at,
                        ))
                    } else {
                        return Err(format!(
                            "stream upstream response is not supported for path {request_path}"
                        ));
                    };
                force_openai_responses_stream_content_type(&mut headers, request_path, is_stream);
                let response = Response::new(status, headers, response_body, None, None);
                let delivery_error = request.respond(response).err().map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let last_sse_event_type = collector.last_event_type.clone();
                let result = with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage: collector.usage,
                        stream_terminal_seen: collector.saw_terminal,
                        stream_terminal_error: collector.terminal_error,
                        delivery_error,
                        upstream_error_hint: with_upstream_debug_suffix(
                            collector.upstream_error_hint,
                            None,
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        ),
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    last_sse_event_type,
                );
                log_bridge_stream_diagnostics(response_adapter, request_path, &result);
                return Ok(result);
            }

            let upstream_body = upstream
                .read_all_bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let len = Some(upstream_body.len());
            let response = Response::new(
                status,
                headers,
                std::io::Cursor::new(upstream_body.to_vec()),
                len,
                None,
            );
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage: UpstreamResponseUsage::default(),
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint: None,
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ))
        }
        ResponseAdapter::CompactFromChatCompletions => {
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = Vec::new();
            for (name, value) in upstream.headers().iter() {
                let name_str = name.as_str();
                if name_str.eq_ignore_ascii_case("transfer-encoding")
                    || name_str.eq_ignore_ascii_case("content-length")
                    || name_str.eq_ignore_ascii_case("connection")
                {
                    continue;
                }
                if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
                    headers.push(header);
                }
            }
            replace_content_type_header(&mut headers, "application/json");
            if let Some(trace_id) = trace_id {
                push_trace_id_header(&mut headers, trace_id);
            }
            let upstream_body = upstream
                .read_all_bytes()
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                .ok()
                .map(|value| parse_usage_from_json(&value))
                .unwrap_or_default();
            let response_body = if status.0 < 400 {
                convert_chat_completions_body_to_compact(upstream_body.as_ref())
                    .unwrap_or_else(|| upstream_body.to_vec())
            } else {
                upstream_body.to_vec()
            };
            let upstream_error_hint = (status.0 >= 400)
                .then(|| {
                    with_upstream_debug_suffix(
                        extract_error_hint_from_body(status.0, upstream_body.as_ref()),
                        None,
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                })
                .flatten();
            let len = Some(response_body.len());
            let response = Response::new(
                status,
                headers,
                std::io::Cursor::new(response_body),
                len,
                None,
            );
            let delivery_error = request.respond(response).err().map(|err| err.to_string());
            Ok(with_bridge_debug_meta(
                UpstreamResponseBridgeResult {
                    usage,
                    stream_terminal_seen: true,
                    stream_terminal_error: None,
                    delivery_error,
                    upstream_error_hint,
                    delivered_status_code: None,
                    upstream_request_id: None,
                    upstream_cf_ray: None,
                    upstream_auth_error: None,
                    upstream_identity_error_code: None,
                    upstream_content_type: None,
                    last_sse_event_type: None,
                },
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
                None,
            ))
        }
        ResponseAdapter::AnthropicMessagesFromResponses
        | ResponseAdapter::ChatCompletionsFromResponses
        | ResponseAdapter::ImagesB64JsonFromResponses
        | ResponseAdapter::ImagesUrlFromResponses
        | ResponseAdapter::GeminiJson
        | ResponseAdapter::GeminiCliJson
        | ResponseAdapter::GeminiSse
        | ResponseAdapter::GeminiCliSse => unreachable!(),
    }
}

/// 函数 `resolve_stream_keepalive_frame`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - response_adapter: 参数 response_adapter
/// - request_path: 参数 request_path
///
/// # 返回
/// 返回函数执行结果
fn resolve_stream_keepalive_frame(
    response_adapter: ResponseAdapter,
    request_path: &str,
) -> SseKeepAliveFrame {
    match response_adapter {
        ResponseAdapter::Passthrough => {
            if request_path.starts_with("/v1/responses") {
                SseKeepAliveFrame::OpenAIResponses
            } else {
                SseKeepAliveFrame::Comment
            }
        }
        ResponseAdapter::AnthropicMessagesFromResponses
        | ResponseAdapter::ChatCompletionsFromResponses
        | ResponseAdapter::CompactFromChatCompletions
        | ResponseAdapter::ImagesB64JsonFromResponses
        | ResponseAdapter::ImagesUrlFromResponses
        | ResponseAdapter::GeminiJson
        | ResponseAdapter::GeminiCliJson
        | ResponseAdapter::GeminiSse
        | ResponseAdapter::GeminiCliSse => SseKeepAliveFrame::Comment,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_passthrough_non_success_message, classify_compact_non_success_kind,
        compact_non_success_body_should_be_normalized, compact_success_body_is_valid,
        convert_chat_completions_body_to_compact, convert_responses_body_to_chat_completions,
        convert_responses_body_to_gemini_generate_content, convert_responses_body_to_images,
        force_openai_responses_stream_content_type, gemini_cli_wrap_response_envelope, Header,
        ImagesResponseFormat, ResponseAdapter,
    };
    use serde_json::json;

    /// 函数 `compact_header_only_identity_error_is_normalized_and_classified`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn compact_header_only_identity_error_is_normalized_and_classified() {
        assert!(compact_non_success_body_should_be_normalized(
            403,
            Some("text/plain"),
            b"",
            None,
            Some("org_membership_required"),
        ));
        assert_eq!(
            classify_compact_non_success_kind(
                403,
                Some("text/plain"),
                b"",
                None,
                None,
                Some("org_membership_required"),
            ),
            "identity_error"
        );
    }

    /// 函数 `compact_header_only_cf_ray_is_classified_as_cloudflare_edge`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn compact_header_only_cf_ray_is_classified_as_cloudflare_edge() {
        assert_eq!(
            classify_compact_non_success_kind(
                502,
                Some("text/plain"),
                b"",
                Some("ray_compact_edge"),
                None,
                None,
            ),
            "cloudflare_edge"
        );
    }

    #[test]
    fn compact_success_body_matches_official_compact_response_shape() {
        assert!(compact_success_body_is_valid(
            json!({
                "output": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{ "type": "input_text", "text": "keep context" }]
                    },
                    {
                        "type": "compaction",
                        "encrypted_content": "summary_payload"
                    }
                ]
            })
            .to_string()
            .as_bytes()
        ));
        assert!(compact_success_body_is_valid(
            json!({
                "output": [
                    {
                        "type": "context_compaction",
                        "encrypted_content": "summary_payload"
                    }
                ]
            })
            .to_string()
            .as_bytes()
        ));
        assert!(compact_success_body_is_valid(
            json!({ "output": [] }).to_string().as_bytes()
        ));
        assert!(compact_success_body_is_valid(
            json!({
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "done" }]
                    }
                ]
            })
            .to_string()
            .as_bytes()
        ));
        assert!(!compact_success_body_is_valid(
            json!({ "id": "resp_missing_output" })
                .to_string()
                .as_bytes()
        ));
        assert!(!compact_success_body_is_valid(
            json!({
                "output": [
                    {
                        "type": "message",
                        "role": "assistant"
                    }
                ]
            })
            .to_string()
            .as_bytes()
        ));
    }

    #[test]
    fn chat_completions_body_converts_to_compact_response_shape() {
        let body = json!({
            "id": "chatcmpl_custom_compact",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "压缩摘要"
                },
                "finish_reason": "stop"
            }]
        });
        let converted = convert_chat_completions_body_to_compact(body.to_string().as_bytes())
            .expect("convert chat completions response");

        assert!(compact_success_body_is_valid(converted.as_slice()));
        let value: serde_json::Value =
            serde_json::from_slice(converted.as_slice()).expect("compact json");
        assert_eq!(value["output"][0]["type"], "message");
        assert_eq!(value["output"][0]["role"], "assistant");
        assert_eq!(value["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(value["output"][0]["content"][0]["text"], "压缩摘要");
    }

    #[test]
    fn header_only_cloudflare_challenge_uses_stable_hint() {
        let message = build_passthrough_non_success_message(
            502,
            Some("text/html; charset=utf-8"),
            b"",
            Some("req-header-only"),
            Some("ray-header-only"),
            None,
            None,
        );

        assert!(message.contains("Cloudflare 安全验证页"));
        assert!(message.contains("cf_ray=ray-header-only"));
    }

    #[test]
    fn cloudflare_html_preview_keeps_title_hint() {
        let message = build_passthrough_non_success_message(
            502,
            Some("text/html; charset=utf-8"),
            b"<html><head><title>Just a moment...</title></head><body>Cloudflare</body></html>",
            Some("req-preview"),
            Some("ray-preview"),
            None,
            None,
        );

        assert!(message.contains("Cloudflare 安全验证页（title=Just a moment...）"));
        assert!(message.contains("cf_ray=ray-preview"));
    }

    #[test]
    fn streaming_responses_passthrough_forces_sse_content_type() {
        let mut headers = vec![
            Header::from_bytes(
                b"Content-Type".as_slice(),
                b"application/json; charset=utf-8".as_slice(),
            )
            .expect("content-type header"),
            Header::from_bytes(b"x-request-id".as_slice(), b"req_test".as_slice())
                .expect("request id header"),
        ];

        force_openai_responses_stream_content_type(&mut headers, "/v1/responses", true);

        let content_type = headers
            .iter()
            .find(|header| {
                header
                    .field
                    .as_str()
                    .as_str()
                    .eq_ignore_ascii_case("Content-Type")
            })
            .map(|header| header.value.as_str());
        assert_eq!(content_type, Some("text/event-stream"));
        assert!(headers
            .iter()
            .any(|header| header.field.as_str().as_str() == "x-request-id"));
    }

    #[test]
    fn non_stream_gemini_response_preserves_function_call_id_and_top_level_function_calls() {
        let body = json!({
            "id": "resp_non_stream_tool",
            "model": "gpt-5.4",
            "output": [{
                "type": "function_call",
                "call_id": "call_non_stream_write",
                "name": "write_file",
                "arguments": "{\"path\":\"plan.md\"}"
            }],
            "usage": { "input_tokens": 1, "output_tokens": 1, "total_tokens": 2 }
        });

        let mapped = convert_responses_body_to_gemini_generate_content(
            serde_json::to_vec(&body).expect("body").as_slice(),
            false,
            None,
        )
        .expect("convert gemini body");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(
            value["candidates"][0]["content"]["parts"][0]["functionCall"]["id"],
            "call_non_stream_write"
        );
        assert_eq!(value["functionCalls"][0]["id"], "call_non_stream_write");
        assert_eq!(value["functionCalls"][0]["args"]["path"], "plan.md");
    }

    #[test]
    fn non_stream_chat_completion_response_adds_image_generation_message_images() {
        let body = json!({
            "id": "resp_non_stream_image",
            "model": "gpt-5.4",
            "output": [{
                "type": "image_generation_call",
                "id": "ig_non_stream_1",
                "status": "completed",
                "output_format": "png",
                "result": "aGVsbG8="
            }],
            "usage": { "input_tokens": 2, "output_tokens": 1, "total_tokens": 3 }
        });

        let mapped = convert_responses_body_to_chat_completions(
            serde_json::to_vec(&body).expect("body").as_slice(),
        )
        .expect("convert chat completion body");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(
            value["choices"][0]["message"]["images"][0]["type"],
            "image_url"
        );
        assert_eq!(
            value["choices"][0]["message"]["images"][0]["image_url"]["url"],
            "data:image/png;base64,aGVsbG8="
        );
        assert_eq!(
            value["usage"]["prompt_tokens"],
            serde_json::Value::Number(2.into())
        );
    }

    #[test]
    fn non_stream_images_response_builds_b64_json_payload() {
        let body = json!({
            "id": "resp_images_1",
            "created_at": 1775900000,
            "model": "gpt-5.4",
            "output": [{
                "type": "image_generation_call",
                "id": "ig_1",
                "status": "completed",
                "revised_prompt": "一只极简猫",
                "output_format": "png",
                "size": "1024x1024",
                "quality": "high",
                "background": "transparent",
                "result": "aGVsbG8="
            }],
            "tool_usage": {
                "image_gen": { "input_tokens": 1, "output_tokens": 2, "total_tokens": 3 }
            }
        });

        let mapped = convert_responses_body_to_images(
            serde_json::to_vec(&body).expect("body").as_slice(),
            ImagesResponseFormat::B64Json,
        )
        .expect("convert images body");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse images body");

        assert_eq!(value["created"], 1775900000);
        assert_eq!(value["data"][0]["b64_json"], "aGVsbG8=");
        assert_eq!(value["data"][0]["revised_prompt"], "一只极简猫");
        assert_eq!(value["size"], "1024x1024");
        assert_eq!(value["quality"], "high");
        assert_eq!(value["background"], "transparent");
        assert_eq!(value["output_format"], "png");
        assert_eq!(value["usage"]["total_tokens"], 3);
    }

    #[test]
    fn non_stream_images_response_builds_url_payload() {
        let body = json!({
            "created": 1775900001,
            "output": [{
                "type": "image_generation_call",
                "output_format": "webp",
                "result": "aGVsbG8="
            }]
        });

        let mapped = convert_responses_body_to_images(
            serde_json::to_vec(&body).expect("body").as_slice(),
            ImagesResponseFormat::Url,
        )
        .expect("convert images body");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse images body");

        assert_eq!(value["data"][0]["url"], "data:image/webp;base64,aGVsbG8=");
    }

    #[test]
    fn non_stream_gemini_response_decodes_double_encoded_function_call_arguments() {
        let body = json!({
            "id": "resp_non_stream_double_encoded_tool",
            "model": "gpt-5.4",
            "output": [{
                "type": "function_call",
                "call_id": "call_non_stream_double_encoded_write",
                "name": "write_file",
                "arguments": "\"{\\\"file_path\\\":\\\"C:/Users/test/Desktop/test/gemini/plan.md\\\",\\\"content\\\":\\\"plan\\\"}\""
            }],
            "usage": { "input_tokens": 1, "output_tokens": 1, "total_tokens": 2 }
        });

        let mapped = convert_responses_body_to_gemini_generate_content(
            serde_json::to_vec(&body).expect("body").as_slice(),
            false,
            None,
        )
        .expect("convert gemini body");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(
            value["candidates"][0]["content"]["parts"][0]["functionCall"]["args"]["file_path"],
            "C:/Users/test/Desktop/test/gemini/plan.md"
        );
        assert_eq!(value["functionCalls"][0]["args"]["content"], "plan");
    }

    #[test]
    fn gemini_cli_wrap_response_envelope_is_enabled_for_gemini_adapter_only() {
        assert!(gemini_cli_wrap_response_envelope(
            ResponseAdapter::GeminiCliJson
        ));
        assert!(gemini_cli_wrap_response_envelope(
            ResponseAdapter::GeminiCliSse
        ));
        assert!(!gemini_cli_wrap_response_envelope(
            ResponseAdapter::AnthropicMessagesFromResponses
        ));
        assert!(!gemini_cli_wrap_response_envelope(
            ResponseAdapter::ChatCompletionsFromResponses
        ));
        assert!(!gemini_cli_wrap_response_envelope(
            ResponseAdapter::Passthrough
        ));
    }
}
