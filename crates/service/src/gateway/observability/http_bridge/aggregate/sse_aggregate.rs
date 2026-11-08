use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Cursor};

use super::openai_responses_event::{OpenAIResponsesEvent, OpenAIResponsesOutputTextState};
use super::{
    append_output_text_raw, collect_output_text_from_event_fields, collect_response_output_text,
    inspect_sse_frame, is_response_completed_event_name, merge_usage, parse_sse_frame_json,
    UpstreamResponseUsage,
};

#[path = "chat_function_calls.rs"]
mod chat_function_calls;
use chat_function_calls::ChatFunctionCalls;

#[derive(Debug, Clone, Default)]
struct ChatCompletionChoiceSynthesis {
    role: Option<String>,
    content: String,
    finish_reason: Option<Value>,
    tool_calls: ChatFunctionCalls,
}

#[derive(Debug, Clone, Default)]
struct ChatCompletionSseSynthesis {
    id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
    system_fingerprint: Option<Value>,
    usage: Option<Value>,
    choices: BTreeMap<i64, ChatCompletionChoiceSynthesis>,
    saw_terminal: bool,
}

#[derive(Debug, Clone, Default)]
struct ResponsesSseSynthesis {
    id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
    usage: Option<Value>,
    output_text: String,
    output_items: BTreeMap<i64, Value>,
    next_output_index: i64,
    saw_completed: bool,
}

/// 函数 `reserve_output_index`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - synthesis: 参数 synthesis
/// - explicit_index: 参数 explicit_index
///
/// # 返回
/// 返回函数执行结果
fn reserve_output_index(synthesis: &mut ResponsesSseSynthesis, explicit_index: Option<i64>) -> i64 {
    if let Some(index) = explicit_index {
        synthesis.next_output_index = synthesis.next_output_index.max(index + 1);
        index
    } else {
        let index = synthesis.next_output_index;
        synthesis.next_output_index += 1;
        index
    }
}

/// 函数 `merge_response_output_item_event`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - synthesis: 参数 synthesis
/// - value: 参数 value
///
/// # 返回
/// 无
fn merge_response_output_item_event(synthesis: &mut ResponsesSseSynthesis, value: &Value) {
    let Some(event_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };

    match event_type {
        "response.output_item.added" | "response.output_item.done" => {
            let Some(item) = value.get("item").or_else(|| value.get("output_item")) else {
                return;
            };
            let explicit_index = value
                .get("output_index")
                .and_then(Value::as_i64)
                .or_else(|| item.get("index").and_then(Value::as_i64));
            let index = reserve_output_index(synthesis, explicit_index);
            let mut stored = item.clone();
            if let Some(stored_obj) = stored.as_object_mut() {
                stored_obj
                    .entry("index".to_string())
                    .or_insert(index.into());
                if let Some(existing_obj) = synthesis
                    .output_items
                    .get(&index)
                    .and_then(Value::as_object)
                {
                    for field in ["id", "call_id", "name", "arguments"] {
                        if stored_obj.get(field).is_none() {
                            if let Some(existing_value) = existing_obj.get(field) {
                                stored_obj.insert(field.to_string(), existing_value.clone());
                            }
                        }
                    }
                }
            }
            synthesis.output_items.insert(index, stored);
        }
        "response.function_call_arguments.delta"
        | "response.function_call_arguments.done"
        | "response.custom_tool_call_input.delta"
        | "response.custom_tool_call_input.done" => {
            let fragment = value
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| value.get("arguments").and_then(Value::as_str))
                .or_else(|| value.get("input").and_then(Value::as_str))
                .unwrap_or_default();
            let explicit_index = value.get("output_index").and_then(Value::as_i64);
            let index = reserve_output_index(synthesis, explicit_index);
            let fallback_type = if event_type.starts_with("response.custom_tool_call_input.") {
                "custom_tool_call"
            } else {
                "function_call"
            };
            let entry = synthesis
                .output_items
                .entry(index)
                .or_insert_with(|| json!({ "type": fallback_type, "index": index }));
            if !entry.is_object() {
                *entry = json!({ "type": fallback_type, "index": index });
            }
            let Some(entry_obj) = entry.as_object_mut() else {
                return;
            };
            if let Some(call_id) = value
                .get("call_id")
                .or_else(|| value.get("item_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if event_type.starts_with("response.custom_tool_call_input.") {
                    entry_obj
                        .entry("id".to_string())
                        .or_insert_with(|| Value::String(call_id.to_string()));
                } else {
                    entry_obj
                        .entry("call_id".to_string())
                        .or_insert_with(|| Value::String(call_id.to_string()));
                }
            }
            let mut arguments = entry_obj
                .get("arguments")
                .and_then(Value::as_str)
                .or_else(|| entry_obj.get("input").and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_default();
            if event_type.ends_with(".delta") {
                // Repeated fragments are valid; only done events are snapshots.
                arguments.push_str(fragment);
            } else {
                arguments = fragment.to_string();
            }
            if !arguments.is_empty() {
                let field = if event_type.starts_with("response.custom_tool_call_input.") {
                    "input"
                } else {
                    "arguments"
                };
                entry_obj.insert(field.to_string(), Value::String(arguments));
            }
        }
        _ => {}
    }
}

/// 函数 `append_chat_delta_content`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - buffer: 参数 buffer
/// - delta_content: 参数 delta_content
///
/// # 返回
/// 无
fn append_chat_delta_content(buffer: &mut String, delta_content: &Value) {
    if let Some(text) = delta_content.as_str() {
        buffer.push_str(text);
        return;
    }
    let Some(parts) = delta_content.as_array() else {
        return;
    };
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            buffer.push_str(text);
        }
    }
}

/// 函数 `update_chat_completion_sse_synthesis`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - synthesis: 参数 synthesis
/// - value: 参数 value
///
/// # 返回
/// 无
fn update_chat_completion_sse_synthesis(synthesis: &mut ChatCompletionSseSynthesis, value: &Value) {
    if value.get("object").and_then(Value::as_str) != Some("chat.completion.chunk") {
        return;
    }
    if synthesis.id.is_none() {
        synthesis.id = value
            .get("id")
            .and_then(Value::as_str)
            .map(|v| v.to_string());
    }
    if synthesis.model.is_none() {
        synthesis.model = value
            .get("model")
            .and_then(Value::as_str)
            .map(|v| v.to_string());
    }
    if synthesis.created.is_none() {
        synthesis.created = value.get("created").and_then(Value::as_i64);
    }
    if synthesis.system_fingerprint.is_none() {
        synthesis.system_fingerprint = value.get("system_fingerprint").cloned();
    }
    if let Some(usage) = value.get("usage") {
        synthesis.usage = Some(usage.clone());
    }

    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return;
    };
    for (position, choice) in choices.iter().enumerate() {
        let index = choice
            .get("index")
            .and_then(Value::as_i64)
            .unwrap_or(position as i64);
        let target = synthesis.choices.entry(index).or_default();
        if target.role.is_none() {
            target.role = choice
                .get("delta")
                .and_then(|delta| delta.get("role"))
                .and_then(Value::as_str)
                .map(|v| v.to_string());
        }
        if let Some(delta_content) = choice.get("delta").and_then(|delta| delta.get("content")) {
            append_chat_delta_content(&mut target.content, delta_content);
        }
        if let Some(delta) = choice.get("delta") {
            target.tool_calls.merge_delta(delta);
        }
        if let Some(finish_reason) = choice.get("finish_reason") {
            if !finish_reason.is_null() {
                target.finish_reason = Some(finish_reason.clone());
                synthesis.saw_terminal = true;
            }
        }
    }
}

/// 函数 `update_responses_sse_synthesis`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - synthesis: 参数 synthesis
/// - value: 参数 value
///
/// # 返回
/// 无
fn update_responses_sse_synthesis(synthesis: &mut ResponsesSseSynthesis, value: &Value) {
    let Some(event_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };

    if synthesis.id.is_none() {
        synthesis.id = value
            .get("response_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_string));
    }
    if synthesis.model.is_none() {
        synthesis.model = value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("model"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
    }
    if synthesis.created.is_none() {
        synthesis.created = value
            .get("created")
            .and_then(Value::as_i64)
            .or_else(|| value.get("created_at").and_then(Value::as_i64))
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("created"))
                    .and_then(Value::as_i64)
            })
            .or_else(|| {
                value
                    .get("response")
                    .and_then(|response| response.get("created_at"))
                    .and_then(Value::as_i64)
            });
    }

    if let Some(response_usage) = value
        .get("response")
        .and_then(|response| response.get("usage"))
        .cloned()
    {
        synthesis.usage = Some(response_usage);
    } else if synthesis.usage.is_none() {
        if let Some(usage) = value.get("usage").cloned() {
            synthesis.usage = Some(usage);
        }
    }

    let mut text_out = String::new();
    match event_type {
        "response.output_text.delta" | "response.content_part.delta" => {
            if let Some(delta) = value.get("delta") {
                collect_response_output_text(delta, &mut text_out);
            }
        }
        "response.output_text.done" => {
            if synthesis.output_text.trim().is_empty() {
                if let Some(text) = value.get("text") {
                    collect_response_output_text(text, &mut text_out);
                } else if let Some(delta) = value.get("delta") {
                    collect_response_output_text(delta, &mut text_out);
                }
            }
        }
        "response.content_part.done" => {
            if synthesis.output_text.trim().is_empty() {
                collect_output_text_from_event_fields(value, &mut text_out);
                if text_out.trim().is_empty() {
                    if let Some(delta) = value.get("delta") {
                        collect_response_output_text(delta, &mut text_out);
                    }
                }
            }
        }
        _ => {}
    }
    if !text_out.trim().is_empty() {
        append_output_text_raw(&mut synthesis.output_text, text_out.as_str());
    }
    merge_response_output_item_event(synthesis, value);

    if is_response_completed_event_name(event_type) {
        synthesis.saw_completed = true;
    }
}

/// 函数 `response_has_effective_output`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - response: 参数 response
///
/// # 返回
/// 返回函数执行结果
fn response_has_effective_output(response: &Value) -> bool {
    let mut output_text = String::new();
    if let Some(output) = response.get("output") {
        collect_response_output_text(output, &mut output_text);
    }
    !output_text.trim().is_empty()
}

/// 函数 `build_response_output_items_from_text`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - text: 参数 text
///
/// # 返回
/// 返回函数执行结果
fn build_response_output_items_from_text(text: &str) -> Value {
    Value::Array(vec![json!({
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": text
        }]
    })])
}

/// 函数 `build_response_output_items_from_sse`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - synthesis: 参数 synthesis
///
/// # 返回
/// 返回函数执行结果
fn build_response_output_items_from_sse(synthesis: &ResponsesSseSynthesis) -> Option<Value> {
    if synthesis.output_items.is_empty() {
        return None;
    }
    Some(Value::Array(
        synthesis.output_items.values().cloned().collect::<Vec<_>>(),
    ))
}

/// 函数 `enrich_completed_response_with_sse_text`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - completed_response: 参数 completed_response
/// - synthesis: 参数 synthesis
///
/// # 返回
/// 返回函数执行结果
fn enrich_completed_response_with_sse_text(
    completed_response: Value,
    synthesis: &ResponsesSseSynthesis,
) -> Value {
    let mut response = completed_response;
    let Some(response_obj) = response.as_object_mut() else {
        return response;
    };

    if response_obj
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(|id| id.is_empty())
    {
        if let Some(id) = synthesis.id.as_ref() {
            response_obj.insert("id".to_string(), Value::String(id.clone()));
        }
    }
    if response_obj
        .get("model")
        .and_then(Value::as_str)
        .is_none_or(|model| model.is_empty())
    {
        if let Some(model) = synthesis.model.as_ref() {
            response_obj.insert("model".to_string(), Value::String(model.clone()));
        }
    }
    if response_obj
        .get("created")
        .and_then(Value::as_i64)
        .is_none()
    {
        if let Some(created) = synthesis.created {
            response_obj.insert("created".to_string(), Value::Number(created.into()));
        }
    }
    if !response_obj.contains_key("object") {
        response_obj.insert("object".to_string(), Value::String("response".to_string()));
    }
    if !response_obj.contains_key("status") {
        response_obj.insert("status".to_string(), Value::String("completed".to_string()));
    }
    if response_obj.get("usage").is_none() {
        if let Some(usage) = synthesis.usage.as_ref() {
            response_obj.insert("usage".to_string(), usage.clone());
        }
    }

    let has_structured_output = response_obj
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    let had_effective_output = response_has_effective_output(&Value::Object(response_obj.clone()));
    let mut inserted_output_from_delta_text = false;
    if !has_structured_output {
        if let Some(output_items) = build_response_output_items_from_sse(synthesis) {
            response_obj.insert("output".to_string(), output_items);
        } else if !had_effective_output && !synthesis.output_text.trim().is_empty() {
            response_obj.insert(
                "output".to_string(),
                build_response_output_items_from_text(synthesis.output_text.as_str()),
            );
            inserted_output_from_delta_text = true;
        }
    }
    let has_effective_output = response_has_effective_output(&Value::Object(response_obj.clone()));
    if (inserted_output_from_delta_text || !has_effective_output)
        && response_obj
            .get("output_text")
            .and_then(Value::as_str)
            .is_none_or(|text| text.trim().is_empty())
        && !synthesis.output_text.trim().is_empty()
    {
        response_obj.insert(
            "output_text".to_string(),
            Value::String(synthesis.output_text.trim().to_string()),
        );
    }

    Value::Object(response_obj.clone())
}

/// 函数 `synthesize_response_body_from_sse`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - synthesis: 参数 synthesis
///
/// # 返回
/// 返回函数执行结果
fn synthesize_response_body_from_sse(synthesis: &ResponsesSseSynthesis) -> Option<Vec<u8>> {
    let output_items = build_response_output_items_from_sse(synthesis);
    let output_items_have_text = output_items.as_ref().is_some_and(|output_items| {
        let mut response = serde_json::Map::new();
        response.insert("output".to_string(), output_items.clone());
        response_has_effective_output(&Value::Object(response))
    });
    if !synthesis.saw_completed
        || (synthesis.output_text.trim().is_empty() && output_items.is_none())
    {
        return None;
    }
    let created = synthesis.created.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    });
    let mut out = serde_json::Map::new();
    out.insert(
        "id".to_string(),
        Value::String(
            synthesis
                .id
                .clone()
                .unwrap_or_else(|| "resp_proxy".to_string()),
        ),
    );
    out.insert("object".to_string(), Value::String("response".to_string()));
    out.insert("created".to_string(), Value::Number(created.into()));
    out.insert(
        "model".to_string(),
        Value::String(
            synthesis
                .model
                .clone()
                .unwrap_or_else(|| super::super::DEFAULT_BRIDGE_MODEL.to_string()),
        ),
    );
    out.insert("status".to_string(), Value::String("completed".to_string()));
    out.insert(
        "output".to_string(),
        output_items
            .unwrap_or_else(|| build_response_output_items_from_text(synthesis.output_text.trim())),
    );
    if !output_items_have_text && !synthesis.output_text.trim().is_empty() {
        out.insert(
            "output_text".to_string(),
            Value::String(synthesis.output_text.trim().to_string()),
        );
    }
    if let Some(usage) = synthesis.usage.clone() {
        out.insert("usage".to_string(), usage);
    }
    serde_json::to_vec(&Value::Object(out)).ok()
}

/// 函数 `synthesize_chat_completion_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - synthesis: 参数 synthesis
///
/// # 返回
/// 返回函数执行结果
fn synthesize_chat_completion_body(synthesis: &ChatCompletionSseSynthesis) -> Option<Vec<u8>> {
    if !synthesis.saw_terminal || synthesis.choices.is_empty() {
        return None;
    }
    let created = synthesis.created.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    });

    let choices = synthesis
        .choices
        .iter()
        .map(|(index, choice)| {
            let tool_calls = choice.tool_calls.snapshot();
            let content = if !tool_calls.is_empty() && choice.content.is_empty() {
                Value::Null
            } else {
                json!(choice.content)
            };
            let mut value = json!({
                "index": index,
                "message": {
                    "role": choice.role.clone().unwrap_or_else(|| "assistant".to_string()),
                    "content": content,
                },
                "finish_reason": choice.finish_reason.clone().unwrap_or(Value::Null),
            });
            if !tool_calls.is_empty() {
                value["message"]["tool_calls"] = Value::Array(tool_calls);
            }
            value
        })
        .collect::<Vec<_>>();
    let mut out = serde_json::Map::new();
    out.insert(
        "id".to_string(),
        Value::String(
            synthesis
                .id
                .clone()
                .unwrap_or_else(|| "chatcmpl_proxy".to_string()),
        ),
    );
    out.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    out.insert("created".to_string(), Value::Number(created.into()));
    out.insert(
        "model".to_string(),
        Value::String(
            synthesis
                .model
                .clone()
                .unwrap_or_else(|| super::super::DEFAULT_BRIDGE_MODEL.to_string()),
        ),
    );
    out.insert("choices".to_string(), Value::Array(choices));
    if let Some(system_fingerprint) = synthesis.system_fingerprint.clone() {
        out.insert("system_fingerprint".to_string(), system_fingerprint);
    }
    if let Some(usage) = synthesis.usage.clone() {
        out.insert("usage".to_string(), usage);
    }
    serde_json::to_vec(&Value::Object(out)).ok()
}

fn merge_sse_frame_usage(
    usage: &mut UpstreamResponseUsage,
    responses_text_state: &mut OpenAIResponsesOutputTextState,
    frame: &[String],
) {
    if let Some(event) = OpenAIResponsesEvent::parse(frame) {
        if event
            .event_type
            .as_deref()
            .is_some_and(|event_type| event_type.starts_with("response."))
        {
            event.merge_usage_into(usage, responses_text_state);
            return;
        }
    }

    let inspection = inspect_sse_frame(frame);
    if let Some(parsed_usage) = inspection.usage {
        merge_usage(usage, parsed_usage);
    }
}

/// 函数 `collect_non_stream_json_from_sse_bytes`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
fn terminal_response_snapshot(value: &Value) -> Option<Value> {
    let event = value.get("type").and_then(Value::as_str)?;
    let status = match event {
        "response.completed" | "response.done" => "completed",
        "response.incomplete" => "incomplete",
        "response.failed" | "response.error" | "response.cancelled" | "response.canceled"
        | "error" => "failed",
        _ => return None,
    };
    if status == "completed" && value.get("response").is_none() {
        return None;
    }
    let mut response = value
        .get("response")
        .cloned()
        .unwrap_or_else(|| value.clone());
    if let Some(object) = response.as_object_mut() {
        object.entry("status").or_insert_with(|| json!(status));
        if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
            object.entry("error").or_insert_with(|| error.clone());
        }
    }
    Some(response)
}

fn enrich_terminal_response(response: Value, synthesis: &ResponsesSseSynthesis) -> Value {
    if response.get("status").and_then(Value::as_str) == Some("completed")
        && response.get("error").is_none_or(Value::is_null)
        && response
            .pointer("/status_details/error")
            .is_none_or(Value::is_null)
    {
        return enrich_completed_response_with_sse_text(response, synthesis);
    }
    let mut response = response;
    // Preserve failed/incomplete envelopes and never promote partial tool
    // buffers into executable calls. A known output limit may retain text that
    // was actually received before its terminal snapshot omitted that text.
    if super::super::response_outcome::is_output_limit_incomplete(&response) {
        let mut text = String::new();
        collect_response_output_text(&response, &mut text);
        if text.trim().is_empty() && !synthesis.output_text.trim().is_empty() {
            let partial = json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":synthesis.output_text}]});
            if let Some(items) = response.get_mut("output").and_then(Value::as_array_mut) {
                items.push(partial);
            } else {
                response["output"] = json!([partial]);
            }
        }
    }
    response
}

pub(in super::super) fn collect_non_stream_json_from_sse_bytes(
    payload: &[u8],
) -> (Option<Vec<u8>>, UpstreamResponseUsage) {
    let mut usage = UpstreamResponseUsage::default();
    let mut terminal_response: Option<Value> = None;
    let mut responses_sse_synthesis = ResponsesSseSynthesis::default();
    let mut responses_usage_text_state = OpenAIResponsesOutputTextState::default();
    let mut chat_completion_synthesis = ChatCompletionSseSynthesis::default();
    let mut frame_lines: Vec<String> = Vec::new();

    let mut reader = BufReader::new(Cursor::new(payload));
    let mut line = String::new();
    loop {
        line.clear();
        let Ok(read) = reader.read_line(&mut line) else {
            break;
        };
        if read == 0 {
            break;
        }
        if line == "\n" || line == "\r\n" {
            if frame_lines.is_empty() {
                continue;
            }
            let frame = std::mem::take(&mut frame_lines);
            merge_sse_frame_usage(&mut usage, &mut responses_usage_text_state, &frame);
            if let Some(value) = parse_sse_frame_json(&frame) {
                update_responses_sse_synthesis(&mut responses_sse_synthesis, &value);
                update_chat_completion_sse_synthesis(&mut chat_completion_synthesis, &value);
                if terminal_response.is_none() {
                    terminal_response = terminal_response_snapshot(&value);
                }
            } else if let Some(super::SseTerminal::Err(message)) =
                inspect_sse_frame(&frame).terminal
            {
                terminal_response = Some(json!({"object":"response","status":"failed","error":{
                    "type":"upstream_error","code":"upstream_response_failed","message":message
                }}));
            }
            if terminal_response.is_some() {
                break;
            }
            continue;
        }
        frame_lines.push(line.clone());
    }

    if !frame_lines.is_empty() {
        merge_sse_frame_usage(&mut usage, &mut responses_usage_text_state, &frame_lines);
        if let Some(value) = parse_sse_frame_json(&frame_lines) {
            update_responses_sse_synthesis(&mut responses_sse_synthesis, &value);
            update_chat_completion_sse_synthesis(&mut chat_completion_synthesis, &value);
            if terminal_response.is_none() {
                terminal_response = terminal_response_snapshot(&value);
            }
        } else if let Some(super::SseTerminal::Err(message)) =
            inspect_sse_frame(&frame_lines).terminal
        {
            terminal_response = Some(json!({"object":"response","status":"failed","error":{
                "type":"upstream_error","code":"upstream_response_failed","message":message
            }}));
        }
    }

    let body = terminal_response
        .map(|value| enrich_terminal_response(value, &responses_sse_synthesis))
        .and_then(|value| serde_json::to_vec(&value).ok())
        .or_else(|| synthesize_response_body_from_sse(&responses_sse_synthesis))
        .or_else(|| synthesize_chat_completion_body(&chat_completion_synthesis));
    (body, usage)
}

#[cfg(test)]
mod terminal_snapshot_tests {
    use super::*;

    #[test]
    fn non_stream_sse_preserves_first_failed_terminal_instead_of_later_success() {
        let wire = concat!(
            "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"quota\",\"message\":\"failure\"},\"output\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":2}}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"call_id\":\"must_not_appear\"}]}}\n\n"
        );
        let (body, usage) = collect_non_stream_json_from_sse_bytes(wire.as_bytes());
        let body: Value = serde_json::from_slice(&body.unwrap()).unwrap();
        assert_eq!(body["status"], "failed");
        assert_eq!(body["error"]["code"], "quota");
        assert!(body["output"].as_array().unwrap().is_empty());
        assert_eq!(usage.output_tokens, Some(2));
    }

    #[test]
    fn non_stream_output_limit_retains_received_text_without_synthesizing_partial_tools() {
        let wire = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial text\"}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"partial_call\",\"name\":\"lookup\",\"arguments\":\"{\"}}\n\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"output\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":2}}}\n\n"
        );
        let (body, _) = collect_non_stream_json_from_sse_bytes(wire.as_bytes());
        let body: Value = serde_json::from_slice(&body.unwrap()).unwrap();
        assert_eq!(body["status"], "incomplete");
        assert_eq!(body["output"].as_array().unwrap().len(), 1);
        assert_eq!(body["output"][0]["type"], "message");
        assert_eq!(body["output"][0]["content"][0]["text"], "partial text");
    }
}

/// 函数 `looks_like_sse_payload`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn looks_like_sse_payload(body: &[u8]) -> bool {
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    let mut saw_sse_prefix = false;
    for line in text.lines().take(32) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            if saw_sse_prefix {
                return true;
            }
            continue;
        }
        if trimmed.starts_with("data:")
            || trimmed.starts_with("event:")
            || trimmed.starts_with("id:")
            || trimmed.starts_with("retry:")
            || trimmed.starts_with(':')
        {
            saw_sse_prefix = true;
            continue;
        }
        if !saw_sse_prefix {
            return false;
        }
    }
    saw_sse_prefix
}
