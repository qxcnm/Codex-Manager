use super::{
    apply_codex_http_request_rules, normalize_official_responses_http_body,
    normalize_official_responses_http_body_with_value,
};
use serde_json::{json, Value};

#[test]
fn responses_http_normalizer_preserves_official_shape_and_unknown_fields() {
    let body = serde_json::to_vec(&json!({
        "model": "gpt-5.4",
        "instructions": "test",
        "input": [{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}],
        "tools": [],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "store": true,
        "stream": true,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "thread-1",
        "client_metadata": {"k":"v"},
        "custom_passthrough": true
    }))
    .expect("serialize body");

    let normalized = normalize_official_responses_http_body("/v1/responses", body);
    let value: serde_json::Value =
        serde_json::from_slice(&normalized).expect("parse normalized body");

    assert_eq!(value["model"], "gpt-5.4");
    assert_eq!(value["tool_choice"], "auto");
    assert_eq!(value["stream"], true);
    assert_eq!(value["custom_passthrough"], true);
}

#[test]
fn responses_http_normalizer_with_value_matches_byte_normalizer() {
    let value = json!({
        "model": "gpt-5.4",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "hello"}]}],
        "custom_passthrough": true
    });
    let body = serde_json::to_vec(&value).expect("serialize body");

    let expected = normalize_official_responses_http_body("/v1/responses", body.clone());
    let (actual, normalized_value) =
        normalize_official_responses_http_body_with_value("/v1/responses", body, value);

    assert_eq!(actual, expected);
    let normalized_value = normalized_value.expect("normalized value");
    assert_eq!(normalized_value["model"], "gpt-5.4");
    assert_eq!(normalized_value["custom_passthrough"], true);
    assert_eq!(normalized_value["stream"], false);
}

#[test]
fn codex_http_rules_promote_and_fill_standard_responses_defaults() {
    let mut obj = serde_json::json!({
        "model": "gpt-5.4",
        "input": [{
            "role": "developer",
            "content": [{"type":"input_text","text":"follow rules"}]
        }],
        "reasoning": {"effort":"high","summary":"auto","context":"current_turn"}
    })
    .as_object()
    .cloned()
    .expect("object");

    let result = apply_codex_http_request_rules(
        "/v1/responses",
        &mut obj,
        true,
        Some("thread-1"),
        false,
        Some("install-1"),
    );

    assert!(result.changed);
    assert_eq!(
        obj.get("instructions").and_then(Value::as_str),
        Some("follow rules")
    );
    assert_eq!(obj.get("stream").and_then(Value::as_bool), Some(true));
    assert_eq!(obj.get("store").and_then(Value::as_bool), Some(false));
    assert_eq!(obj.get("tool_choice").and_then(Value::as_str), Some("auto"));
    assert_eq!(
        obj.get("reasoning")
            .and_then(Value::as_object)
            .and_then(|reasoning| reasoning.get("context"))
            .and_then(Value::as_str),
        Some("current_turn")
    );
    assert_eq!(
        obj.get("reasoning")
            .and_then(Value::as_object)
            .and_then(|reasoning| reasoning.get("summary"))
            .and_then(Value::as_str),
        Some("auto")
    );
    assert_eq!(
        obj.get("prompt_cache_key").and_then(Value::as_str),
        Some("thread-1")
    );
    assert_eq!(
        obj.get("client_metadata")
            .and_then(Value::as_object)
            .and_then(|value| value.get("x-codex-installation-id"))
            .and_then(Value::as_str),
        Some("install-1")
    );
}

#[test]
fn codex_http_rules_repair_only_synthetic_reasoning_item_ids() {
    let mut obj = json!({
        "model": "gpt-5.4",
        "input": [
            {
                "type": "reasoning",
                "id": "item_a10c42b35178f112a4ee3a1f",
                "encrypted_content": "encrypted"
            },
            {
                "type": "reasoning",
                "id": "rs_already_valid",
                "encrypted_content": "encrypted"
            },
            {
                "type": "function_call",
                "id": "item_32f745f4a36ae17b8a063bf0",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{}"
            },
            {
                "type": "message",
                "id": "item_c044be127474392ddeeb230b",
                "role": "assistant",
                "content": []
            },
            {
                "type": "reasoning",
                "encrypted_content": "no id to synthesize"
            },
            {
                "type": "reasoning",
                "id": "unknown_prefix",
                "encrypted_content": "do not guess"
            },
            {
                "type": "custom_tool_call",
                "id": "item_9de417a1597e50f2bd14f73b",
                "call_id": "call_2",
                "name": "apply_patch",
                "input": "*** Begin Patch"
            }
        ]
    })
    .as_object()
    .cloned()
    .expect("object");

    let result =
        apply_codex_http_request_rules("/v1/responses", &mut obj, false, None, false, None);

    assert!(result.changed);
    let input = obj
        .get("input")
        .and_then(Value::as_array)
        .expect("input array");
    assert_eq!(input[0]["id"], "rs_a10c42b35178f112a4ee3a1f");
    assert_eq!(input[1]["id"], "rs_already_valid");
    assert_eq!(input[2]["id"], "fc_32f745f4a36ae17b8a063bf0");
    assert_eq!(input[3]["id"], "msg_c044be127474392ddeeb230b");
    assert!(input[4].get("id").is_none());
    assert_eq!(input[5]["id"], "unknown_prefix");
    assert_eq!(input[6]["id"], "ctc_9de417a1597e50f2bd14f73b");
}

#[test]
fn codex_http_rules_do_not_repair_reasoning_ids_outside_responses_paths() {
    let mut obj = json!({
        "input": [{"type": "reasoning", "id": "item_legacy"}]
    })
    .as_object()
    .cloned()
    .expect("object");

    let result =
        apply_codex_http_request_rules("/v1/chat/completions", &mut obj, false, None, false, None);

    assert!(!result.changed);
    assert_eq!(obj["input"][0]["id"], "item_legacy");
}
