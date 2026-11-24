//! Preserve Codex Desktop's attached context without inventing tool calls.
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
    let Some(output_type) = object
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| kind.ends_with("_output"))
    else {
        return false;
    };
    match object.get("call_id") {
        None | Some(Value::Null) => {}
        Some(Value::String(id)) if id.trim().is_empty() => {}
        _ => return false,
    }
    let output = match object.get("output") {
        Some(Value::String(output)) => output.clone(),
        Some(output) => output.to_string(),
        // Some attachments carry context in fields such as `goal` or `tools`
        // instead of `output`. Retain the full item as text in that case.
        None => item.to_string(),
    };
    // These Desktop attachments are context, not results of model-issued calls.
    // Desktop uses this shape for goals and automations as well as page context;
    // names and namespaces are not a stable allowlist. Real tool results with a
    // non-empty call_id must remain untouched, even for an unfamiliar tool type.
    // Keep their source and exact text at user priority, rather than assigning a
    // fake call_id or attaching them to an unrelated real tool invocation.
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(output_type);
    let source = match object
        .get("namespace")
        .and_then(Value::as_str)
        .filter(|namespace| !namespace.trim().is_empty())
    {
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

#[cfg(test)]
#[path = "companion_page_context_tests.rs"]
mod tests;
