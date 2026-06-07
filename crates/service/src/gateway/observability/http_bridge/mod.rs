use tiny_http::{Header, Request};

use crate::gateway::upstream::GatewayUpstreamResponse;
use serde_json::{json, Value};

mod aggregate;
#[cfg(test)]
mod openai;
use aggregate::collect_response_reasoning_summary_text;
use aggregate::openai_responses_event::{OpenAIResponsesEvent, OpenAIResponsesOutputTextState};
pub(crate) use aggregate::PassthroughSseProtocol;
#[allow(unused_imports)]
use aggregate::{
    append_output_text, append_output_text_raw, collect_output_text_from_event_fields,
    collect_response_output_text,
};
use aggregate::{
    collect_non_stream_json_from_sse_bytes, extract_error_hint_from_body,
    extract_error_message_from_json, inspect_sse_frame_for_protocol, looks_like_sse_payload,
    merge_usage, parse_usage_from_json, reload_output_text_from_env, usage_has_signal, SseTerminal,
    UpstreamResponseBridgeResult, UpstreamResponseUsage,
};
#[cfg(test)]
use aggregate::{
    inspect_sse_frame, output_text_limit_bytes, parse_sse_frame_json, parse_usage_from_sse_frame,
    OUTPUT_TEXT_TRUNCATED_MARKER,
};

/// 函数 `reload_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn reload_from_env() {
    reload_output_text_from_env();
    stream_readers::reload_from_env();
}

/// 函数 `current_sse_keepalive_interval_ms`
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
pub(super) fn current_sse_keepalive_interval_ms() -> u64 {
    stream_readers::current_sse_keepalive_interval_ms()
}

/// 函数 `set_sse_keepalive_interval_ms`
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
pub(super) fn set_sse_keepalive_interval_ms(interval_ms: u64) -> Result<u64, String> {
    stream_readers::set_sse_keepalive_interval_ms(interval_ms)
}

/// 函数 `summarize_upstream_error_hint_from_body`
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
pub(crate) fn summarize_upstream_error_hint_from_body(
    status_code: u16,
    body: &[u8],
) -> Option<String> {
    aggregate::extract_error_hint_from_body(status_code, body)
}

/// 函数 `push_trace_id_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - trace_id: 参数 trace_id
///
/// # 返回
/// 无
fn push_trace_id_header(headers: &mut Vec<Header>, trace_id: &str) {
    let Some(trace_id) = Some(trace_id)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    if let Ok(header) = Header::from_bytes(
        crate::error_codes::TRACE_ID_HEADER_NAME.as_bytes(),
        trace_id.as_bytes(),
    ) {
        headers.push(header);
    }
}

fn mime_type_from_codex_output_format(output_format: Option<&str>) -> &'static str {
    match output_format
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("png") | None => "image/png",
        _ => "image/png",
    }
}

fn image_generation_data_url_from_item(item: &Value) -> Option<String> {
    if item.get("type").and_then(Value::as_str) != Some("image_generation_call") {
        return None;
    }
    let b64 = item
        .get("result")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mime_type =
        mime_type_from_codex_output_format(item.get("output_format").and_then(Value::as_str));
    Some(format!("data:{mime_type};base64,{b64}"))
}

fn image_generation_partial_data_url_from_event(event: &Value) -> Option<String> {
    if event.get("type").and_then(Value::as_str)
        != Some("response.image_generation_call.partial_image")
    {
        return None;
    }
    let b64 = event
        .get("partial_image_b64")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mime_type =
        mime_type_from_codex_output_format(event.get("output_format").and_then(Value::as_str));
    Some(format!("data:{mime_type};base64,{b64}"))
}

fn collect_image_generation_data_urls(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .flat_map(collect_image_generation_data_urls)
            .collect(),
        Value::Object(obj) => {
            let mut images = Vec::new();
            if let Some(image) = image_generation_data_url_from_item(value) {
                images.push(image);
            }
            if let Some(image) = image_generation_partial_data_url_from_event(value) {
                images.push(image);
            }
            for field in ["response", "output", "item", "output_item"] {
                if let Some(child) = obj.get(field) {
                    images.extend(collect_image_generation_data_urls(child));
                }
            }
            images
        }
        _ => Vec::new(),
    }
}

fn chat_image_payload(url: String, index: usize) -> Value {
    json!({
        "type": "image_url",
        "index": index,
        "image_url": {
            "url": url
        }
    })
}

fn collect_image_generation_chat_images(value: &Value) -> Vec<Value> {
    collect_image_generation_data_urls(value)
        .into_iter()
        .enumerate()
        .map(|(index, url)| chat_image_payload(url, index))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImagesResponseFormat {
    B64Json,
    Url,
}

#[derive(Debug, Clone)]
struct ImageGenerationResult {
    result: String,
    revised_prompt: Option<String>,
    output_format: Option<String>,
    size: Option<String>,
    background: Option<String>,
    quality: Option<String>,
}

fn image_generation_result_from_item(item: &Value) -> Option<ImageGenerationResult> {
    if item.get("type").and_then(Value::as_str) != Some("image_generation_call") {
        return None;
    }
    let result = item
        .get("result")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    Some(ImageGenerationResult {
        result,
        revised_prompt: trimmed_string_field(item, "revised_prompt"),
        output_format: trimmed_string_field(item, "output_format"),
        size: trimmed_string_field(item, "size"),
        background: trimmed_string_field(item, "background"),
        quality: trimmed_string_field(item, "quality"),
    })
}

fn trimmed_string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn collect_image_generation_results(value: &Value) -> Vec<ImageGenerationResult> {
    match value {
        Value::Array(items) => items
            .iter()
            .flat_map(collect_image_generation_results)
            .collect(),
        Value::Object(obj) => {
            let mut results = Vec::new();
            if let Some(result) = image_generation_result_from_item(value) {
                results.push(result);
            }
            for field in ["response", "output", "item", "output_item"] {
                if let Some(child) = obj.get(field) {
                    results.extend(collect_image_generation_results(child));
                }
            }
            results
        }
        _ => Vec::new(),
    }
}

fn images_created_timestamp(response: &Value) -> i64 {
    response
        .get("created_at")
        .or_else(|| response.get("created"))
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0)
        })
}

fn images_usage_value(response: &Value) -> Option<Value> {
    response
        .get("tool_usage")
        .and_then(|value| value.get("image_gen"))
        .cloned()
        .or_else(|| response.get("usage").cloned())
}

fn image_generation_result_payload(
    result: &ImageGenerationResult,
    response_format: ImagesResponseFormat,
) -> Value {
    let mut item = serde_json::Map::new();
    match response_format {
        ImagesResponseFormat::Url => {
            let mime_type = mime_type_from_codex_output_format(result.output_format.as_deref());
            item.insert(
                "url".to_string(),
                Value::String(format!("data:{mime_type};base64,{}", result.result)),
            );
        }
        ImagesResponseFormat::B64Json => {
            item.insert("b64_json".to_string(), Value::String(result.result.clone()));
        }
    }
    if let Some(revised_prompt) = result.revised_prompt.as_ref() {
        item.insert(
            "revised_prompt".to_string(),
            Value::String(revised_prompt.clone()),
        );
    }
    Value::Object(item)
}

fn build_images_api_response(response: &Value, response_format: ImagesResponseFormat) -> Value {
    let results = collect_image_generation_results(response);
    let mut out = json!({
        "created": images_created_timestamp(response),
        "data": results
            .iter()
            .map(|result| image_generation_result_payload(result, response_format))
            .collect::<Vec<_>>()
    });
    if let Some(first) = results.first() {
        if let Some(background) = first.background.as_ref() {
            out["background"] = Value::String(background.clone());
        }
        if let Some(output_format) = first.output_format.as_ref() {
            out["output_format"] = Value::String(output_format.clone());
        }
        if let Some(quality) = first.quality.as_ref() {
            out["quality"] = Value::String(quality.clone());
        }
        if let Some(size) = first.size.as_ref() {
            out["size"] = Value::String(size.clone());
        }
    }
    if let Some(usage) = images_usage_value(response) {
        out["usage"] = usage;
    }
    out
}

mod delivery;
mod stream_readers;
/// 函数 `respond_with_upstream`
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
pub(super) fn respond_with_upstream(
    request: Request,
    upstream: GatewayUpstreamResponse,
    inflight_guard: super::AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<super::GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&super::ToolNameRestoreMap>,
    is_stream: bool,
    allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    match upstream {
        GatewayUpstreamResponse::Blocking(upstream) => delivery::respond_with_upstream(
            request,
            upstream,
            inflight_guard,
            response_adapter,
            passthrough_sse_protocol,
            gemini_stream_output_mode,
            request_path,
            tool_name_restore_map,
            is_stream,
            allow_failover_for_deactivation,
            trace_id,
            fallback_model,
            request_started_at,
        ),
        GatewayUpstreamResponse::Stream(upstream) => delivery::respond_with_stream_upstream(
            request,
            upstream,
            inflight_guard,
            response_adapter,
            passthrough_sse_protocol,
            gemini_stream_output_mode,
            request_path,
            tool_name_restore_map,
            is_stream,
            allow_failover_for_deactivation,
            trace_id,
            fallback_model,
            request_started_at,
        ),
    }
}
pub(super) use stream_readers::{
    ChatCompletionsFromResponsesSseReader, ImagesFromResponsesSseReader,
    OpenAIResponsesPassthroughSseReader, PassthroughSseCollector, PassthroughSseUsageReader,
    ResponsesFromAnthropicSseReader, SseKeepAliveFrame,
};

pub(super) use stream_readers::{AnthropicSseReader, GeminiSseReader};

#[cfg(test)]
#[path = "../tests/http_bridge_tests.rs"]
mod tests;
