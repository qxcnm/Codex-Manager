use rand::{distributions::Alphanumeric, Rng};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, VecDeque};

use super::{AdaptedGatewayRequest, GeminiStreamOutputMode, ResponseAdapter, ToolNameRestoreMap};
use crate::apikey_profile::{
    is_anthropic_request_path, is_gemini_generate_content_request_path, PROTOCOL_ANTHROPIC_NATIVE,
    PROTOCOL_GEMINI_NATIVE,
};

pub(crate) fn adapt_request_for_protocol(
    protocol_type: &str,
    path: &str,
    body: Vec<u8>,
) -> Result<AdaptedGatewayRequest, String> {
    if protocol_type == PROTOCOL_ANTHROPIC_NATIVE && is_anthropic_request_path(path) {
        return adapt_anthropic_messages_request(path, body);
    }
    if protocol_type == PROTOCOL_GEMINI_NATIVE && is_gemini_generate_content_request_path(path) {
        return adapt_gemini_generate_content_request(path, body);
    }

    Ok(AdaptedGatewayRequest {
        path: path.to_string(),
        body,
        response_adapter: ResponseAdapter::Passthrough,
        gemini_stream_output_mode: None,
        tool_name_restore_map: ToolNameRestoreMap::new(),
    })
}

pub(crate) fn adapt_openai_responses_to_anthropic_messages(
    body: &[u8],
    model_override: Option<&str>,
) -> Result<Vec<u8>, String> {
    let payload = serde_json::from_slice::<Value>(body)
        .map_err(|err| format!("invalid responses request json: {err}"))?;
    let obj = payload
        .as_object()
        .ok_or_else(|| "responses request body must be an object".to_string())?;

    let model = model_override
        .and_then(normalize_text)
        .or_else(|| {
            obj.get("model")
                .and_then(Value::as_str)
                .and_then(normalize_text)
        })
        .ok_or_else(|| "responses request model is required for anthropic upstream".to_string())?;

    let mut rewritten = Map::new();
    rewritten.insert("model".to_string(), Value::String(model));
    rewritten.insert(
        "stream".to_string(),
        Value::Bool(obj.get("stream").and_then(Value::as_bool).unwrap_or(true)),
    );
    let max_tokens = obj
        .get("max_output_tokens")
        .or_else(|| obj.get("max_tokens"))
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .unwrap_or(4096);
    rewritten.insert("max_tokens".to_string(), Value::from(max_tokens));

    let mut system_parts = Vec::new();
    if let Some(instructions) = obj
        .get("instructions")
        .and_then(Value::as_str)
        .and_then(normalize_system_text)
    {
        system_parts.push(instructions);
    }

    let mut messages = Vec::new();
    responses_input_to_anthropic_messages(obj.get("input"), &mut system_parts, &mut messages)?;
    if messages.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": "" }],
        }));
    }
    rewritten.insert("messages".to_string(), Value::Array(messages));

    if !system_parts.is_empty() {
        rewritten.insert(
            "system".to_string(),
            Value::String(system_parts.join("\n\n")),
        );
    }
    if let Some(tools) = responses_tools_to_anthropic(obj.get("tools"))? {
        rewritten.insert("tools".to_string(), tools);
        if let Some(tool_choice) = responses_tool_choice_to_anthropic(obj.get("tool_choice")) {
            rewritten.insert("tool_choice".to_string(), tool_choice);
        }
    }
    if let Some(thinking) = responses_reasoning_to_anthropic(obj.get("reasoning"), max_tokens) {
        rewritten.insert("thinking".to_string(), thinking);
    }

    serde_json::to_vec(&Value::Object(rewritten))
        .map_err(|err| format!("serialize anthropic request failed: {err}"))
}

fn adapt_anthropic_messages_request(
    _path: &str,
    body: Vec<u8>,
) -> Result<AdaptedGatewayRequest, String> {
    let payload = serde_json::from_slice::<Value>(&body)
        .map_err(|err| format!("invalid anthropic request json: {err}"))?;
    let obj = payload
        .as_object()
        .ok_or_else(|| "anthropic request body must be an object".to_string())?;

    let mut rewritten = Map::new();
    let mut tool_name_restore_map = ToolNameRestoreMap::new();
    let short_name_map = declared_short_name_map_for_anthropic_tools(obj.get("tools"));
    copy_string_field(obj, &mut rewritten, "model");
    rewritten.insert("instructions".to_string(), Value::String(String::new()));

    if let Some(system_message) = anthropic_system_to_developer_message(obj.get("system")) {
        push_input_item(&mut rewritten, system_message);
    }

    if let Some(input) = anthropic_messages_to_input(
        obj.get("messages"),
        &short_name_map,
        &mut tool_name_restore_map,
    )? {
        extend_input_items(&mut rewritten, input);
    }

    if let Some(tools) = anthropic_tools_to_responses(
        obj.get("tools"),
        &short_name_map,
        &mut tool_name_restore_map,
    )? {
        rewritten.insert("tools".to_string(), tools);
        rewritten.insert("tool_choice".to_string(), Value::String("auto".to_string()));
    }
    rewritten.insert(
        "parallel_tool_calls".to_string(),
        Value::Bool(anthropic_parallel_tool_calls(obj.get("tool_choice"))),
    );
    let reasoning = anthropic_reasoning_to_responses(obj.get("thinking"), obj.get("output_config"))
        .unwrap_or_else(|| json!({ "effort": "medium", "summary": "auto" }));
    rewritten.insert("reasoning".to_string(), reasoning);
    rewritten.insert("stream".to_string(), Value::Bool(true));
    rewritten.insert("store".to_string(), Value::Bool(false));
    rewritten.insert(
        "include".to_string(),
        Value::Array(vec![Value::String(
            "reasoning.encrypted_content".to_string(),
        )]),
    );

    Ok(AdaptedGatewayRequest {
        path: "/v1/responses".to_string(),
        body: serde_json::to_vec(&Value::Object(rewritten))
            .map_err(|err| format!("serialize anthropic compatibility request failed: {err}"))?,
        response_adapter: ResponseAdapter::AnthropicMessagesFromResponses,
        gemini_stream_output_mode: None,
        tool_name_restore_map,
    })
}

fn adapt_gemini_generate_content_request(
    path: &str,
    body: Vec<u8>,
) -> Result<AdaptedGatewayRequest, String> {
    let payload = serde_json::from_slice::<Value>(&body)
        .map_err(|err| format!("invalid gemini request json: {err}"))?;
    let obj = payload
        .as_object()
        .ok_or_else(|| "gemini request body must be an object".to_string())?;
    let request_obj = gemini_request_payload_object(obj);
    let backfilled_contents = request_obj
        .get("contents")
        .map(backfill_empty_gemini_function_response_names);

    let mut rewritten = Map::new();
    let mut tool_name_restore_map = ToolNameRestoreMap::new();
    let short_name_map = declared_short_name_map_for_gemini_tools(request_obj.get("tools"));
    let mut pending_tool_call_ids = VecDeque::new();
    let request_stream = request_obj
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| path.contains(":streamGenerateContent"));
    rewritten.insert("instructions".to_string(), Value::String(String::new()));
    let model = obj
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| extract_gemini_model_from_path(path));
    if let Some(model) = model {
        rewritten.insert("model".to_string(), Value::String(model));
    }

    if let Some(system_message) = gemini_system_instruction_to_developer_message(
        request_obj
            .get("systemInstruction")
            .or_else(|| request_obj.get("system_instruction")),
    ) {
        push_input_item(&mut rewritten, system_message);
    }

    if let Some(input) = gemini_contents_to_input(
        backfilled_contents.as_ref(),
        &short_name_map,
        &mut pending_tool_call_ids,
        &mut tool_name_restore_map,
    )? {
        extend_input_items(&mut rewritten, input);
    }

    if let Some(tools) = gemini_tools_to_responses(
        request_obj.get("tools"),
        &short_name_map,
        &mut tool_name_restore_map,
    )? {
        rewritten.insert("tools".to_string(), tools);
        rewritten.insert(
            "tool_choice".to_string(),
            gemini_tool_choice_to_responses(request_obj, &short_name_map),
        );
    }
    let reasoning = request_obj
        .get("generationConfig")
        .or_else(|| request_obj.get("generation_config"))
        .and_then(Value::as_object)
        .and_then(gemini_reasoning_to_responses)
        .unwrap_or_else(|| json!({ "effort": "medium", "summary": "auto" }));
    rewritten.insert("parallel_tool_calls".to_string(), Value::Bool(true));
    rewritten.insert("reasoning".to_string(), reasoning);
    rewritten.insert("stream".to_string(), Value::Bool(true));
    rewritten.insert("store".to_string(), Value::Bool(false));
    rewritten.insert(
        "include".to_string(),
        Value::Array(vec![Value::String(
            "reasoning.encrypted_content".to_string(),
        )]),
    );

    let gemini_stream_output_mode = resolve_gemini_stream_output_mode(path, request_stream);
    let normalized_path = path.split('?').next().unwrap_or(path);
    let response_adapter = if normalized_path.starts_with("/v1internal:") {
        if request_stream {
            ResponseAdapter::GeminiCliSse
        } else {
            ResponseAdapter::GeminiCliJson
        }
    } else if request_stream {
        ResponseAdapter::GeminiSse
    } else {
        ResponseAdapter::GeminiJson
    };

    Ok(AdaptedGatewayRequest {
        path: "/v1/responses".to_string(),
        body: serde_json::to_vec(&Value::Object(rewritten))
            .map_err(|err| format!("serialize gemini compatibility request failed: {err}"))?,
        response_adapter,
        gemini_stream_output_mode,
        tool_name_restore_map,
    })
}

fn resolve_gemini_stream_output_mode(
    path: &str,
    request_stream: bool,
) -> Option<GeminiStreamOutputMode> {
    if !request_stream {
        return None;
    }
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    for item in query.split('&') {
        let Some((key, value)) = item.split_once('=') else {
            continue;
        };
        if !key.eq_ignore_ascii_case("alt") {
            continue;
        }
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() || normalized == "sse" {
            return Some(GeminiStreamOutputMode::Sse);
        }
        return Some(GeminiStreamOutputMode::Raw);
    }
    Some(GeminiStreamOutputMode::Sse)
}

fn gemini_request_payload_object<'a>(obj: &'a Map<String, Value>) -> &'a Map<String, Value> {
    obj.get("request").and_then(Value::as_object).unwrap_or(obj)
}

fn backfill_empty_gemini_function_response_names(contents: &Value) -> Value {
    let Some(_items) = contents.as_array() else {
        return contents.clone();
    };
    let mut out = contents.clone();
    let Some(out_items) = out.as_array_mut() else {
        return out;
    };
    let mut pending_call_names = Vec::new();
    for content in out_items.iter_mut() {
        let Some(content_obj) = content.as_object_mut() else {
            continue;
        };
        let role = content_obj
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role == "model" {
            pending_call_names = content_obj
                .get("parts")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part.as_object())
                        .filter_map(|part_obj| {
                            part_obj
                                .get("functionCall")
                                .and_then(Value::as_object)
                                .and_then(|function_call| {
                                    function_call
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .and_then(normalize_text)
                                })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            continue;
        }
        if pending_call_names.is_empty() {
            continue;
        }
        let Some(parts) = content_obj.get_mut("parts").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut response_index = 0usize;
        for part in parts.iter_mut() {
            let Some(part_obj) = part.as_object_mut() else {
                continue;
            };
            let Some(function_response) = part_obj
                .get_mut("functionResponse")
                .and_then(Value::as_object_mut)
            else {
                continue;
            };
            let name_is_empty = function_response
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_none_or(|value| value.is_empty());
            if name_is_empty {
                if let Some(call_name) = pending_call_names.get(response_index) {
                    function_response.insert("name".to_string(), Value::String(call_name.clone()));
                }
            }
            response_index += 1;
        }
        pending_call_names.clear();
    }
    out
}

fn copy_string_field(source: &Map<String, Value>, target: &mut Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key).and_then(Value::as_str) {
        let value = value.trim();
        if !value.is_empty() {
            target.insert(key.to_string(), Value::String(value.to_string()));
        }
    }
}

fn push_input_item(target: &mut Map<String, Value>, item: Value) {
    match target.entry("input".to_string()) {
        serde_json::map::Entry::Vacant(entry) => {
            entry.insert(Value::Array(vec![item]));
        }
        serde_json::map::Entry::Occupied(mut entry) => {
            if let Value::Array(items) = entry.get_mut() {
                items.push(item);
            }
        }
    }
}

fn extend_input_items(target: &mut Map<String, Value>, items: Value) {
    let Value::Array(items) = items else {
        return;
    };
    for item in items {
        push_input_item(target, item);
    }
}

fn responses_message(role: &str, content: Value) -> Value {
    json!({
        "type": "message",
        "role": role,
        "content": content,
    })
}

fn anthropic_message_role_to_responses(role: &str) -> &'static str {
    match role {
        "assistant" => "assistant",
        // Claude Code 2.x sends mid-conversation system messages for MCP/runtime
        // instructions. The Codex Responses backend expects that content as a
        // developer message, not as a raw input role named "system".
        "system" | "developer" => "developer",
        _ => "user",
    }
}

fn responses_input_to_anthropic_messages(
    input: Option<&Value>,
    system_parts: &mut Vec<String>,
    messages: &mut Vec<Value>,
) -> Result<(), String> {
    match input {
        Some(Value::String(text)) => {
            messages.push(anthropic_message("user", vec![anthropic_text_block(text)]));
        }
        Some(Value::Array(items)) => {
            for item in items {
                responses_input_item_to_anthropic(item, system_parts, messages)?;
            }
        }
        Some(Value::Object(_)) => {
            responses_input_item_to_anthropic(input.unwrap(), system_parts, messages)?;
        }
        Some(_) | None => {}
    }
    Ok(())
}

fn responses_input_item_to_anthropic(
    item: &Value,
    system_parts: &mut Vec<String>,
    messages: &mut Vec<Value>,
) -> Result<(), String> {
    let Some(obj) = item.as_object() else {
        return Ok(());
    };
    match obj.get("type").and_then(Value::as_str).unwrap_or("message") {
        "message" => {
            let role = obj
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("user");
            let content = responses_message_content_to_anthropic(obj.get("content"))?;
            if matches!(role, "developer" | "system") {
                let text = anthropic_content_to_text(&content);
                if !text.trim().is_empty() {
                    system_parts.push(text);
                }
            } else if !content.is_empty() {
                messages.push(anthropic_message(
                    if role == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    },
                    content,
                ));
            }
        }
        "function_call" | "custom_tool_call" => {
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .unwrap_or_else(|| "tool".to_string());
            let id = obj
                .get("call_id")
                .or_else(|| obj.get("id"))
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .unwrap_or_else(generate_tool_call_id);
            let input =
                parse_json_string_or_value(obj.get("arguments").or_else(|| obj.get("input")));
            messages.push(anthropic_message(
                "assistant",
                vec![json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                })],
            ));
        }
        "function_call_output" => {
            let call_id = obj
                .get("call_id")
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .unwrap_or_else(generate_tool_call_id);
            let content = obj
                .get("output")
                .cloned()
                .unwrap_or(Value::String(String::new()));
            messages.push(anthropic_message(
                "user",
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": anthropic_tool_result_content(content),
                })],
            ));
        }
        _ => {}
    }
    Ok(())
}

fn responses_message_content_to_anthropic(content: Option<&Value>) -> Result<Vec<Value>, String> {
    match content {
        Some(Value::String(text)) => Ok(vec![anthropic_text_block(text)]),
        Some(Value::Array(parts)) => {
            let mut out = Vec::new();
            for part in parts {
                if let Some(mapped) = responses_content_part_to_anthropic(part)? {
                    out.push(mapped);
                }
            }
            Ok(out)
        }
        Some(Value::Object(_)) => Ok(responses_content_part_to_anthropic(content.unwrap())?
            .map(|part| vec![part])
            .unwrap_or_default()),
        Some(other) => Ok(vec![anthropic_text_block(other.to_string().as_str())]),
        None => Ok(Vec::new()),
    }
}

fn responses_content_part_to_anthropic(part: &Value) -> Result<Option<Value>, String> {
    let Some(obj) = part.as_object() else {
        return Ok(part.as_str().map(anthropic_text_block));
    };
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("text");
    match kind {
        "input_text" | "output_text" | "text" => Ok(obj
            .get("text")
            .and_then(Value::as_str)
            .map(anthropic_text_block)),
        "input_image" | "image" => Ok(None),
        _ => Ok(None),
    }
}

fn anthropic_message(role: &str, content: Vec<Value>) -> Value {
    json!({
        "role": role,
        "content": content,
    })
}

fn anthropic_text_block(text: &str) -> Value {
    json!({
        "type": "text",
        "text": text,
    })
}

fn anthropic_content_to_text(content: &[Value]) -> String {
    content
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn anthropic_tool_result_content(value: Value) -> Value {
    match value {
        Value::Array(_) => value,
        Value::String(text) => Value::String(text),
        Value::Null => Value::String(String::new()),
        other => Value::String(other.to_string()),
    }
}

fn responses_tools_to_anthropic(tools: Option<&Value>) -> Result<Option<Value>, String> {
    let Some(tools) = tools else {
        return Ok(None);
    };
    let items = tools
        .as_array()
        .ok_or_else(|| "responses tools must be an array".to_string())?;
    let mut out = Vec::new();
    for item in items {
        let Some(tool) = item.as_object() else {
            continue;
        };
        if tool
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "web_search")
        {
            out.push(json!({ "type": "web_search_20250305" }));
            continue;
        }
        if tool
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "function")
        {
            continue;
        }
        let Some(name) = tool
            .get("name")
            .and_then(Value::as_str)
            .and_then(normalize_text)
        else {
            continue;
        };
        let mut mapped = Map::new();
        mapped.insert("name".to_string(), Value::String(name));
        if let Some(description) = tool
            .get("description")
            .and_then(Value::as_str)
            .and_then(normalize_text)
        {
            mapped.insert("description".to_string(), Value::String(description));
        }
        mapped.insert(
            "input_schema".to_string(),
            tool.get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
        );
        out.push(Value::Object(mapped));
    }
    Ok((!out.is_empty()).then(|| Value::Array(out)))
}

fn responses_tool_choice_to_anthropic(tool_choice: Option<&Value>) -> Option<Value> {
    match tool_choice {
        Some(Value::String(value)) if value == "none" => Some(json!({ "type": "none" })),
        Some(Value::String(value)) if value == "required" => Some(json!({ "type": "any" })),
        Some(Value::Object(obj)) => obj
            .get("name")
            .or_else(|| {
                obj.get("function")
                    .and_then(Value::as_object)
                    .and_then(|function| function.get("name"))
            })
            .and_then(Value::as_str)
            .and_then(normalize_text)
            .map(|name| json!({ "type": "tool", "name": name })),
        _ => Some(json!({ "type": "auto" })),
    }
}

fn responses_reasoning_to_anthropic(reasoning: Option<&Value>, max_tokens: i64) -> Option<Value> {
    let obj = reasoning?.as_object()?;
    let effort = obj
        .get("effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("medium");
    if effort.eq_ignore_ascii_case("none") {
        return Some(json!({ "type": "disabled" }));
    }
    if max_tokens <= 1024 {
        return None;
    }
    let budget = match effort.to_ascii_lowercase().as_str() {
        "minimal" | "low" => 1024,
        "high" | "xhigh" => 8192,
        _ => 4096,
    }
    .min(max_tokens - 1);
    Some(json!({
        "type": "enabled",
        "budget_tokens": budget,
    }))
}

fn parse_json_string_or_value(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Some(value) => value.clone(),
        None => Value::Object(Map::new()),
    }
}

fn anthropic_system_to_developer_message(system: Option<&Value>) -> Option<Value> {
    anthropic_system_to_text(system).map(|text| {
        json!({
            "type": "message",
            "role": "developer",
            "content": [{ "type": "input_text", "text": text }],
        })
    })
}

fn anthropic_system_to_text(system: Option<&Value>) -> Option<String> {
    match system {
        Some(Value::String(text)) => normalize_system_text(text),
        Some(Value::Array(items)) => {
            let parts = items
                .iter()
                .filter_map(anthropic_content_block_to_text)
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n\n"))
        }
        Some(Value::Object(_)) => anthropic_content_block_to_text(system?),
        _ => None,
    }
}

fn anthropic_messages_to_input(
    messages: Option<&Value>,
    short_name_map: &BTreeMap<String, String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Result<Option<Value>, String> {
    let Some(messages) = messages else {
        return Ok(None);
    };
    let items = messages
        .as_array()
        .ok_or_else(|| "anthropic messages must be an array".to_string())?;
    let mut out = Vec::new();
    for item in items {
        let Some(message) = item.as_object() else {
            continue;
        };
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("user");
        let response_role = anthropic_message_role_to_responses(role);
        match message.get("content") {
            Some(Value::String(text)) => {
                if let Some(text) = normalize_text(text) {
                    out.push(responses_message(
                        response_role,
                        json!([{ "type": "input_text", "text": text }]),
                    ));
                }
            }
            Some(Value::Array(content_items)) => {
                let mut content_parts = Vec::new();
                for content_item in content_items {
                    let Some(content_obj) = content_item.as_object() else {
                        continue;
                    };
                    let kind = content_obj
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("text");
                    match kind {
                        "text" | "input_text" | "image" => {
                            if let Some(part) = anthropic_content_block_to_responses(
                                content_item,
                                role,
                                short_name_map,
                                tool_name_restore_map,
                            ) {
                                content_parts.push(part);
                            }
                        }
                        "tool_use" | "tool_result" => {
                            if !content_parts.is_empty() {
                                out.push(responses_message(
                                    response_role,
                                    Value::Array(content_parts),
                                ));
                                content_parts = Vec::new();
                            }
                            if let Some(mapped) = anthropic_content_block_to_responses(
                                content_item,
                                response_role,
                                short_name_map,
                                tool_name_restore_map,
                            ) {
                                out.push(mapped);
                            }
                        }
                        _ => {}
                    }
                }
                if !content_parts.is_empty() {
                    out.push(responses_message(
                        response_role,
                        Value::Array(content_parts),
                    ));
                }
            }
            Some(Value::Object(_)) => {
                if let Some(content) = anthropic_message_content_to_responses(
                    message.get("content"),
                    response_role,
                    short_name_map,
                    tool_name_restore_map,
                )? {
                    if matches!(content, Value::Array(_)) {
                        out.push(responses_message(response_role, content));
                    } else {
                        out.push(content);
                    }
                }
            }
            _ => {}
        }
    }
    Ok((!out.is_empty()).then(|| Value::Array(out)))
}

fn anthropic_message_content_to_responses(
    content: Option<&Value>,
    role: &str,
    short_name_map: &BTreeMap<String, String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Result<Option<Value>, String> {
    match content {
        Some(Value::String(text)) => Ok(normalize_text(text).map(|text| {
            Value::Array(vec![json!({
                "type": text_content_type_for_role(role),
                "text": text,
            })])
        })),
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                if let Some(mapped) = anthropic_content_block_to_responses(
                    item,
                    role,
                    short_name_map,
                    tool_name_restore_map,
                ) {
                    out.push(mapped);
                }
            }
            Ok((!out.is_empty()).then(|| Value::Array(out)))
        }
        Some(Value::Object(_)) => Ok(content
            .and_then(|item| {
                anthropic_content_block_to_responses(
                    item,
                    role,
                    short_name_map,
                    tool_name_restore_map,
                )
            })
            .map(|item| Value::Array(vec![item]))),
        Some(_) => Err("unsupported anthropic content payload".to_string()),
        None => Ok(None),
    }
}

fn anthropic_content_block_to_text(value: &Value) -> Option<String> {
    let block = value.as_object()?;
    let kind = block
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("text");
    match kind {
        "text" | "input_text" => block
            .get("text")
            .and_then(Value::as_str)
            .and_then(normalize_system_text),
        _ => None,
    }
}

fn anthropic_content_block_to_responses(
    value: &Value,
    role: &str,
    short_name_map: &BTreeMap<String, String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Option<Value> {
    let block = value.as_object()?;
    let kind = block
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("text");
    match kind {
        "text" | "input_text" => block
            .get("text")
            .and_then(Value::as_str)
            .and_then(normalize_text)
            .map(|text| {
                json!({
                    "type": text_content_type_for_role(role),
                    "text": text,
                })
            }),
        "image" => anthropic_image_block_to_responses(block),
        "tool_use" => {
            anthropic_tool_use_block_to_responses(block, short_name_map, tool_name_restore_map)
        }
        "tool_result" => anthropic_tool_result_block_to_responses(block),
        _ => None,
    }
}

fn anthropic_tools_to_responses(
    tools: Option<&Value>,
    short_name_map: &BTreeMap<String, String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Result<Option<Value>, String> {
    let Some(tools) = tools else {
        return Ok(None);
    };
    let items = tools
        .as_array()
        .ok_or_else(|| "anthropic tools must be an array".to_string())?;
    let mut out = Vec::new();
    for item in items {
        let Some(tool) = item.as_object() else {
            continue;
        };
        if tool
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "web_search_20250305")
        {
            out.push(json!({ "type": "web_search" }));
            continue;
        }
        let Some(name) = tool
            .get("name")
            .and_then(Value::as_str)
            .and_then(normalize_text)
        else {
            continue;
        };
        let short_name = resolve_short_tool_name(name.as_str(), short_name_map);
        tool_name_restore_map.insert(short_name.clone(), name.clone());
        let mut mapped = Map::new();
        mapped.insert("type".to_string(), Value::String("function".to_string()));
        mapped.insert("name".to_string(), Value::String(short_name));
        if let Some(description) = tool
            .get("description")
            .and_then(Value::as_str)
            .and_then(normalize_text)
        {
            mapped.insert("description".to_string(), Value::String(description));
        }
        if let Some(schema) = tool.get("input_schema") {
            mapped.insert(
                "parameters".to_string(),
                normalize_tool_schema(schema.clone()),
            );
        }
        mapped.insert("strict".to_string(), Value::Bool(false));
        out.push(Value::Object(mapped));
    }
    Ok((!out.is_empty()).then(|| Value::Array(out)))
}

fn anthropic_parallel_tool_calls(tool_choice: Option<&Value>) -> bool {
    tool_choice
        .and_then(Value::as_object)
        .and_then(|choice| choice.get("disable_parallel_tool_use"))
        .and_then(Value::as_bool)
        .map(|disabled| !disabled)
        .unwrap_or(true)
}

fn anthropic_reasoning_to_responses(
    thinking: Option<&Value>,
    output_config: Option<&Value>,
) -> Option<Value> {
    let thinking = thinking?.as_object()?;
    let kind = thinking
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let effort = match kind {
        value if value.eq_ignore_ascii_case("enabled") => {
            let budget = thinking
                .get("budget_tokens")
                .or_else(|| thinking.get("budgetTokens"))
                .and_then(Value::as_i64);
            match budget {
                Some(value) if value <= 0 => "none".to_string(),
                Some(value) if value <= 1024 => "low".to_string(),
                Some(value) if value <= 8192 => "medium".to_string(),
                Some(_) => "high".to_string(),
                None => "medium".to_string(),
            }
        }
        value if value.eq_ignore_ascii_case("adaptive") || value.eq_ignore_ascii_case("auto") => {
            output_config
                .and_then(Value::as_object)
                .and_then(|config| config.get("effort"))
                .and_then(Value::as_str)
                .and_then(normalize_text)
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_else(|| "high".to_string())
        }
        value if value.eq_ignore_ascii_case("disabled") => "none".to_string(),
        _ => return None,
    };
    Some(json!({ "effort": effort, "summary": "auto" }))
}

fn extract_gemini_model_from_path(path: &str) -> Option<String> {
    let normalized = path.split('?').next().unwrap_or(path);
    let marker = "/models/";
    let start = normalized.find(marker)? + marker.len();
    let tail = &normalized[start..];
    let end = tail.find(':').unwrap_or(tail.len());
    let model = tail[..end].trim();
    (!model.is_empty()).then(|| model.to_string())
}

fn gemini_reasoning_to_responses(config: &Map<String, Value>) -> Option<Value> {
    let thinking = config
        .get("thinkingConfig")
        .or_else(|| config.get("thinking_config"))?
        .as_object()?;
    if let Some(level) = thinking
        .get("thinkingLevel")
        .or_else(|| thinking.get("thinking_level"))
        .and_then(Value::as_str)
        .and_then(normalize_text)
    {
        return Some(json!({ "effort": level.to_ascii_lowercase(), "summary": "auto" }));
    }
    let budget = thinking
        .get("thinkingBudget")
        .or_else(|| thinking.get("thinking_budget"))
        .and_then(Value::as_i64)?;
    let effort = if budget <= 0 {
        "none"
    } else if budget <= 1024 {
        "low"
    } else if budget <= 8192 {
        "medium"
    } else {
        "high"
    };
    Some(json!({ "effort": effort, "summary": "auto" }))
}

fn gemini_system_instruction_to_developer_message(
    system_instruction: Option<&Value>,
) -> Option<Value> {
    gemini_system_instruction_to_text(system_instruction).map(|text| {
        json!({
            "type": "message",
            "role": "developer",
            "content": [{ "type": "input_text", "text": text }],
        })
    })
}

fn gemini_system_instruction_to_text(system_instruction: Option<&Value>) -> Option<String> {
    let system = system_instruction?.as_object()?;
    let parts = system
        .get("parts")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(gemini_part_to_text)
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn gemini_contents_to_input(
    contents: Option<&Value>,
    short_name_map: &BTreeMap<String, String>,
    pending_tool_call_ids: &mut VecDeque<String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Result<Option<Value>, String> {
    let Some(contents) = contents else {
        return Ok(None);
    };
    let items = contents
        .as_array()
        .ok_or_else(|| "gemini contents must be an array".to_string())?;
    let mut out = Vec::new();
    for item in items {
        let Some(content) = item.as_object() else {
            continue;
        };
        let role = match content.get("role").and_then(Value::as_str).map(str::trim) {
            Some("model") => "assistant",
            Some("user") => "user",
            Some(other) if !other.is_empty() => other,
            _ => "user",
        };
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            continue;
        };
        let mut mapped_parts = Vec::new();
        for part in parts {
            let kind = part
                .as_object()
                .and_then(|obj| {
                    if obj.contains_key("functionCall") {
                        Some("functionCall")
                    } else if obj.contains_key("functionResponse") {
                        Some("functionResponse")
                    } else if obj.get("thought").and_then(Value::as_bool) == Some(true)
                        && obj.contains_key("text")
                    {
                        Some("reasoning")
                    } else if obj.contains_key("text") {
                        Some("text")
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            if let Some(mapped) = gemini_part_to_responses(
                part,
                role,
                short_name_map,
                pending_tool_call_ids,
                tool_name_restore_map,
            ) {
                match kind {
                    "text" => mapped_parts.push(mapped),
                    "reasoning" | "functionCall" | "functionResponse" => {
                        if !mapped_parts.is_empty() {
                            out.push(responses_message(role, Value::Array(mapped_parts)));
                            mapped_parts = Vec::new();
                        }
                        out.push(mapped);
                    }
                    _ => {}
                }
            }
        }
        if !mapped_parts.is_empty() {
            out.push(responses_message(role, Value::Array(mapped_parts)));
        }
    }
    Ok((!out.is_empty()).then(|| Value::Array(out)))
}

fn gemini_part_to_text(value: &Value) -> Option<String> {
    let part = value.as_object()?;
    part.get("text")
        .and_then(Value::as_str)
        .and_then(normalize_text)
}

fn gemini_part_to_responses(
    value: &Value,
    role: &str,
    short_name_map: &BTreeMap<String, String>,
    pending_tool_call_ids: &mut VecDeque<String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Option<Value> {
    let part = value.as_object()?;
    if let Some(text) = part
        .get("text")
        .and_then(Value::as_str)
        .and_then(normalize_text)
    {
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            let mut reasoning = Map::new();
            reasoning.insert("type".to_string(), Value::String("reasoning".to_string()));
            reasoning.insert(
                "summary".to_string(),
                Value::Array(vec![json!({
                    "type": "summary_text",
                    "text": text,
                })]),
            );
            if let Some(signature) = part
                .get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                reasoning.insert(
                    "encrypted_content".to_string(),
                    Value::String(signature.to_string()),
                );
            }
            return Some(Value::Object(reasoning));
        }
        return Some(json!({
            "type": text_content_type_for_role(role),
            "text": text,
        }));
    }
    if let Some(function_call) = part.get("functionCall").and_then(Value::as_object) {
        let name = function_call
            .get("name")
            .and_then(Value::as_str)
            .and_then(normalize_text)?;
        let short_name = resolve_short_tool_name(name.as_str(), short_name_map);
        tool_name_restore_map.insert(short_name.clone(), name);
        let call_id = function_call
            .get("id")
            .and_then(Value::as_str)
            .and_then(normalize_text)
            .unwrap_or_else(generate_tool_call_id);
        pending_tool_call_ids.push_back(call_id.clone());
        return Some(json!({
            "type": "function_call",
            "call_id": call_id,
            "name": short_name,
            "arguments": function_call.get("args").cloned().unwrap_or_else(|| json!({})).to_string(),
        }));
    }
    if let Some(function_response) = part.get("functionResponse").and_then(Value::as_object) {
        let output = gemini_function_response_output(function_response);
        let call_id =
            resolve_gemini_function_response_call_id(function_response, pending_tool_call_ids);
        return Some(json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": output,
        }));
    }
    None
}

fn resolve_gemini_function_response_call_id(
    function_response: &Map<String, Value>,
    pending_tool_call_ids: &mut VecDeque<String>,
) -> String {
    if let Some(id) = function_response
        .get("id")
        .and_then(Value::as_str)
        .and_then(normalize_text)
    {
        if let Some(position) = pending_tool_call_ids
            .iter()
            .position(|pending| pending == id.as_str())
        {
            pending_tool_call_ids.remove(position);
            return id;
        }
        return id;
    }
    pending_tool_call_ids
        .pop_front()
        .unwrap_or_else(generate_tool_call_id)
}

fn gemini_tools_to_responses(
    tools: Option<&Value>,
    short_name_map: &BTreeMap<String, String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Result<Option<Value>, String> {
    let Some(tools) = tools else {
        return Ok(None);
    };
    let items = tools
        .as_array()
        .ok_or_else(|| "gemini tools must be an array".to_string())?;
    let mut out = Vec::new();
    for item in items {
        let Some(tool) = item.as_object() else {
            continue;
        };
        let Some(declarations) = gemini_function_declarations(tool) else {
            continue;
        };
        for declaration in declarations {
            let Some(function) = declaration.as_object() else {
                continue;
            };
            let Some(name) = function
                .get("name")
                .and_then(Value::as_str)
                .and_then(normalize_text)
            else {
                continue;
            };
            let short_name = resolve_short_tool_name(name.as_str(), short_name_map);
            tool_name_restore_map.insert(short_name.clone(), name);
            let mut mapped = Map::new();
            mapped.insert("type".to_string(), Value::String("function".to_string()));
            mapped.insert("name".to_string(), Value::String(short_name));
            if let Some(description) = function
                .get("description")
                .and_then(Value::as_str)
                .and_then(normalize_text)
            {
                mapped.insert("description".to_string(), Value::String(description));
            }
            if let Some(parameters) = function
                .get("parameters")
                .or_else(|| function.get("parametersJsonSchema"))
                .cloned()
            {
                mapped.insert(
                    "parameters".to_string(),
                    normalize_gemini_tool_schema_like_cpa(parameters),
                );
            }
            mapped.insert("strict".to_string(), Value::Bool(false));
            out.push(Value::Object(mapped));
        }
    }
    Ok((!out.is_empty()).then(|| Value::Array(out)))
}

fn gemini_tool_choice_to_responses(
    request_obj: &Map<String, Value>,
    short_name_map: &BTreeMap<String, String>,
) -> Value {
    let config = request_obj
        .get("toolConfig")
        .or_else(|| request_obj.get("tool_config"))
        .and_then(Value::as_object)
        .and_then(|tool_config| {
            tool_config
                .get("functionCallingConfig")
                .or_else(|| tool_config.get("function_calling_config"))
        })
        .and_then(Value::as_object);
    let Some(config) = config else {
        return Value::String("auto".to_string());
    };
    let mode = config
        .get("mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_uppercase();
    match mode.as_str() {
        "NONE" => Value::String("none".to_string()),
        "ANY" => {
            let allowed = config
                .get("allowedFunctionNames")
                .or_else(|| config.get("allowed_function_names"))
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().and_then(normalize_text))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if allowed.len() == 1 {
                let name = resolve_short_tool_name(allowed[0].as_str(), short_name_map);
                json!({ "type": "function", "name": name })
            } else {
                Value::String("required".to_string())
            }
        }
        _ => Value::String("auto".to_string()),
    }
}

fn gemini_function_response_output(function_response: &Map<String, Value>) -> Value {
    let Some(response) = function_response.get("response") else {
        return Value::String(String::new());
    };
    for key in ["result", "output", "error"] {
        if let Some(value) = response.get(key) {
            return match value {
                Value::String(text) => Value::String(text.clone()),
                Value::Null => Value::String(String::new()),
                other => Value::String(other.to_string()),
            };
        }
    }
    if let Some(text) = response.as_str() {
        return Value::String(text.to_string());
    }
    Value::String(response.to_string())
}

fn normalize_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn normalize_system_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with("x-anthropic-billing-header: ") {
        return None;
    }
    Some(value.to_string())
}

fn text_content_type_for_role(role: &str) -> &'static str {
    if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    }
}

fn anthropic_image_block_to_responses(block: &Map<String, Value>) -> Option<Value> {
    let source = block.get("source")?.as_object()?;
    let data = source
        .get("data")
        .or_else(|| source.get("base64"))
        .and_then(Value::as_str)
        .and_then(normalize_text)?;
    let mime = source
        .get("media_type")
        .or_else(|| source.get("mime_type"))
        .and_then(Value::as_str)
        .and_then(normalize_text)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    Some(json!({
        "type": "input_image",
        "image_url": format!("data:{mime};base64,{data}"),
    }))
}

fn anthropic_tool_use_block_to_responses(
    block: &Map<String, Value>,
    short_name_map: &BTreeMap<String, String>,
    tool_name_restore_map: &mut ToolNameRestoreMap,
) -> Option<Value> {
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .and_then(normalize_text)?;
    let short_name = resolve_short_tool_name(name.as_str(), short_name_map);
    tool_name_restore_map.insert(short_name.clone(), name);
    Some(json!({
        "type": "function_call",
        "call_id": block.get("id").and_then(Value::as_str).unwrap_or("toolu_unknown"),
        "name": short_name,
        "arguments": block.get("input").cloned().unwrap_or_else(|| json!({})).to_string(),
    }))
}

fn anthropic_tool_result_block_to_responses(block: &Map<String, Value>) -> Option<Value> {
    let call_id = block
        .get("tool_use_id")
        .and_then(Value::as_str)
        .and_then(normalize_text)?;
    let output = match block.get("content") {
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(normalize_text)
                {
                    parts.push(json!({ "type": "input_text", "text": text }));
                } else if let Some(obj) = item.as_object() {
                    if let Some(image) = anthropic_image_block_to_responses(obj) {
                        parts.push(image);
                    }
                }
            }
            Value::Array(parts)
        }
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(other) => other.clone(),
        None => Value::String(String::new()),
    };
    Some(json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output,
    }))
}

fn shorten_tool_name(name: &str) -> String {
    const LIMIT: usize = 64;
    if name.len() <= LIMIT {
        return name.to_string();
    }
    if let Some(rest) = name
        .strip_prefix("mcp__")
        .and_then(|value| value.rsplit_once("__").map(|(_, tail)| tail))
    {
        let candidate = format!("mcp__{rest}");
        return candidate.chars().take(LIMIT).collect();
    }
    name.chars().take(LIMIT).collect()
}

fn resolve_short_tool_name(name: &str, short_name_map: &BTreeMap<String, String>) -> String {
    short_name_map
        .get(name)
        .cloned()
        .unwrap_or_else(|| shorten_tool_name(name))
}

fn build_short_name_map(names: &[String]) -> BTreeMap<String, String> {
    const LIMIT: usize = 64;
    let mut used = BTreeMap::<String, ()>::new();
    let mut mapped = BTreeMap::new();

    let base_candidate = |name: &str| -> String {
        if name.len() <= LIMIT {
            return name.to_string();
        }
        if let Some(rest) = name
            .strip_prefix("mcp__")
            .and_then(|value| value.rsplit_once("__").map(|(_, tail)| tail))
        {
            let candidate = format!("mcp__{rest}");
            return candidate.chars().take(LIMIT).collect();
        }
        name.chars().take(LIMIT).collect()
    };

    for name in names {
        let candidate = base_candidate(name);
        let unique = if !used.contains_key(&candidate) {
            candidate
        } else {
            let mut seq = 1usize;
            loop {
                let suffix = format!("_{seq}");
                let allowed = LIMIT.saturating_sub(suffix.len());
                let mut base = candidate.clone();
                if base.len() > allowed {
                    base = base.chars().take(allowed).collect();
                }
                let next = format!("{base}{suffix}");
                if !used.contains_key(&next) {
                    break next;
                }
                seq += 1;
            }
        };
        used.insert(unique.clone(), ());
        mapped.insert(name.clone(), unique);
    }

    mapped
}

fn declared_short_name_map_for_anthropic_tools(tools: Option<&Value>) -> BTreeMap<String, String> {
    let names = tools
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("name")
                        .and_then(Value::as_str)
                        .and_then(normalize_text)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    build_short_name_map(&names)
}

fn declared_short_name_map_for_gemini_tools(tools: Option<&Value>) -> BTreeMap<String, String> {
    let mut names = Vec::new();
    if let Some(items) = tools.and_then(Value::as_array) {
        for item in items {
            let Some(tool) = item.as_object() else {
                continue;
            };
            if let Some(declarations) = gemini_function_declarations(tool) {
                for declaration in declarations {
                    if let Some(name) = declaration
                        .get("name")
                        .and_then(Value::as_str)
                        .and_then(normalize_text)
                    {
                        names.push(name);
                    }
                }
            }
        }
    }
    build_short_name_map(&names)
}

fn gemini_function_declarations(tool: &Map<String, Value>) -> Option<&Vec<Value>> {
    tool.get("functionDeclarations")
        .or_else(|| tool.get("function_declarations"))
        .and_then(Value::as_array)
}

fn normalize_tool_schema(mut schema: Value) -> Value {
    if let Some(obj) = schema.as_object_mut() {
        obj.remove("$schema");
        if !obj.contains_key("type") {
            obj.insert("type".to_string(), Value::String("object".to_string()));
        }
        if obj
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "object")
            && !obj.contains_key("properties")
        {
            obj.insert("properties".to_string(), Value::Object(Map::new()));
        }
        obj.insert("additionalProperties".to_string(), Value::Bool(false));
    }
    schema
}

fn normalize_gemini_tool_schema_like_cpa(mut schema: Value) -> Value {
    normalize_gemini_schema_types_like_cpa(&mut schema);
    if let Some(obj) = schema.as_object_mut() {
        obj.remove("$schema");
        obj.insert("additionalProperties".to_string(), Value::Bool(false));
    }
    schema
}

fn normalize_gemini_schema_types_like_cpa(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            for (key, child) in obj.iter_mut() {
                if key == "type" {
                    if let Some(kind) = child.as_str() {
                        *child = Value::String(kind.to_ascii_lowercase());
                    }
                } else {
                    normalize_gemini_schema_types_like_cpa(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_gemini_schema_types_like_cpa(item);
            }
        }
        _ => {}
    }
}

fn generate_tool_call_id() -> String {
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();
    format!("call_{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{
        adapt_openai_responses_to_anthropic_messages, adapt_request_for_protocol,
        backfill_empty_gemini_function_response_names, gemini_function_response_output,
    };
    use crate::apikey_profile::{PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_GEMINI_NATIVE};
    use crate::gateway::{GeminiStreamOutputMode, ResponseAdapter};
    use serde_json::{json, Value};

    #[test]
    fn anthropic_messages_are_rewritten_to_responses() {
        let body = br#"{"model":"claude-3-7-sonnet","system":"be helpful","messages":[{"role":"user","content":"hi"}],"stream":true}"#.to_vec();

        let adapted = adapt_request_for_protocol(PROTOCOL_ANTHROPIC_NATIVE, "/v1/messages", body)
            .expect("adapt anthropic request");

        assert_eq!(adapted.path, "/v1/responses");
        assert_eq!(
            adapted.response_adapter,
            ResponseAdapter::AnthropicMessagesFromResponses
        );
        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["model"], "claude-3-7-sonnet");
        assert_eq!(payload["instructions"], "");
        assert_eq!(payload["input"][0]["type"], "message");
        assert_eq!(payload["input"][0]["role"], "developer");
        assert_eq!(payload["input"][0]["content"][0]["text"], "be helpful");
        assert_eq!(payload["input"][1]["type"], "message");
        assert_eq!(payload["input"][1]["role"], "user");
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["reasoning"]["effort"], "medium");
        assert_eq!(payload["include"][0], "reasoning.encrypted_content");
        assert_eq!(payload["parallel_tool_calls"], true);
    }

    #[test]
    fn anthropic_mid_conversation_system_messages_map_to_developer() {
        let body = serde_json::json!({
            "model": "claude-sonnet",
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "system", "content": "# MCP Server Instructions" },
                { "role": "developer", "content": "runtime note" },
                { "role": "unknown", "content": "fallback note" }
            ],
            "stream": true
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_ANTHROPIC_NATIVE,
            "/v1/messages?beta=true",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt anthropic request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["input"][0]["role"], "user");
        assert_eq!(payload["input"][1]["role"], "developer");
        assert_eq!(
            payload["input"][1]["content"][0]["text"],
            "# MCP Server Instructions"
        );
        assert_eq!(payload["input"][2]["role"], "developer");
        assert_eq!(payload["input"][2]["content"][0]["text"], "runtime note");
        assert_eq!(payload["input"][3]["role"], "user");
        assert_eq!(payload["input"][3]["content"][0]["text"], "fallback note");
    }

    #[test]
    fn gemini_generate_content_is_rewritten_to_responses() {
        let body = br#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#.to_vec();

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gemini-2.5-pro:generateContent",
            body,
        )
        .expect("adapt gemini request");

        assert_eq!(adapted.path, "/v1/responses");
        assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiJson);
        assert_eq!(adapted.gemini_stream_output_mode, None);
        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["model"], "gemini-2.5-pro");
        assert_eq!(payload["instructions"], "");
        assert_eq!(payload["input"][0]["type"], "message");
        assert_eq!(payload["input"][0]["role"], "user");
        assert_eq!(payload["reasoning"]["effort"], "medium");
        assert_eq!(payload["include"][0], "reasoning.encrypted_content");
        assert_eq!(payload["parallel_tool_calls"], true);
        assert!(payload.get("tools").is_none());
    }

    #[test]
    fn gemini_cli_wrapped_generate_content_rewrites_request_tools_and_history() {
        let body = serde_json::json!({
            "model": "gpt-5.4",
            "request": {
                "systemInstruction": {
                    "parts": [{ "text": "use tools when writing files" }]
                },
                "contents": [
                    { "role": "user", "parts": [{ "text": "write the plan" }] },
                    {
                        "role": "model",
                        "parts": [{
                            "functionCall": {
                                "id": "WriteFile_1",
                                "name": "WriteFile",
                                "args": { "file_path": "plans/site.md", "content": "plan" }
                            }
                        }]
                    },
                    {
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "id": "WriteFile_1",
                                "name": "WriteFile",
                                "response": { "output": "ok" }
                            }
                        }]
                    }
                ],
                "tools": [{
                    "function_declarations": [{
                        "name": "WriteFile",
                        "description": "Write a file",
                        "parameters": {
                            "type": "OBJECT",
                            "properties": {
                                "file_path": { "type": "STRING" },
                                "content": { "type": "STRING" }
                            }
                        }
                    }]
                }],
                "generationConfig": {
                    "thinkingConfig": { "thinkingBudget": 1024 }
                }
            }
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1internal:streamGenerateContent?alt=sse",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini cli request");

        assert_eq!(adapted.path, "/v1/responses");
        assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiCliSse);
        assert_eq!(
            adapted.gemini_stream_output_mode,
            Some(GeminiStreamOutputMode::Sse)
        );
        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["model"], "gpt-5.4");
        assert_eq!(payload["input"][0]["role"], "developer");
        assert_eq!(payload["input"][1]["role"], "user");
        assert_eq!(payload["input"][2]["type"], "function_call");
        assert_eq!(payload["input"][2]["name"], "WriteFile");
        assert_eq!(payload["input"][3]["type"], "function_call_output");
        assert_eq!(
            payload["input"][3]["call_id"],
            payload["input"][2]["call_id"]
        );
        assert_eq!(payload["tools"][0]["type"], "function");
        assert_eq!(payload["tools"][0]["name"], "WriteFile");
        assert_eq!(
            payload["tools"][0]["parameters"]["additionalProperties"],
            false
        );
        assert_eq!(payload["tools"][0]["parameters"]["type"], "object");
        assert_eq!(
            payload["tools"][0]["parameters"]["properties"]["file_path"]["type"],
            "string"
        );
        assert_eq!(payload["tools"].as_array().expect("tools").len(), 1);
        assert_eq!(payload["tool_choice"], "auto");
        assert_eq!(payload["reasoning"]["effort"], "low");
    }

    #[test]
    fn gemini_stream_generate_content_uses_sse_stream_mode_without_alt_sse() {
        let body = br#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#.to_vec();

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent",
            body,
        )
        .expect("adapt gemini request");

        assert_eq!(
            adapted.gemini_stream_output_mode,
            Some(GeminiStreamOutputMode::Sse)
        );
        assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiSse);
    }

    #[test]
    fn gemini_cli_internal_stream_defaults_to_sse_wrapped_mode() {
        let body = br#"{"model":"gpt-5.4","request":{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}}"#.to_vec();

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1internal:streamGenerateContent",
            body,
        )
        .expect("adapt gemini cli request");

        assert_eq!(
            adapted.gemini_stream_output_mode,
            Some(GeminiStreamOutputMode::Sse)
        );
        assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiCliSse);
    }

    #[test]
    fn anthropic_tool_use_and_result_are_rewritten_as_responses_tool_items() {
        let long_tool_name = "mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore";
        let body = br#"{
            "model":"claude-3-7-sonnet",
            "messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","input":{"q":"hi"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":[{"type":"text","text":"ok"}]}]}
            ],
            "tools":[{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","input_schema":{"type":"object","properties":{"q":{"type":"string"}}}}]
        }"#.to_vec();

        let adapted = adapt_request_for_protocol(PROTOCOL_ANTHROPIC_NATIVE, "/v1/messages", body)
            .expect("adapt anthropic request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["input"][0]["type"], "function_call");
        assert_eq!(payload["input"][0]["call_id"], "toolu_1");
        assert_eq!(payload["input"][1]["type"], "function_call_output");
        assert_eq!(payload["input"][1]["call_id"], "toolu_1");
        assert_eq!(payload["tools"][0]["type"], "function");
        assert_ne!(payload["tools"][0]["name"], long_tool_name);
        assert_eq!(payload["tool_choice"], "auto");
        assert_eq!(
            adapted
                .tool_name_restore_map
                .get(payload["tools"][0]["name"].as_str().unwrap_or("")),
            Some(&long_tool_name.to_string())
        );
    }

    #[test]
    fn gemini_function_call_and_response_are_rewritten_as_responses_tool_items() {
        let long_tool_name = "mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore";
        let body = br#"{
            "contents":[
                {"role":"model","parts":[{"functionCall":{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","args":{"q":"hi"}}}]},
                {"role":"user","parts":[{"functionResponse":{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","response":{"result":"ok"}}}]}
            ],
            "tools":[{"functionDeclarations":[{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","parameters":{"type":"object","properties":{"q":{"type":"string"}}}}]}
            ]
        }"#.to_vec();

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gemini-2.5-pro:generateContent",
            body,
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["input"][0]["type"], "function_call");
        assert_eq!(payload["input"][1]["type"], "function_call_output");
        assert_eq!(
            payload["input"][0]["call_id"],
            payload["input"][1]["call_id"]
        );
        assert_eq!(payload["tools"][0]["type"], "function");
        assert_ne!(payload["tools"][0]["name"], long_tool_name);
        assert_eq!(payload["tools"].as_array().expect("tools").len(), 1);
        assert_eq!(payload["tool_choice"], "auto");
        assert_eq!(
            adapted
                .tool_name_restore_map
                .get(payload["tools"][0]["name"].as_str().unwrap_or("")),
            Some(&long_tool_name.to_string())
        );
    }

    #[test]
    fn gemini_function_call_id_pairs_exactly_with_function_response_id() {
        let body = serde_json::json!({
            "contents": [
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "id": "call_exact_from_gemini",
                            "name": "ReadFolder",
                            "args": { "path": "." }
                        }
                    }]
                },
                {
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "id": "call_exact_from_gemini",
                            "name": "ReadFolder",
                            "response": { "output": "Directory is empty." }
                        }
                    }]
                }
            ]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gpt-5.4:streamGenerateContent?alt=sse",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["input"][0]["type"], "function_call");
        assert_eq!(payload["input"][1]["type"], "function_call_output");
        assert_eq!(
            payload["input"][1]["call_id"],
            payload["input"][0]["call_id"]
        );
        assert_eq!(payload["input"][0]["call_id"], "call_exact_from_gemini");
        assert_eq!(payload["input"][1]["output"], "Directory is empty.");
    }

    #[test]
    fn gemini_function_response_without_id_uses_fifo_for_user_and_function_roles() {
        let body = serde_json::json!({
            "contents": [
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "id": "call_user_role",
                            "name": "ReadFile",
                            "args": { "path": "a.md" }
                        }
                    }]
                },
                {
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": "",
                            "response": { "result": "A" }
                        }
                    }]
                },
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "id": "call_function_role",
                            "name": "ReadFile",
                            "args": { "path": "b.md" }
                        }
                    }]
                },
                {
                    "role": "function",
                    "parts": [{
                        "functionResponse": {
                            "response": { "result": "B" }
                        }
                    }]
                }
            ]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1internal:streamGenerateContent",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["input"][0]["call_id"], "call_user_role");
        assert_eq!(payload["input"][1]["type"], "function_call_output");
        assert_eq!(payload["input"][1]["call_id"], "call_user_role");
        assert_eq!(payload["input"][2]["call_id"], "call_function_role");
        assert_eq!(payload["input"][3]["type"], "function_call_output");
        assert_eq!(payload["input"][3]["call_id"], "call_function_role");
        assert_eq!(payload["input"][1]["output"], "A");
        assert_eq!(payload["input"][3]["output"], "B");
    }

    #[test]
    fn gemini_function_response_output_prefers_output_and_error_fields() {
        let output_value = json!({
            "response": {
                "output": "Directory is empty."
            }
        });
        let error_value = json!({
            "response": {
                "error": {
                    "message": "params must have required property 'command'"
                }
            }
        });

        let output = gemini_function_response_output(
            output_value.as_object().expect("functionResponse object"),
        );
        let error = gemini_function_response_output(
            error_value.as_object().expect("functionResponse object"),
        );

        assert_eq!(output, Value::String("Directory is empty.".to_string()));
        assert_eq!(
            error,
            Value::String(
                "{\"message\":\"params must have required property 'command'\"}".to_string()
            )
        );
    }

    #[test]
    fn gemini_function_response_names_are_backfilled_from_previous_model_call() {
        let contents = json!([
            {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "ReadFile",
                        "args": { "path": "a.md" }
                    }
                }]
            },
            {
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": "",
                        "response": { "output": "A" }
                    }
                }]
            },
            {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "ReadFile",
                        "args": { "path": "b.md" }
                    }
                }]
            },
            {
                "role": "function",
                "parts": [{
                    "functionResponse": {
                        "response": { "output": "B" }
                    }
                }]
            }
        ]);

        let backfilled = backfill_empty_gemini_function_response_names(&contents);
        assert_eq!(
            backfilled[1]["parts"][0]["functionResponse"]["name"],
            "ReadFile"
        );
        assert_eq!(
            backfilled[3]["parts"][0]["functionResponse"]["name"],
            "ReadFile"
        );
    }

    #[test]
    fn anthropic_disable_parallel_tool_use_and_unique_short_names_follow_cpa_shape() {
        let tool_a =
            "mcp__workspace__ThisIsAnExtremelyLongToolNameThatNeedsToBeShortenedForCodexRouteAlpha";
        let tool_b =
            "mcp__workspace__ThisIsAnExtremelyLongToolNameThatNeedsToBeShortenedForCodexRouteBeta";
        let body = serde_json::json!({
            "model": "claude-sonnet",
            "tool_choice": {
                "type": "tool",
                "name": tool_b,
                "disable_parallel_tool_use": true
            },
            "tools": [
                { "name": tool_a, "input_schema": {"description":"x"} },
                { "name": tool_b, "input_schema": {"$schema":"http://json-schema.org/draft-07/schema#"} }
            ],
            "messages": [{
                "role": "assistant",
                "content": [{ "type": "tool_use", "id": "toolu_1", "name": tool_b, "input": {} }]
            }]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_ANTHROPIC_NATIVE,
            "/v1/messages",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt anthropic request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["parallel_tool_calls"], false);
        assert_ne!(payload["tools"][0]["name"], payload["tools"][1]["name"]);
        assert_eq!(payload["tool_choice"], "auto");
        assert_eq!(payload["input"][0]["name"], payload["tools"][1]["name"]);
        assert_eq!(payload["tools"][0]["parameters"]["type"], "object");
        assert!(payload["tools"][0]["parameters"]["properties"].is_object());
        assert!(payload["tools"][1]["parameters"].get("$schema").is_none());
    }

    #[test]
    fn gemini_any_mode_allowed_function_name_maps_to_specific_function_tool_choice() {
        let long_tool_name = "mcp__workspace__ReadFileLongLongLongLongLongLongLongLongLongLong";
        let body = serde_json::json!({
            "tools": [{
                "functionDeclarations": [{
                    "name": long_tool_name,
                    "parameters": {"description":"x"}
                }]
            }],
            "toolConfig": {
                "functionCallingConfig": {
                    "mode": "ANY",
                    "allowedFunctionNames": [long_tool_name]
                }
            },
            "contents": [{"role":"user","parts":[{"text":"hi"}]}]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gemini-2.5-pro:generateContent",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["tool_choice"]["type"], "function");
        assert_eq!(payload["tool_choice"]["name"], payload["tools"][0]["name"]);
        assert_eq!(payload["tools"][0]["parameters"]["description"], "x");
        assert_eq!(
            payload["tools"][0]["parameters"]["additionalProperties"],
            false
        );
        assert!(payload["tools"][0]["parameters"].get("type").is_none());
        assert!(payload["tools"][0]["parameters"]
            .get("properties")
            .is_none());
        assert_eq!(payload["tools"].as_array().expect("tools").len(), 1);
    }

    #[test]
    fn gemini_any_mode_without_single_allowed_function_requires_tool_call() {
        let body = serde_json::json!({
            "tools": [{
                "functionDeclarations": [{
                    "name": "WriteFile",
                    "parameters": {"type":"object","properties":{}}
                }]
            }],
            "toolConfig": {
                "functionCallingConfig": { "mode": "ANY" }
            },
            "contents": [{"role":"user","parts":[{"text":"write it"}]}]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1internal:streamGenerateContent",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["tool_choice"], "required");
    }

    #[test]
    fn gemini_none_mode_disables_tool_choice() {
        let body = serde_json::json!({
            "tools": [{
                "functionDeclarations": [{
                    "name": "WriteFile",
                    "parameters": {"type":"object","properties":{}}
                }]
            }],
            "toolConfig": {
                "functionCallingConfig": { "mode": "NONE" }
            },
            "contents": [{"role":"user","parts":[{"text":"do not use tools"}]}]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1internal:streamGenerateContent",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["tool_choice"], "none");
    }

    #[test]
    fn anthropic_enabled_thinking_adds_reasoning_and_include_only_when_explicit() {
        let body = serde_json::json!({
            "model": "claude-sonnet",
            "thinking": {
                "type": "enabled",
                "budget_tokens": 4096
            },
            "messages": [{"role":"user","content":"hi"}]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_ANTHROPIC_NATIVE,
            "/v1/messages",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt anthropic request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["reasoning"]["effort"], "medium");
        assert_eq!(payload["include"][0], "reasoning.encrypted_content");
    }

    #[test]
    fn responses_request_rewrites_to_anthropic_messages_for_claude_upstream() {
        let body = json!({
            "model": "gpt-5.3",
            "instructions": "be direct",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "hello" }]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"/tmp/a\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "done"
                }
            ],
            "tools": [{
                "type": "function",
                "name": "read_file",
                "description": "Read a file",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }
            }],
            "reasoning": { "effort": "high" },
            "stream": true
        });

        let adapted = adapt_openai_responses_to_anthropic_messages(
            serde_json::to_vec(&body).expect("body").as_slice(),
            Some("deepseek/deepseek-v4-pro"),
        )
        .expect("adapt responses request");
        let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

        assert_eq!(payload["model"], "deepseek/deepseek-v4-pro");
        assert_eq!(payload["system"], "be direct");
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][0]["content"][0]["type"], "text");
        assert_eq!(payload["messages"][0]["content"][0]["text"], "hello");
        assert_eq!(payload["messages"][1]["role"], "assistant");
        assert_eq!(payload["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(payload["messages"][1]["content"][0]["id"], "call_1");
        assert_eq!(
            payload["messages"][1]["content"][0]["input"]["path"],
            "/tmp/a"
        );
        assert_eq!(payload["messages"][2]["role"], "user");
        assert_eq!(payload["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(payload["tools"][0]["name"], "read_file");
        assert_eq!(payload["thinking"]["type"], "enabled");
    }

    #[test]
    fn responses_reasoning_budget_stays_below_max_tokens() {
        let body = json!({
            "model": "gpt-5.3",
            "max_output_tokens": 2048,
            "input": "think carefully",
            "reasoning": { "effort": "high" }
        });

        let adapted = adapt_openai_responses_to_anthropic_messages(
            serde_json::to_vec(&body).expect("body").as_slice(),
            Some("claude-sonnet-4"),
        )
        .expect("adapt responses request");
        let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

        let max_tokens = payload["max_tokens"].as_i64().expect("max tokens");
        let budget_tokens = payload["thinking"]["budget_tokens"]
            .as_i64()
            .expect("budget tokens");
        assert!(
            budget_tokens < max_tokens,
            "Anthropic thinking budget_tokens must be lower than max_tokens"
        );
    }

    #[test]
    fn responses_request_drops_images_for_deepseek_anthropic_bridge() {
        let body = json!({
            "model": "gpt-5.3",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "describe this" },
                    { "type": "input_image", "image_url": "data:image/png;base64,aGVsbG8=" }
                ]
            }]
        });

        let adapted = adapt_openai_responses_to_anthropic_messages(
            serde_json::to_vec(&body).expect("body").as_slice(),
            Some("deepseek-v4-pro"),
        )
        .expect("adapt responses request");
        let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

        assert_eq!(
            payload["messages"][0]["content"].as_array().unwrap().len(),
            1
        );
        assert_eq!(payload["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn gemini_thinking_config_adds_reasoning_and_include_only_when_explicit() {
        let body = serde_json::json!({
            "contents": [{"role":"user","parts":[{"text":"hi"}]}],
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingBudget": 2048
                }
            }
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gemini-2.5-pro:generateContent",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["reasoning"]["effort"], "medium");
        assert_eq!(payload["include"][0], "reasoning.encrypted_content");
    }

    #[test]
    fn anthropic_disabled_thinking_maps_to_none_effort_like_cpa() {
        let body = serde_json::json!({
            "model": "claude-sonnet",
            "thinking": { "type": "disabled" },
            "messages": [{"role":"user","content":"hi"}]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_ANTHROPIC_NATIVE,
            "/v1/messages",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt anthropic request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["reasoning"]["effort"], "none");
    }

    #[test]
    fn gemini_assistant_text_maps_to_output_text_like_cpa() {
        let body = serde_json::json!({
            "contents": [{"role":"model","parts":[{"text":"hello"}]}]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gemini-2.5-pro:generateContent",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["input"][0]["role"], "assistant");
        assert_eq!(payload["input"][0]["content"][0]["type"], "output_text");
    }

    #[test]
    fn gemini_thought_text_maps_to_reasoning_history_not_visible_output() {
        let body = serde_json::json!({
            "contents": [{
                "role":"model",
                "parts":[{
                    "text":"internal plan",
                    "thought":true,
                    "thoughtSignature":"sig_reasoning"
                }]
            }]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_GEMINI_NATIVE,
            "/v1beta/models/gemini-2.5-pro:generateContent",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt gemini request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["input"][0]["type"], "reasoning");
        assert_eq!(payload["input"][0]["summary"][0]["text"], "internal plan");
        assert_eq!(payload["input"][0]["encrypted_content"], "sig_reasoning");
    }

    #[test]
    fn anthropic_web_search_tool_maps_to_codex_web_search_like_cpa() {
        let body = serde_json::json!({
            "model": "claude-sonnet",
            "messages": [{"role":"user","content":"hi"}],
            "tools": [{ "type": "web_search_20250305", "name": "search" }]
        });

        let adapted = adapt_request_for_protocol(
            PROTOCOL_ANTHROPIC_NATIVE,
            "/v1/messages",
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt anthropic request");

        let payload: serde_json::Value =
            serde_json::from_slice(&adapted.body).expect("parse adapted body");
        assert_eq!(payload["tools"][0]["type"], "web_search");
        assert!(payload["tools"][0].get("name").is_none());
    }
}
