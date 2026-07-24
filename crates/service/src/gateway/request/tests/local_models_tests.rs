use super::*;
use codexmanager_core::rpc::types::{ModelInfo, ModelServiceTier, ModelsResponse};
use serde_json::Value;

#[test]
fn official_account_catalog_bypasses_manager_key_filters() {
    let storage = codexmanager_core::storage::Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let models = ModelsResponse {
        models: vec![ModelInfo {
            slug: "future-official-model".to_string(),
            display_name: "Future Official Model".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };

    let (filtered, include_implicit) = filter_models_for_catalog_policy(
        &storage,
        "missing-key-would-fail-managed-filtering",
        models,
        crate::codex_model_catalog::GatewayCatalogPolicy::OfficialAccountPool,
    )
    .expect("official catalog bypasses key filter");

    assert!(include_implicit);
    assert_eq!(filtered.models.len(), 1);
    assert_eq!(filtered.models[0].slug, "future-official-model");
}

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
fn serialize_models_response_preserves_description_for_codex_and_api_clients() {
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
fn serialize_models_response_preserves_service_tier_capabilities_for_codex_clients() {
    let items = ModelsResponse {
        models: vec![ModelInfo {
            slug: "gpt-5.5-codex".to_string(),
            display_name: "GPT-5.5 Codex".to_string(),
            supported_in_api: true,
            service_tiers: vec![ModelServiceTier {
                id: "flex".to_string(),
                name: "Flex".to_string(),
                description: "Lower priority capacity.".to_string(),
                ..Default::default()
            }],
            default_service_tier: Some("flex".to_string()),
            upgrade_info: Some(serde_json::json!({
                "model": "gpt-5.5-codex",
                "upgrade_copy": "Use the newer coding model"
            })),
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

    assert_eq!(
        models[0]["service_tiers"][0]
            .get("id")
            .and_then(Value::as_str),
        Some("flex")
    );
    assert_eq!(
        models[0]
            .get("default_service_tier")
            .and_then(Value::as_str),
        Some("flex")
    );
    assert_eq!(
        models[0]["upgrade_info"]
            .get("model")
            .and_then(Value::as_str),
        Some("gpt-5.5-codex")
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

#[test]
fn local_models_lists_image_model_once_with_image_only_capabilities() {
    let storage = codexmanager_core::storage::Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let response = crate::models_v2::models_response_with_storage(&storage)
        .expect("read managed local models");
    assert_eq!(response.models.len(), 8);
    let image_models = response
        .models
        .iter()
        .filter(|model| model.slug == "gpt-image-2")
        .collect::<Vec<_>>();
    assert_eq!(image_models.len(), 1);
    let image = image_models[0];
    assert_eq!(image.input_modalities, ["text", "image"]);
    assert_eq!(
        image.extra["output_modalities"],
        serde_json::json!(["image"])
    );
    assert_eq!(
        image.extra["supported_endpoints"],
        serde_json::json!(["/v1/images/generations", "/v1/images/edits"])
    );
    assert_eq!(image.extra["supports_text_generation"], false);

    let serialized: Value =
        serde_json::from_str(&serialize_models_response(&response)).expect("serialize models");
    let data = serialized["data"].as_array().expect("data array");
    assert_eq!(
        data.iter()
            .filter(|model| model["id"] == "gpt-image-2")
            .count(),
        1
    );
}
