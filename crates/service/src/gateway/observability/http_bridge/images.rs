use serde_json::{json, Value};

use super::aggregate::{
    collect_non_stream_json_from_sse_bytes, extract_error_message_from_json,
    inspect_sse_frame_for_protocol, looks_like_sse_payload, parse_sse_frame_json,
    PassthroughSseProtocol, SseTerminal,
};

/// Images requests use a streaming Responses upstream even when the caller
/// expects JSON. Inspect the payload rather than trusting its Content-Type.
pub(super) fn parse_images_response_body(body: &[u8]) -> Result<Value, String> {
    parse_images_response_body_with_sse(body, false)
}

pub(super) fn parse_images_response_body_with_sse(
    body: &[u8],
    is_sse: bool,
) -> Result<Value, String> {
    // SSE permits one UTF-8 BOM at the start of the stream. Strip it before
    // both terminal inspection and aggregation, including data-only frames.
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    let value = if let Ok(value) = serde_json::from_slice::<Value>(body) {
        value
    } else if is_sse || looks_like_sse_payload(body) {
        let text = std::str::from_utf8(body)
            .map_err(|_| "upstream image response is not valid UTF-8".to_string())?;
        let mut frame = Vec::new();
        let mut saw_completion = false;
        let mut inspect = |frame: &[String]| -> Result<(), String> {
            let inspection = inspect_sse_frame_for_protocol(frame, PassthroughSseProtocol::Generic);
            let payload = parse_sse_frame_json(frame);
            let event_type = inspection
                .last_event_type
                .as_deref()
                .filter(|kind| !kind.eq_ignore_ascii_case("message"))
                .or_else(|| payload.as_ref()?.get("type")?.as_str());
            match inspection.terminal {
                Some(SseTerminal::Err(message)) => return Err(message),
                Some(SseTerminal::Ok)
                    if matches!(event_type, Some("response.completed" | "response.done")) =>
                {
                    saw_completion = true;
                }
                _ => {}
            }
            Ok(())
        };
        for line in text.lines() {
            if line.is_empty() {
                inspect(&frame)?;
                frame.clear();
            } else {
                frame.push(line.to_string());
            }
        }
        inspect(&frame)?;
        if !saw_completion {
            return Err("upstream image stream ended without a completed response".to_string());
        }
        let (response, _) = collect_non_stream_json_from_sse_bytes(body);
        let response = response
            .ok_or_else(|| "upstream image stream did not contain a response".to_string())?;
        serde_json::from_slice::<Value>(&response)
            .map_err(|_| "upstream image response is not valid JSON".to_string())?
    } else {
        return Err("upstream image response is not valid JSON".to_string());
    };
    if let Some(message) = extract_error_message_from_json(&value) {
        return Err(message);
    }
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        if matches!(
            kind,
            "error"
                | "response.failed"
                | "response.error"
                | "response.incomplete"
                | "response.canceled"
                | "response.cancelled"
        ) {
            return Err(format!(
                "upstream image response did not complete (type={kind})"
            ));
        }
    }
    let response = value.get("response").unwrap_or(&value);
    validate_images_response(response)?;
    Ok(response.clone())
}

pub(super) fn validate_images_response(response: &Value) -> Result<(), String> {
    if let Some(message) = extract_error_message_from_json(response).or_else(|| {
        response
            .get("status_details")
            .and_then(extract_error_message_from_json)
    }) {
        return Err(message);
    }
    if let Some(status) = response.get("status").and_then(Value::as_str) {
        if status != "completed" {
            return Err(format!(
                "upstream image response did not complete (status={status})"
            ));
        }
    }
    if collect_image_generation_results(response).is_empty() {
        return Err("upstream image response did not contain a generated image".to_string());
    }
    Ok(())
}

pub(super) fn mime_type_from_codex_output_format(output_format: Option<&str>) -> &'static str {
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

pub(super) fn collect_image_generation_data_urls(value: &Value) -> Vec<String> {
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

pub(super) fn chat_image_payload(url: String, index: usize) -> Value {
    json!({
        "type": "image_url",
        "index": index,
        "image_url": {
            "url": url
        }
    })
}

pub(super) fn collect_image_generation_chat_images(value: &Value) -> Vec<Value> {
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
pub(super) struct ImageGenerationResult {
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

pub(super) fn collect_image_generation_results(value: &Value) -> Vec<ImageGenerationResult> {
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

pub(super) fn images_usage_value(response: &Value) -> Option<Value> {
    response
        .get("tool_usage")
        .and_then(|value| value.get("image_gen"))
        .cloned()
        .or_else(|| response.get("usage").cloned())
}

pub(super) fn image_generation_result_payload(
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

pub(super) fn build_images_api_response(
    response: &Value,
    response_format: ImagesResponseFormat,
) -> Value {
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

#[cfg(test)]
#[path = "images_tests.rs"]
mod tests;
