use serde_json::{json, Value};

pub(super) fn convert_anthropic_json_to_sse(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "invalid anthropic json response".to_string())?;
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "error")
    {
        let mut out = String::new();
        append_sse_event(&mut out, "error", &value);
        return Ok((out.into_bytes(), "text/event-stream"));
    }

    let response_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_codexmanager");
    let response_model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let input_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let stop_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn");
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut out = String::new();
    append_sse_event(
        &mut out,
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": response_id,
                "type": "message",
                "role": "assistant",
                "model": response_model,
                "content": [],
                "stop_reason": Value::Null,
                "stop_sequence": Value::Null,
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": 0,
                }
            }
        }),
    );

    let mut content_block_index = 0usize;
    for block in content {
        let Some(block_obj) = block.as_object() else {
            continue;
        };
        let block_type = block_obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match block_type {
            "text" => {
                let text = block_obj
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                append_sse_event(
                    &mut out,
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": content_block_index,
                        "content_block": { "type": "text", "text": "" }
                    }),
                );
                if !text.is_empty() {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": content_block_index,
                            "delta": { "type": "text_delta", "text": text }
                        }),
                    );
                }
                append_sse_event(
                    &mut out,
                    "content_block_stop",
                    &json!({
                        "type": "content_block_stop",
                        "index": content_block_index,
                    }),
                );
                content_block_index += 1;
            }
            "tool_use" => {
                let tool_input = block_obj.get("input").cloned().unwrap_or_else(|| json!({}));
                append_sse_event(
                    &mut out,
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": content_block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": block_obj.get("id").cloned().unwrap_or_else(|| Value::String(format!("toolu_{}", content_block_index))),
                            "name": block_obj.get("name").cloned().unwrap_or_else(|| Value::String("tool".to_string())),
                            "input": json!({})
                        }
                    }),
                );
                if let Some(partial_json) = to_tool_input_partial_json(&tool_input) {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": content_block_index,
                            "delta": {
                                "type": "input_json_delta",
                                "partial_json": partial_json,
                            }
                        }),
                    );
                }
                append_sse_event(
                    &mut out,
                    "content_block_stop",
                    &json!({
                        "type": "content_block_stop",
                        "index": content_block_index,
                    }),
                );
                content_block_index += 1;
            }
            "thinking" => {
                let thinking = block_obj
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let signature = block_obj
                    .get("signature")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                append_sse_event(
                    &mut out,
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": content_block_index,
                        "content_block": { "type": "thinking", "thinking": "" }
                    }),
                );
                if !thinking.is_empty() {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": content_block_index,
                            "delta": { "type": "thinking_delta", "thinking": thinking }
                        }),
                    );
                }
                if let Some(signature) = signature.filter(|value| !value.is_empty()) {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": content_block_index,
                            "delta": { "type": "signature_delta", "signature": signature }
                        }),
                    );
                }
                append_sse_event(
                    &mut out,
                    "content_block_stop",
                    &json!({
                        "type": "content_block_stop",
                        "index": content_block_index,
                    }),
                );
                content_block_index += 1;
            }
            _ => {}
        }
    }

    if content_block_index == 0 {
        append_sse_event(
            &mut out,
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            }),
        );
        append_sse_event(
            &mut out,
            "content_block_stop",
            &json!({
                "type": "content_block_stop",
                "index": 0,
            }),
        );
    }

    append_sse_event(
        &mut out,
        "message_delta",
        &json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": Value::Null },
            "usage": { "output_tokens": output_tokens }
        }),
    );
    append_sse_event(&mut out, "message_stop", &json!({ "type": "message_stop" }));

    Ok((out.into_bytes(), "text/event-stream"))
}

pub(super) fn to_tool_input_partial_json(value: &Value) -> Option<String> {
    let serialized = serde_json::to_string(value).ok()?;
    if serialized == "{}" {
        return None;
    }
    Some(serialized)
}

pub(super) fn append_sse_event(buffer: &mut String, event_name: &str, payload: &Value) {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    buffer.push_str("event: ");
    buffer.push_str(event_name);
    buffer.push('\n');
    buffer.push_str("data: ");
    buffer.push_str(&data);
    buffer.push_str("\n\n");
}
