//! Preserve Codex Desktop's attached page context without inventing tool calls.
use serde_json::{json, Value};

pub(crate) fn normalize_companion_page_context(path: &str, payload: &mut Value) -> usize {
    if !matches!(
        path.split('?').next(),
        Some("/v1/responses" | "/v1/responses/compact")
    ) {
        return 0;
    }
    let Some(input) = payload.get_mut("input") else {
        return 0;
    };
    match input {
        Value::Array(items) => items
            .iter_mut()
            .map(|item| usize::from(normalize_item(item)))
            .sum(),
        item => usize::from(normalize_item(item)),
    }
}

fn normalize_item(item: &mut Value) -> bool {
    let Some(object) = item.as_object() else {
        return false;
    };
    if object.get("type").and_then(Value::as_str) != Some("function_call_output") {
        return false;
    }
    match object.get("call_id") {
        None | Some(Value::Null) => {}
        Some(Value::String(id)) if id.trim().is_empty() => {}
        _ => return false,
    }
    match object.get("namespace") {
        None => {}
        Some(Value::String(namespace)) if namespace == "agent" => {}
        _ => return false,
    }
    let Some(name) = object.get("name").and_then(Value::as_str) else {
        return false;
    };
    if !is_companion_page_name(name) {
        return false;
    }
    let Some(output) = object.get("output").and_then(Value::as_str) else {
        return false;
    };
    // These Desktop attachments are context, not results of model-issued calls.
    // Keep their source and exact text at user priority, rather than assigning a
    // fake call_id or attaching them to an unrelated real tool invocation.
    let source = match object.get("namespace").and_then(Value::as_str) {
        Some(namespace) => format!("{namespace}.{name}"),
        None => name.to_string(),
    };
    *item = json!({
        "type": "message",
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": format!("Companion Page context ({source}):\n{output}")
        }]
    });
    true
}

fn is_companion_page_name(name: &str) -> bool {
    if name == "codex_apps_open_page" {
        return true;
    }
    let Some(parts) = name.strip_prefix("codex_apps_selected_page_part_") else {
        return false;
    };
    let Some((part, total)) = parts.split_once("_of_") else {
        return false;
    };
    let positive_integer = |value: &str| {
        matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
            && value.bytes().all(|byte| byte.is_ascii_digit())
    };
    positive_integer(part) && positive_integer(total)
}

#[cfg(test)]
#[path = "companion_page_context_tests.rs"]
mod tests;
