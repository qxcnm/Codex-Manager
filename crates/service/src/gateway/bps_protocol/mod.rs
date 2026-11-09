//! CodexManager's Responses-to-BPS adapter. BPS is an experimental product
//! protocol; successful conversion is not evidence of model quality or access.
mod constraints;
mod request;
mod stream;
mod tools;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

pub(crate) use constraints::validate_original_constraints;
pub(crate) use request::prepare;

#[derive(Clone)]
struct ClientTool {
    name: String,
    namespace: Option<String>,
    custom: bool,
    declaration: Value,
}

#[derive(Default)]
enum ToolChoice {
    #[default]
    Auto,
    None,
    Required,
    Named(String),
}

#[derive(Default)]
pub(crate) struct Bridge {
    tools: BTreeMap<String, ClientTool>,
    omitted_tools: Vec<String>,
    choice: ToolChoice,
    pending_tool_ids: HashSet<String>,
    terminal: bool,
    sequence: u64,
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    let text = value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty());
    text.ok_or_else(|| format!("BPS requires a non-empty {key}"))
}

fn fingerprint(value: &impl serde::Serialize) -> String {
    let encoded = serde_json::to_vec(value).expect("JSON value is serializable");
    let digest = Sha256::digest(encoded);
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "BPS input item must be an object".into())
}

#[cfg(test)]
mod tests;
