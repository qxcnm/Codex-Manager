use serde_json::{json, Map, Value};

use super::super::{GeminiStreamOutputMode, ResponseAdapter, ToolNameRestoreMap};
use super::{
    append_output_text, build_images_api_response, collect_image_generation_chat_images,
    collect_response_reasoning_summary_text, merge_usage, parse_usage_from_json,
    ImagesResponseFormat, UpstreamResponseUsage,
};

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

fn responses_usage_from_anthropic(value: &Value) -> Value {
    let usage = value.get("usage").cloned().unwrap_or(Value::Null);
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let cached_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let reasoning_tokens = usage
        .get("reasoning_output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens,
        "input_tokens_details": { "cached_tokens": cached_tokens },
        "output_tokens_details": { "reasoning_tokens": reasoning_tokens },
    })
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

fn convert_anthropic_messages_body_to_responses(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_codexmanager");
    let model = value.get("model").and_then(Value::as_str).unwrap_or("");
    let mut output = Vec::new();
    let mut output_text = String::new();
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for item in content {
            let Some(obj) = item.as_object() else {
                continue;
            };
            match obj.get("type").and_then(Value::as_str).unwrap_or_default() {
                "text" => {
                    if let Some(text) = obj.get("text").and_then(Value::as_str) {
                        append_output_text(&mut output_text, text);
                    }
                }
                "tool_use" => {
                    output.push(json!({
                        "id": obj
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_unknown"),
                        "type": "function_call",
                        "status": "completed",
                        "call_id": obj
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_unknown"),
                        "name": obj
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool"),
                        "arguments": obj
                            .get("input")
                            .cloned()
                            .unwrap_or_else(|| json!({}))
                            .to_string(),
                    }));
                }
                _ => {}
            }
        }
    }
    if !output_text.is_empty() {
        output.insert(
            0,
            json!({
                "id": format!("msg_{response_id}"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": output_text }],
            }),
        );
    }
    let payload = json!({
        "id": response_id,
        "object": "response",
        "created_at": 0,
        "status": "completed",
        "model": model,
        "output": output,
        "usage": responses_usage_from_anthropic(&value),
    });
    serde_json::to_vec(&payload).ok()
}

pub(super) fn convert_responses_body_to_gemini_generate_content(
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

pub(super) fn merge_usage_from_body_without_output_text(
    usage: &mut UpstreamResponseUsage,
    body: &[u8],
) {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    let mut parsed_usage = parse_usage_from_json(&value);
    parsed_usage.output_text = None;
    merge_usage(usage, parsed_usage);
}

pub(super) fn convert_success_body_for_adapter(
    response_adapter: ResponseAdapter,
    body: &[u8],
    _request_path: &str,
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
) -> Option<Vec<u8>> {
    match response_adapter {
        ResponseAdapter::AnthropicMessagesFromResponses => {
            convert_responses_body_to_anthropic_messages(body, tool_name_restore_map)
        }
        ResponseAdapter::ResponsesFromAnthropicMessages => {
            convert_anthropic_messages_body_to_responses(body)
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

pub(super) fn convert_responses_body_to_chat_completions(body: &[u8]) -> Option<Vec<u8>> {
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
    let mut reasoning_text = String::new();
    collect_response_reasoning_summary_text(response, &mut reasoning_text);
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
    if !reasoning_text.trim().is_empty() {
        completion["choices"][0]["message"]["reasoning"] = Value::String(reasoning_text.clone());
        completion["choices"][0]["message"]["reasoning_content"] = Value::String(reasoning_text);
    }
    let tool_calls = collect_responses_function_calls(response);
    if !tool_calls.is_empty() {
        completion["choices"][0]["message"]["tool_calls"] = Value::Array(tool_calls);
        completion["choices"][0]["finish_reason"] = Value::String("tool_calls".to_string());
        if text.is_empty() {
            completion["choices"][0]["message"]["content"] = Value::Null;
        }
    }
    let images = collect_image_generation_chat_images(response);
    if !images.is_empty() {
        completion["choices"][0]["message"]["images"] = Value::Array(images);
    }
    serde_json::to_vec(&completion).ok()
}

fn collect_responses_function_calls(response: &Value) -> Vec<Value> {
    response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                return None;
            }
            let name = item.get("name").and_then(Value::as_str)?;
            let id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("call_codexmanager");
            let arguments = match item.get("arguments") {
                Some(Value::String(arguments)) => arguments.clone(),
                Some(arguments) => arguments.to_string(),
                None => "{}".to_string(),
            };
            Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments,
                }
            }))
        })
        .collect()
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

pub(super) fn convert_chat_completions_body_to_compact(body: &[u8]) -> Option<Vec<u8>> {
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

pub(super) fn convert_responses_body_to_images(
    body: &[u8],
    response_format: ImagesResponseFormat,
) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response = value.get("response").unwrap_or(&value);
    serde_json::to_vec(&build_images_api_response(response, response_format)).ok()
}

pub(super) fn images_response_body_to_sse(
    body: &[u8],
    response_format: ImagesResponseFormat,
) -> Vec<u8> {
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

pub(super) fn chat_completion_body_to_single_sse(body: &[u8]) -> Vec<u8> {
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

pub(super) fn convert_error_body_for_adapter(
    response_adapter: ResponseAdapter,
    message: &str,
) -> Vec<u8> {
    match response_adapter {
        ResponseAdapter::AnthropicMessagesFromResponses => {
            convert_upstream_error_to_anthropic_body(message)
        }
        ResponseAdapter::ResponsesFromAnthropicMessages
        | ResponseAdapter::ChatCompletionsFromResponses => serde_json::to_vec(&json!({
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

pub(super) fn compatibility_stream_content_type(
    response_adapter: ResponseAdapter,
    gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
) -> &'static str {
    match response_adapter {
        ResponseAdapter::AnthropicMessagesFromResponses => "text/event-stream",
        ResponseAdapter::ResponsesFromAnthropicMessages
        | ResponseAdapter::ChatCompletionsFromResponses => "text/event-stream",
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

pub(super) fn gemini_cli_wrap_response_envelope(response_adapter: ResponseAdapter) -> bool {
    matches!(
        response_adapter,
        ResponseAdapter::GeminiCliJson | ResponseAdapter::GeminiCliSse
    )
}
