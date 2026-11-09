use super::validate_original_constraints;
use bytes::Bytes;
use serde_json::{json, Value};

#[test]
fn bps_original_constraints_survive_consumed_reader_and_codex_field_removal() {
    let _guard = crate::test_env_guard();
    for stream in [false, true] {
        for (field, value) in [
            ("max_output_tokens", json!(17)),
            ("temperature", json!(0.25)),
            ("top_p", json!(0.8)),
            ("max_tool_calls", json!(1)),
            ("truncation", json!("disabled")),
            ("previous_response_id", json!("resp_previous")),
            ("conversation", json!("conv_previous")),
            ("background", json!(true)),
        ] {
            let mut source = json!({
                "model":"gpt-5.3-codex", "input":"hello", "stream":stream,
                "tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}]
            });
            source[field] = value;
            let bytes = Bytes::from(serde_json::to_vec(&source).unwrap());
            let (parts, ()) = axum::http::Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .body(())
                .unwrap()
                .into_parts();
            let (mut request, _receiver) =
                crate::http::gateway_request::GatewayRequest::new(parts, bytes.clone());
            let mut consumed = Vec::new();
            request.as_reader().read_to_end(&mut consumed).unwrap();
            assert_eq!(request.original_body(), bytes.as_ref());
            let normalized = crate::gateway::request_rewrite::apply_request_overrides(
                "/v1/responses",
                consumed,
                None,
                None,
                Some("https://chatgpt.com/backend-api/codex"),
            );
            let normalized_value: Value = serde_json::from_slice(&normalized).unwrap();
            assert!(
                normalized_value.get(field).is_none(),
                "fixture must exercise stripped {field}"
            );
            // Existing Codex rewriting still accepts and removes these fields;
            // validating only the normalized request cannot detect the loss.
            assert!(super::super::prepare(&normalized, "test").is_ok());
            assert!(validate_original_constraints(request.url(), request.original_body()).is_err());
        }
    }
}

#[test]
fn bps_original_chat_constraints_cover_aliases_and_stream_modes() {
    for stream in [false, true] {
        for (field, value) in [
            ("max_tokens", json!(10)),
            ("max_completion_tokens", json!(10)),
            ("temperature", json!(0.4)),
            ("top_p", json!(0.7)),
            ("stop", json!(["END"])),
            ("seed", json!(1)),
            ("frequency_penalty", json!(1)),
            ("presence_penalty", json!(1)),
            ("logit_bias", json!({"42":10})),
            ("logprobs", json!(true)),
            ("top_logprobs", json!(2)),
            ("n", json!(2)),
            ("response_format", json!({"type":"json_object"})),
        ] {
            let mut source = json!({
                "model":"m", "stream":stream,
                "messages":[{"role":"user","content":"hello"}],
                "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]
            });
            source[field] = value;
            let original = serde_json::to_vec(&source).unwrap();
            assert!(
                validate_original_constraints("/v1/chat/completions/?test=1", &original).is_err(),
                "must reject original {field}"
            );
        }
    }
}

#[test]
fn bps_original_constraints_allow_nulls_and_supported_explicit_defaults() {
    let body = serde_json::to_vec(&json!({
        "model":"m", "input":"hi", "temperature":null, "top_p":null,
        "max_output_tokens":null, "max_tokens":null, "max_completion_tokens":null,
        "background":false, "service_tier":"auto", "reasoning":{"mode":"standard"},
        "text":{"format":{"type":"text"}}, "response_format":{"type":"text"},
        "previous_response_id":null, "conversation":null, "n":1
    }))
    .unwrap();
    assert!(validate_original_constraints("/v1/responses", &body).is_ok());
    assert!(validate_original_constraints("/v1/chat/completions", &body).is_ok());
}

#[test]
fn bps_original_constraints_share_responses_checks() {
    for (field, value) in [
        ("reasoning", json!({"mode":"research"})),
        ("service_tier", json!("priority")),
        ("text", json!({"format":{"type":"json_schema"}})),
    ] {
        let mut body = json!({"model":"m","input":"hi"});
        body[field] = value;
        let encoded = serde_json::to_vec(&body).unwrap();
        let original = validate_original_constraints("/v1/responses", &encoded).unwrap_err();
        let prepared = super::super::prepare(&encoded, "test").err().unwrap();
        assert_eq!(original, prepared);
    }
}

#[test]
fn bps_reasoning_tool_requests_drop_codex_include_after_real_normalization() {
    let _guard = crate::test_env_guard();
    for stream in [false, true] {
        for tool in [
            json!({
                "type":"custom", "name":"echo_text", "description":"Echo exact raw text",
                "format":{"type":"text"}
            }),
            json!({
                "type":"function", "name":"echo_text", "description":"Echo the given text",
                "strict":true,
                "parameters":{
                    "type":"object", "properties":{"text":{"type":"string"}},
                    "required":["text"], "additionalProperties":false
                }
            }),
        ] {
            let source = json!({
                "model":"gpt-6-luna", "input":"Call echo_text exactly once with BPS_TOOL_OK.",
                "stream":stream, "store":false, "reasoning":{"effort":"low"},
                "service_tier":"default", "parallel_tool_calls":false,
                "tool_choice":"required", "tools":[tool.clone()]
            });
            assert!(source.get("include").is_none());
            let original = serde_json::to_vec(&source).unwrap();
            validate_original_constraints("/v1/responses", &original).unwrap();
            // Routing enables compatibility defaults after selecting a Codex
            // candidate; the generic override entry point can defer them.
            let normalized = crate::gateway::request_rewrite::apply_codex_candidate_transport_rules(
                "/v1/responses",
                original,
            );
            let normalized_value: Value = serde_json::from_slice(&normalized).unwrap();
            assert_eq!(
                normalized_value["include"],
                json!(["reasoning.encrypted_content"]),
                "fixture must reproduce the selector injected by Codex normalization"
            );
            let (wire, _) = super::super::prepare(&normalized, "regression-session").unwrap();
            let wire: Value = serde_json::from_slice(&wire).unwrap();
            assert!(wire.get("include").is_none());
            assert_eq!(wire["model"], "gpt-6-luna");
            assert_eq!(wire["reasoning_effort"], "low");
            assert_eq!(wire["stream"], true);
            let prompt = wire["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["role"] == "developer")
                .filter_map(|item| item.pointer("/content/0/text").and_then(Value::as_str))
                .find(|text| text.contains("Client tool catalog:\n"))
                .expect("client tools must survive normalization into the BPS prompt");
            let catalog_line = prompt
                .split_once("Client tool catalog:\n")
                .unwrap()
                .1
                .lines()
                .next()
                .unwrap();
            let catalog: Value = serde_json::from_str(catalog_line).unwrap();
            assert_eq!(catalog, json!([tool]));
            assert!(prompt.contains("This response must request a declared client tool."));
        }
    }
}
