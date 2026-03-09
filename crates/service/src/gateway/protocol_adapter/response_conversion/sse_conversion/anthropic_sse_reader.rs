use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::super::json_conversion::parse_tool_arguments_as_object;

pub(super) fn convert_anthropic_sse_to_json(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    let text = std::str::from_utf8(body).map_err(|_| "invalid anthropic sse bytes".to_string())?;
    let mut current_event: Option<String> = None;
    let mut response_id = "msg_codexmanager".to_string();
    let mut response_model = "unknown".to_string();
    let mut input_tokens: i64 = 0;
    let mut output_tokens: i64 = 0;
    let mut stop_reason = "end_turn".to_string();
    let mut content_blocks: BTreeMap<usize, Value> = BTreeMap::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.starts_with("event:") {
            current_event = Some(line.trim_start_matches("event:").trim().to_string());
            continue;
        }
        if !line.starts_with("data:") {
            continue;
        }
        let payload = line.trim_start_matches("data:").trim();
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            continue;
        };
        if current_event.as_deref() == Some("error") {
            let bytes = serde_json::to_vec(&value)
                .map_err(|err| format!("serialize anthropic error json failed: {err}"))?;
            return Ok((bytes, "application/json"));
        }
        match current_event.as_deref() {
            Some("message_start") => {
                if let Some(message) = value.get("message") {
                    response_id = message
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("msg_codexmanager")
                        .to_string();
                    response_model = message
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    input_tokens = message
                        .get("usage")
                        .and_then(|usage| usage.get("input_tokens"))
                        .and_then(Value::as_i64)
                        .unwrap_or(input_tokens);
                }
            }
            Some("content_block_start") => {
                let index = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                    .unwrap_or(content_blocks.len());
                if let Some(block) = value.get("content_block") {
                    content_blocks.insert(index, block.clone());
                }
            }
            Some("content_block_delta") => {
                let index = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                    .unwrap_or(0);
                let delta_type = value
                    .get("delta")
                    .and_then(|delta| delta.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if delta_type == "input_json_delta" {
                    let partial_json = value
                        .get("delta")
                        .and_then(|delta| delta.get("partial_json"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let input_value = parse_tool_arguments_as_object(partial_json);
                    let entry = content_blocks.entry(index).or_insert_with(|| {
                        json!({
                            "type": "tool_use",
                            "input": {},
                        })
                    });
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("input".to_string(), input_value);
                    }
                } else if delta_type == "thinking_delta" {
                    let fragment = value
                        .get("delta")
                        .and_then(|delta| delta.get("thinking").or_else(|| delta.get("text")))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let entry = content_blocks.entry(index).or_insert_with(|| {
                        json!({
                            "type": "thinking",
                            "thinking": "",
                        })
                    });
                    let existing = entry
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let mut merged = existing.to_string();
                    merged.push_str(fragment);
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("type".to_string(), Value::String("thinking".to_string()));
                        obj.insert("thinking".to_string(), Value::String(merged));
                    }
                } else if delta_type == "signature_delta" {
                    let fragment = value
                        .get("delta")
                        .and_then(|delta| delta.get("signature"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let entry = content_blocks.entry(index).or_insert_with(|| {
                        json!({
                            "type": "thinking",
                            "thinking": "",
                        })
                    });
                    let existing = entry
                        .get("signature")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let mut merged = existing.to_string();
                    merged.push_str(fragment);
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("type".to_string(), Value::String("thinking".to_string()));
                        obj.insert("signature".to_string(), Value::String(merged));
                    }
                } else {
                    let fragment = value
                        .get("delta")
                        .and_then(|delta| delta.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let entry = content_blocks.entry(index).or_insert_with(|| {
                        json!({
                            "type": "text",
                            "text": "",
                        })
                    });
                    if let Some(existing) = entry.get("text").and_then(Value::as_str) {
                        let mut merged = existing.to_string();
                        merged.push_str(fragment);
                        if let Some(obj) = entry.as_object_mut() {
                            obj.insert("text".to_string(), Value::String(merged));
                        }
                    }
                }
            }
            Some("message_delta") => {
                if let Some(reason) = value
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    stop_reason = reason.to_string();
                }
                output_tokens = value
                    .get("usage")
                    .and_then(|usage| usage.get("output_tokens"))
                    .and_then(Value::as_i64)
                    .unwrap_or(output_tokens);
            }
            _ => {}
        }
    }

    let mut blocks = content_blocks
        .into_iter()
        .map(|(_, block)| block)
        .collect::<Vec<_>>();
    if blocks.is_empty() {
        blocks.push(json!({
            "type": "text",
            "text": "",
        }));
    }

    let out = json!({
        "id": response_id,
        "type": "message",
        "role": "assistant",
        "model": response_model,
        "content": blocks,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    });
    let bytes = serde_json::to_vec(&out)
        .map_err(|err| format!("serialize anthropic json failed: {err}"))?;
    Ok((bytes, "application/json"))
}

