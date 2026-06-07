use super::*;
use codexmanager_core::rpc::types::{ModelInfo, ModelsResponse};
use serde_json::Value;

/// 函数 `serialize_models_response_outputs_codex_and_api_shapes`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn serialize_models_response_outputs_codex_and_api_shapes() {
    let items = ModelsResponse {
        models: vec![
            ModelInfo {
                slug: "gpt-5.3-codex".to_string(),
                display_name: "GPT-5.3 Codex".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
            ModelInfo {
                slug: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
        ],
        extra: std::collections::BTreeMap::from([(
            "etag".to_string(),
            serde_json::json!("\"abc\""),
        )]),
        ..Default::default()
    };
    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    assert_eq!(value.get("object").and_then(Value::as_str), Some("list"));
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .expect("OpenAI-compatible data array");
    assert_eq!(data.len(), 2);
    assert_eq!(
        data[0].get("id").and_then(Value::as_str),
        Some("gpt-5.3-codex")
    );
    assert_eq!(data[0].get("object").and_then(Value::as_str), Some("model"));
    assert_eq!(
        data[0].get("owned_by").and_then(Value::as_str),
        Some("codexmanager")
    );
    assert_eq!(data[1].get("id").and_then(Value::as_str), Some("gpt-4o"));
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");
    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].get("slug").and_then(Value::as_str),
        Some("gpt-5.3-codex")
    );
    assert_eq!(
        models[1].get("slug").and_then(Value::as_str),
        Some("gpt-4o")
    );
    assert_eq!(
        models[0].get("display_name").and_then(Value::as_str),
        Some("GPT-5.3 Codex")
    );
    assert_eq!(
        models[1].get("visibility").and_then(Value::as_str),
        Some("list")
    );
    assert_eq!(value.as_object().map(|object| object.len()), Some(3));
    assert!(value.get("etag").is_none());
}

#[test]
fn serialize_models_response_preserves_description_for_codex_clients() {
    let items = ModelsResponse {
        models: vec![ModelInfo {
            slug: "gpt-5.3-codex".to_string(),
            display_name: "GPT-5.3 Codex".to_string(),
            description: Some("Latest frontier agentic coding model.".to_string()),
            supported_in_api: true,
            visibility: Some("list".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .expect("OpenAI-compatible data array");
    assert_eq!(models.len(), 1);
    assert_eq!(
        models[0].get("description").and_then(Value::as_str),
        Some("Latest frontier agentic coding model.")
    );
    assert_eq!(
        data[0].get("description").and_then(Value::as_str),
        Some("Latest frontier agentic coding model.")
    );
}

#[test]
fn serialize_models_response_does_not_invent_codex_image_tool_model() {
    let items = ModelsResponse {
        models: vec![ModelInfo {
            slug: "gpt-5.4-mini".to_string(),
            display_name: "GPT-5.4 Mini".to_string(),
            supported_in_api: true,
            visibility: Some("list".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");

    assert_eq!(models.len(), 1);
    assert_eq!(
        models[0].get("slug").and_then(Value::as_str),
        Some("gpt-5.4-mini")
    );
}

#[test]
fn serialize_models_response_filters_api_data_to_supported_models() {
    let items = ModelsResponse {
        models: vec![
            ModelInfo {
                slug: "gpt-supported".to_string(),
                display_name: "GPT Supported".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
            ModelInfo {
                slug: "gpt-hidden".to_string(),
                display_name: "GPT Hidden".to_string(),
                supported_in_api: false,
                visibility: Some("hidden".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .expect("OpenAI-compatible data array");
    let ids = data
        .iter()
        .filter_map(|model| model.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert!(ids.contains(&"gpt-supported"));
    assert!(!ids.contains(&"gpt-hidden"));
}

#[test]
fn models_etag_header_uses_extra_etag_value() {
    let items = ModelsResponse {
        models: vec![],
        extra: std::collections::BTreeMap::from([(
            "etag".to_string(),
            serde_json::json!("\"remote-etag\""),
        )]),
    };

    let header = models_etag_header(&items)
        .expect("etag header should build")
        .expect("etag header should exist");

    assert!(header.field.equiv("etag"));
    assert_eq!(header.value.as_str(), "\"remote-etag\"");
}
