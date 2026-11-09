use super::{fingerprint, object, required_string, string, tools, Bridge};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) fn prepare(body: &[u8], session_scope: &str) -> Result<(Vec<u8>, Bridge), String> {
    let source: Value = serde_json::from_slice(body).map_err(|_| "invalid BPS request JSON")?;
    object(&source)?;
    let model = required_string(&source, "model")?;
    super::constraints::validate_responses_constraints(&source)?;
    let history = match &source["input"] {
        Value::String(text) => vec![message("user", text)],
        Value::Array(items) => items.clone(),
        _ => return Err("BPS input must be text or a complete Responses history array".into()),
    };
    let mut bridge = Bridge::default();
    let explicit_effort = source
        .pointer("/reasoning/effort")
        .and_then(Value::as_str)
        .or_else(|| source["reasoning_effort"].as_str())
        .filter(|s| !s.is_empty());
    let mut effort = explicit_effort;
    for item in &history {
        if string(item, "type") == "additional_tools" {
            bridge.collect_tools(&item["tools"], None, 0)?;
        }
        if string(item, "type") == "configuration_update" && explicit_effort.is_none() {
            if let Some(updated) = item
                .pointer("/reasoning/effort")
                .and_then(Value::as_str)
                .or_else(|| item["reasoning_effort"].as_str())
            {
                effort = Some(updated);
            }
        }
        if string(item, "type") == "configuration_update"
            && object(item)?.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "type" | "id" | "reasoning" | "reasoning_effort"
                )
            })
        {
            return Err("BPS cannot preserve this historical configuration update".into());
        }
    }
    // The current declaration is authoritative over historical dynamic updates.
    bridge.collect_tools(&source["tools"], None, 0)?;
    bridge.set_choice(&source["tool_choice"])?;
    let effort = normalize_effort(effort.unwrap_or("medium"))?;
    let mut input = Vec::new();
    if let Some(instructions) = source.get("instructions").filter(|v| !v.is_null()) {
        let instructions = instructions
            .as_str()
            .ok_or("BPS instructions must be text")?;
        if !instructions.is_empty() {
            input.push(message("developer", instructions));
        }
    }
    let mut prompt = bridge.tool_prompt();
    if let Some(verbosity) = source.pointer("/text/verbosity").and_then(Value::as_str) {
        match verbosity {
            "low" => prompt.push_str("\nKeep the answer concise."),
            "high" => prompt.push_str("\nProvide a detailed answer."),
            "medium" => {}
            _ => return Err("unsupported BPS text verbosity".into()),
        }
    }
    input.push(message("developer", &prompt));
    input.extend(translate_history(&history)?);
    let cache_key = source["prompt_cache_key"]
        .as_str()
        .filter(|v| !v.is_empty());
    let conversation = cache_key
        .map(str::to_owned)
        .unwrap_or_else(|| fingerprint(&history.first()));
    let turn_end = history
        .iter()
        .rposition(|v| string(v, "role") == "user")
        .map(|i| i + 1)
        .unwrap_or(0);
    let iteration = history[turn_end..]
        .iter()
        .filter(|v| {
            matches!(
                string(v, "type"),
                "function_call_output" | "custom_tool_call_output"
            )
        })
        .count()
        + 1;
    let mut output = json!({
        "model":model,"model_selection":"explicit","reasoning_effort":effort,
        "stream":true,"store":false,"input":input,
        "context_management":[{"type":"compaction","compact_threshold":200000}],
        "metadata":{
            "task_id":fingerprint(&(session_scope,conversation)),
            "turn_id":fingerprint(&(session_scope,&history[..turn_end])),
            "agent_iteration":iteration.to_string()
        }
    });
    if let Some(include) = source.get("include").filter(|v| !v.is_null()) {
        let fields = include.as_array().ok_or("BPS include must be an array")?;
        if fields
            .iter()
            .any(|v| v.as_str() != Some("reasoning.encrypted_content"))
        {
            return Err("BPS cannot provide the requested include fields".into());
        }
        // This is a Responses output selector, not a BPS request field.
        // Codex normalization injects it for reasoning requests; forwarding it
        // makes BPS reject an otherwise valid generation. Opaque reasoning
        // content returned by BPS is preserved by the response bridge.
    }
    if let Some(management) = source.get("context_management").filter(|v| !v.is_null()) {
        let items = management
            .as_array()
            .ok_or("BPS context_management must be an array")?;
        if items.iter().any(|v| {
            string(v, "type") != "compaction"
                || v["compact_threshold"].as_u64().is_none_or(|n| n == 0)
        }) {
            return Err("BPS only supports compaction with a positive compact_threshold".into());
        }
        output["context_management"] = management.clone();
    }
    if let Some(key) = cache_key {
        output["prompt_cache_key"] =
            json!(format!("cm-bps-{}", fingerprint(&(session_scope, key))));
    }
    let encoded = serde_json::to_vec(&output).map_err(|_| "could not encode BPS request")?;
    Ok((encoded, bridge))
}

fn normalize_effort(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "medium" => Ok("medium"),
        "low" | "minimal" | "none" => Ok("low"),
        "high" => Ok("high"),
        "xhigh" | "x-high" | "extra-high" | "extra_high" | "max" | "ultra" => Ok("xhigh"),
        _ => Err("BPS reasoning effort is unsupported".into()),
    }
}

fn message(role: &str, text: &str) -> Value {
    json!({"type":"message","role":role,"content":[{"type":"input_text","text":text}]})
}

fn content(value: &Value) -> Result<Value, String> {
    if let Some(text) = value.as_str() {
        return Ok(json!([{"type":"input_text","text":text}]));
    }
    let items = value
        .as_array()
        .ok_or("BPS message content must be text or an array")?;
    let mut output = Vec::with_capacity(items.len());
    for item in items {
        let converted = match string(item, "type") {
            "input_text" | "output_text" | "text" => {
                json!({"type":"input_text","text":item.get("text").and_then(Value::as_str).ok_or("BPS text content is missing")?})
            }
            "input_image" | "image_url" => {
                let raw = item["image_url"].as_str().or_else(|| item.pointer("/image_url/url").and_then(Value::as_str)).ok_or("BPS requires an HTTPS image URL")?;
                let url = url::Url::parse(raw).map_err(|_| "BPS image URL is invalid")?;
                if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                    return Err("BPS image input requires an HTTPS URL without embedded credentials; inline images are unsupported".into());
                }
                let mut image = json!({"type":"input_image","image_url":raw});
                if let Some(detail) = item.get("detail").or_else(|| item.pointer("/image_url/detail")) { image["detail"] = detail.clone(); }
                image
            }
            "refusal" => json!({"type":"input_text","text":item.get("refusal").and_then(Value::as_str).ok_or("BPS refusal text is missing")?}),
            _ => return Err("BPS cannot preserve this content type; files, audio, and inline images require the original route".into()),
        };
        output.push(converted);
    }
    Ok(Value::Array(output))
}

fn translate_history(history: &[Value]) -> Result<Vec<Value>, String> {
    let mut output = Vec::new();
    let mut calls = HashSet::new();
    let mut results = HashSet::new();
    for item in history {
        object(item)?;
        match string(item, "type") {
            "additional_tools" | "configuration_update" => {}
            "item_reference" => {
                return Err("BPS requires expanded history instead of item_reference".into())
            }
            "function_call" | "custom_tool_call" => {
                let id = required_string(item, "call_id")?;
                if !calls.insert(id.to_owned()) {
                    return Err("BPS history contains duplicate tool call IDs".into());
                }
                output.push(tools::history_call(item)?);
            }
            "function_call_output" | "custom_tool_call_output" => {
                let id = required_string(item, "call_id")?;
                if !calls.contains(id) {
                    return Err(
                        "BPS tool result is missing its original call; provide complete history"
                            .into(),
                    );
                }
                if !results.insert(id.to_owned()) {
                    return Err("BPS history contains duplicate tool results".into());
                }
                let result = item
                    .get("output")
                    .ok_or("BPS tool result is missing output")?;
                let result = if result.is_string() {
                    result.clone()
                } else {
                    content(result)?
                };
                output.push(json!({"type":"function_call_output","call_id":id,"id":format!("fc_{}",fingerprint(&(id,"output"))),"output":result}));
            }
            "reasoning" => {
                if let Some(encrypted) = item
                    .get("encrypted_content")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                {
                    output.push(
                        json!({"type":"reasoning","summary":[],"encrypted_content":encrypted}),
                    );
                }
            }
            "compaction" => {
                required_string(item, "encrypted_content")?;
                output.push(item.clone());
            }
            "compaction_trigger" => output.push(item.clone()),
            "agent_message" => {
                // Opaque agent state is preserved together with its visible content.
                let mut message = item.clone();
                message
                    .as_object_mut()
                    .expect("object checked")
                    .remove("internal_chat_message_metadata_passthrough");
                let parts = item["content"]
                    .as_array()
                    .ok_or("BPS agent message content must be an array")?;
                for part in parts {
                    if string(part, "type") == "encrypted_content" {
                        required_string(part, "encrypted_content")?;
                    } else {
                        content(&json!([part]))?;
                    }
                }
                output.push(message);
            }
            "message" | "" => {
                let role = required_string(item, "role")?;
                if !matches!(role, "user" | "assistant" | "system" | "developer") {
                    return Err("BPS message role is unsupported".into());
                }
                let mut message =
                    json!({"type":"message","role":role,"content":content(&item["content"])?});
                for field in ["id", "phase"] {
                    if let Some(value) = item.get(field) {
                        message[field] = value.clone();
                    }
                }
                output.push(message);
            }
            _ => {
                return Err(
                    "BPS cannot preserve this history item type; disable BPS for this request"
                        .into(),
                )
            }
        }
    }
    Ok(output)
}
