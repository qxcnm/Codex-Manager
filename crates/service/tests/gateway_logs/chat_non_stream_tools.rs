use super::*;
use codexmanager_core::storage::AggregateApi;
use serde_json::{json, Value};

pub(super) const MODEL: &str = "gpt-non-stream-tools-test";
pub(super) const UPSTREAM_MODEL: &str = "gpt-non-stream-tools-upstream";

fn seed_aggregate_route(storage: &Storage, upstream_addr: &str, action: &str) {
    let now = now_ts();
    let aggregate_id = "agg_non_stream_tools";
    storage
        .insert_aggregate_api(&AggregateApi {
            id: aggregate_id.to_string(),
            provider_type: "codex".to_string(),
            supplier_name: Some("non-stream tools mock".to_string()),
            sort: 0,
            url: format!("http://{upstream_addr}/backend-api/codex"),
            auth_type: "apikey".to_string(),
            auth_params_json: None,
            action: Some(action.to_string()),
            model_override: None,
            user_agent: None,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
            balance_query_enabled: false,
            balance_query_template: None,
            balance_query_base_url: None,
            balance_query_user_id: None,
            balance_query_config_json: None,
            last_balance_at: None,
            last_balance_status: None,
            last_balance_error: None,
            last_balance_json: None,
        })
        .expect("insert mock aggregate API");
    storage
        .upsert_aggregate_api_secret(aggregate_id, "mock-aggregate-secret")
        .expect("insert mock aggregate API secret");
    seed_model_catalog_models(storage, &[MODEL]);
    let mut model = storage
        .get_managed_model_v2(MODEL)
        .expect("read test model")
        .expect("test model exists");
    model.routes = vec![ModelRouteV2 {
        id: String::new(),
        source_kind: "aggregate_api".to_string(),
        source_id: aggregate_id.to_string(),
        upstream_model: UPSTREAM_MODEL.to_string(),
        enabled: true,
        priority: 0,
        weight: 1,
    }];
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .expect("use only mock aggregate route");
    seed_platform_key(storage, "hybrid_aggregate_first_rotation");
}

pub(super) fn seed_account_route(storage: &Storage) {
    let now = now_ts();
    let account_id = "acc_non_stream_tools_mock";
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: "non-stream tools temporary mock".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_non_stream_tools_mock".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert temporary mock account");
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: String::new(),
            access_token: "mock-access-non-stream-tools".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("mock-api-access-non-stream-tools".to_string()),
            last_refresh: now,
        })
        .expect("insert temporary mock token");
    seed_model_catalog_models(storage, &[MODEL]);
    let mut model = storage
        .get_managed_model_v2(MODEL)
        .expect("read account-route test model")
        .expect("account-route test model exists");
    model.routes = vec![ModelRouteV2 {
        id: String::new(),
        source_kind: "account_pool".to_string(),
        source_id: "default".to_string(),
        upstream_model: UPSTREAM_MODEL.to_string(),
        enabled: true,
        priority: 0,
        weight: 1,
    }];
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .expect("use only temporary mock account route");
    seed_platform_key(storage, "account_rotation");
}

fn seed_platform_key(storage: &Storage, rotation_strategy: &str) {
    let now = now_ts();
    storage
        .insert_api_key(&ApiKey {
            id: "gk_non_stream_tools".to_string(),
            name: Some("non-stream tools mock".to_string()),
            model_slug: Some(MODEL.to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: rotation_strategy.to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test("pk_non_stream_tools"),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert mock platform key");
}

fn responses(output: Value) -> Value {
    json!({
        "id": "resp_non_stream_tools",
        "object": "response",
        "created_at": 1720000000,
        "status": "completed",
        "model": UPSTREAM_MODEL,
        "output": output,
        "usage": {
            "input_tokens": 7,
            "input_tokens_details": {"cached_tokens": 2},
            "output_tokens": 5,
            "output_tokens_details": {"reasoning_tokens": 1},
            "total_tokens": 12
        }
    })
}

fn expected_usage() -> Value {
    json!({
        "prompt_tokens": 7,
        "prompt_tokens_details": {"cached_tokens": 2},
        "completion_tokens": 5,
        "completion_tokens_details": {"reasoning_tokens": 1},
        "total_tokens": 12
    })
}

fn expected_tool(id: &str, name: &str, arguments: &str) -> Value {
    json!({"id": id, "type": "function", "function": {"name": name, "arguments": arguments}})
}

fn message(text: &str) -> Value {
    json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]})
}

fn reasoning(text: &str) -> Value {
    json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": text}]})
}

fn completed_sse(upstream: &Value) -> String {
    format!(
        "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"type": "response.completed", "response": upstream})
    )
}

fn assert_chat(actual: &Value, expected: &Value) {
    assert_eq!(actual["object"], "chat.completion");
    assert_eq!(actual["id"], "resp_non_stream_tools");
    assert_eq!(actual["model"], UPSTREAM_MODEL);
    assert_eq!(actual["choices"].as_array().expect("choices").len(), 1);
    assert_eq!(actual["choices"][0]["index"], 0);
    assert_eq!(actual["choices"][0]["message"], expected["message"]);
    assert_eq!(
        actual["choices"][0]["finish_reason"],
        expected["finish_reason"]
    );
    assert_eq!(actual["usage"], expected["usage"]);
}

/// Exercise the public HTTP route with an isolated migrated DB and a loopback-only
/// account or aggregate route. The optional fixture is written before assertions so a failing
/// baseline is available to an official SDK without weakening the regression.
fn request_through_gateway(
    case: &str,
    upstream: &Value,
    wire_body: &str,
    content_type: &str,
    action: &str,
    path: &str,
    native_client: bool,
    expected: Value,
) -> Value {
    let dir = new_test_dir(&format!("codexmanager-{case}"));
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open isolated db");
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_once_with_content_type(wire_body, content_type);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);
    let converted_chat = path == "/v1/chat/completions" && !native_client;
    if converted_chat {
        // Aggregate-only models intentionally take the passthrough branch before
        // Chat compatibility adaptation. The account route exercises the real
        // Responses-to-Chat conversion rather than only its JSON helper.
        seed_account_route(&storage);
    } else {
        seed_aggregate_route(&storage, &upstream_addr, action);
    }
    let server = codexmanager_service::start_one_shot_server().expect("start gateway");
    let request = if path == "/v1/responses" {
        json!({"model": MODEL, "input": "test tools", "stream": false})
    } else {
        json!({
            "model": MODEL,
            "messages": [{"role": "user", "content": "test tools"}],
            "stream": false,
            "tools": [{"type": "function", "function": {"name": "lookup", "parameters": {"type": "object"}}}]
        })
    };
    let mut headers = vec![
        ("Content-Type", "application/json"),
        ("Authorization", "Bearer pk_non_stream_tools"),
    ];
    if native_client {
        headers.push(("User-Agent", "codex_cli_rs/r04-test"));
    }
    let (status, raw) = post_http_raw(
        &server.addr,
        path,
        &serde_json::to_string(&request).expect("serialize client request"),
        &headers,
    );
    server.join();
    upstream_join.join().expect("join mock upstream");
    let actual: Value = serde_json::from_str(&raw).expect("gateway returns JSON");
    let captured = upstream_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(
        captured.len(),
        1,
        "{case}: exactly one mock upstream request"
    );
    let upstream_request: Value =
        serde_json::from_slice(&decode_upstream_request_body(&captured[0]))
            .expect("parse actual upstream request");
    if let Some(fixture_dir) = std::env::var_os("CM_REPAIR_FIXTURE_DIR") {
        let fixture_dir = PathBuf::from(fixture_dir);
        fs::create_dir_all(&fixture_dir).expect("create fixture directory");
        let fixture = json!({
            "case": case,
            "path": path,
            "upstream": upstream,
            "upstream_wire": wire_body,
            "upstream_content_type": content_type,
            "upstream_request": upstream_request,
            "native_client": native_client,
            "route_kind": if converted_chat { "account_pool" } else { "aggregate_api" },
            "request": request,
            "status": status,
            "actual": actual,
            "expected": expected
        });
        fs::write(
            fixture_dir.join(format!("{case}.json")),
            serde_json::to_vec_pretty(&fixture).expect("serialize fixture"),
        )
        .expect("write real HTTP fixture");
    }
    assert_eq!(status, 200, "{case}: gateway response: {raw}");
    assert_eq!(captured[0].path, format!("/backend-api/codex{action}"));
    assert_eq!(upstream_request["model"], UPSTREAM_MODEL);
    if converted_chat {
        assert!(
            upstream_request["input"].is_array(),
            "{case}: production Chat compatibility route must send Responses input"
        );
        assert!(
            upstream_request.get("messages").is_none(),
            "{case}: production Chat compatibility route must not pass through Chat messages"
        );
    }
    if native_client && action == "/chat/completions" {
        assert!(upstream_request["messages"].is_array());
        assert!(upstream_request.get("input").is_none());
        assert_eq!(upstream_request["stream"], false);
    }
    actual
}

#[test]
fn non_stream_chat_preserves_function_calls_from_responses() {
    let _lock = test_env_guard();
    let upstream = responses(json!([{
        "type": "function_call",
        "id": "fc_item_1",
        "call_id": "call_lookup_1",
        "name": "lookup",
        "arguments": "{\"city\":\"Shanghai\"}"
    }]));
    let expected = json!({
        "message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [expected_tool("call_lookup_1", "lookup", "{\"city\":\"Shanghai\"}")]
        },
        "finish_reason": "tool_calls",
        "usage": expected_usage()
    });
    let actual = request_through_gateway(
        "non_stream_chat_preserves_function_calls_from_responses",
        &upstream,
        &upstream.to_string(),
        "application/json",
        "/responses",
        "/v1/chat/completions",
        false,
        expected.clone(),
    );
    assert_chat(&actual, &expected);
}

#[test]
fn non_stream_chat_preserves_mixed_output_order_ids_and_argument_types() {
    let _lock = test_env_guard();
    // Preserve the original string verbatim; object parameters must become JSON,
    // rather than an empty string, debug output, or a quoted JSON string.
    let exact_arguments = " { \"city\" : \"上海\", \"units\" : \"c\" } ";
    let object_arguments = json!({"nested": {"enabled": true}, "values": [1, 2]});
    let upstream = responses(json!([
        reasoning("Think before. "),
        message("Before. "),
        {"type": "function_call", "id": "fc_item_a", "call_id": "call_preferred_a", "name": "lookup", "arguments": exact_arguments},
        message("Between. "),
        {"type": "function_call", "id": "fc_fallback_b", "name": "compute", "arguments": object_arguments},
        {"type": "function_call", "id": "fc_fallback_c", "call_id": "", "name": "finish", "arguments": "{}"},
        reasoning("Think after."),
        message("After.")
    ]));
    let expected = json!({
        "message": {
            "role": "assistant",
            "content": "Before. Between. After.",
            "reasoning": "Think before. Think after.",
            "reasoning_content": "Think before. Think after.",
            "tool_calls": [
                expected_tool("call_preferred_a", "lookup", exact_arguments),
                expected_tool("fc_fallback_b", "compute", &object_arguments.to_string()),
                expected_tool("fc_fallback_c", "finish", "{}")
            ]
        },
        "finish_reason": "tool_calls",
        "usage": expected_usage()
    });
    for (suffix, content_type, wire_body) in [
        ("json", "application/json", upstream.to_string()),
        ("sse", "text/event-stream", completed_sse(&upstream)),
    ] {
        let actual = request_through_gateway(
            &format!("non_stream_chat_mixed_output_{suffix}"),
            &upstream,
            &wire_body,
            content_type,
            "/responses",
            "/v1/chat/completions",
            false,
            expected.clone(),
        );
        assert_chat(&actual, &expected);
        assert_eq!(actual["created"], 1720000000);
        let object_string = actual["choices"][0]["message"]["tool_calls"][1]["function"]
            ["arguments"]
            .as_str()
            .expect("object parameters serialized to a string");
        assert_eq!(
            serde_json::from_str::<Value>(object_string).expect("arguments are valid JSON"),
            object_arguments
        );
    }
}

#[test]
fn non_stream_chat_assembles_sse_tool_deltas_before_final_response() {
    let _lock = test_env_guard();
    let upstream = responses(json!([
        reasoning("Before. "),
        {"type": "function_call", "id": "fc_delta_a", "call_id": "call_delta_a", "name": "lookup", "arguments": "{\"n\":1}"},
        message("Checked. "),
        {"type": "function_call", "id": "fc_delta_b", "call_id": "call_delta_b", "name": "compute", "arguments": "{\"n\":2}"},
        reasoning("After.")
    ]));
    let mut events = Vec::new();
    for (index, item) in upstream["output"].as_array().unwrap().iter().enumerate() {
        let mut added = item.clone();
        if added["type"] == "function_call" {
            added["arguments"] = json!("");
        }
        events.push(
            json!({"type": "response.output_item.added", "output_index": index, "item": added}),
        );
        if item["type"] == "function_call" {
            events.push(json!({
                "type": "response.function_call_arguments.delta",
                "output_index": index,
                "item_id": item["id"],
                "delta": "{\"n\":"
            }));
            events.push(json!({
                "type": "response.function_call_arguments.delta",
                "output_index": index,
                "item_id": item["id"],
                "delta": if index == 1 { "1}" } else { "2}" }
            }));
            events.push(json!({
                "type": "response.function_call_arguments.done",
                "output_index": index,
                "item_id": item["id"],
                "arguments": item["arguments"]
            }));
        }
    }
    let mut final_response = upstream.clone();
    final_response["output"] = json!([]);
    events.push(json!({"type": "response.completed", "response": final_response}));
    let mut wire_body = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    wire_body.push_str("data: [DONE]\n\n");
    let expected = json!({
        "message": {
            "role": "assistant",
            "content": "Checked. ",
            "reasoning": "Before. After.",
            "reasoning_content": "Before. After.",
            "tool_calls": [
                expected_tool("call_delta_a", "lookup", "{\"n\":1}"),
                expected_tool("call_delta_b", "compute", "{\"n\":2}")
            ]
        },
        "finish_reason": "tool_calls",
        "usage": expected_usage()
    });
    let actual = request_through_gateway(
        "non_stream_chat_assembles_sse_tool_deltas_before_final_response",
        &upstream,
        &wire_body,
        "text/event-stream",
        "/responses",
        "/v1/chat/completions",
        false,
        expected.clone(),
    );
    assert_chat(&actual, &expected);
}

#[test]
fn non_stream_chat_pure_text_still_finishes_with_stop() {
    let _lock = test_env_guard();
    let upstream = responses(json!([message("Plain answer")]));
    let expected = json!({
        "message": {"role": "assistant", "content": "Plain answer"},
        "finish_reason": "stop",
        "usage": expected_usage()
    });
    for (suffix, content_type, wire_body) in [
        ("json", "application/json", upstream.to_string()),
        ("sse", "text/event-stream", completed_sse(&upstream)),
    ] {
        let actual = request_through_gateway(
            &format!("non_stream_chat_pure_text_{suffix}"),
            &upstream,
            &wire_body,
            content_type,
            "/responses",
            "/v1/chat/completions",
            false,
            expected.clone(),
        );
        assert_chat(&actual, &expected);
        assert!(actual["choices"][0]["message"].get("tool_calls").is_none());
    }
}

#[test]
fn non_stream_chat_does_not_map_custom_tool_calls_to_functions() {
    let _lock = test_env_guard();
    // Chat's existing contract accepts Responses function_call. Custom tools use
    // a different input schema and must not silently become function calls.
    let custom = json!({"type": "custom_tool_call", "id": "ct_item", "call_id": "custom_call", "name": "shell", "input": "ls -la"});
    for (suffix, output, tool_calls) in [
        (
            "only",
            json!([custom.clone(), message("Custom boundary")]),
            json!([]),
        ),
        (
            "mixed",
            json!([custom, {"type": "function_call", "id": "fc_supported", "call_id": "call_supported", "name": "lookup", "arguments": "{}"}, message("Custom boundary")]),
            json!([expected_tool("call_supported", "lookup", "{}")]),
        ),
    ] {
        let upstream = responses(output);
        let mut expected = json!({
            "message": {"role": "assistant", "content": "Custom boundary"},
            "finish_reason": "stop",
            "usage": expected_usage()
        });
        if !tool_calls.as_array().unwrap().is_empty() {
            expected["message"]["tool_calls"] = tool_calls;
            expected["finish_reason"] = json!("tool_calls");
        }
        for (wire_suffix, content_type, wire_body) in [
            ("json", "application/json", upstream.to_string()),
            ("sse", "text/event-stream", completed_sse(&upstream)),
        ] {
            let actual = request_through_gateway(
                &format!("non_stream_chat_custom_tools_{suffix}_{wire_suffix}"),
                &upstream,
                &wire_body,
                content_type,
                "/responses",
                "/v1/chat/completions",
                false,
                expected.clone(),
            );
            assert_chat(&actual, &expected);
            assert!(!actual.to_string().contains("custom_call"));
            assert!(!actual.to_string().contains("ls -la"));
        }
    }
}

#[test]
fn non_stream_native_responses_preserves_function_and_custom_tools() {
    let _lock = test_env_guard();
    let upstream = responses(json!([
        reasoning("Keep native output"),
        {"type": "function_call", "id": "fc_native", "call_id": "call_native", "name": "lookup", "arguments": "{\"city\":\"Shanghai\"}"},
        {"type": "custom_tool_call", "id": "ct_native", "call_id": "custom_native", "name": "shell", "input": "pwd"},
        message("Native text")
    ]));
    let expected = json!({"output": upstream["output"], "usage": upstream["usage"]});
    for (suffix, content_type, wire_body) in [
        ("json", "application/json", upstream.to_string()),
        ("sse", "text/event-stream", completed_sse(&upstream)),
    ] {
        let actual = request_through_gateway(
            &format!("non_stream_native_responses_{suffix}"),
            &upstream,
            &wire_body,
            content_type,
            "/responses",
            "/v1/responses",
            false,
            expected.clone(),
        );
        assert_eq!(actual["id"], upstream["id"]);
        assert_eq!(actual["status"], "completed");
        assert_eq!(actual["model"], UPSTREAM_MODEL);
        assert_eq!(actual["output"], expected["output"]);
        assert_eq!(actual["usage"], expected["usage"]);
        assert!(actual.get("choices").is_none());
    }
}

#[test]
fn non_stream_native_chat_json_preserves_tools_without_responses_mapping() {
    let _lock = test_env_guard();
    let upstream = json!({
        "id": "chatcmpl_native_tools",
        "object": "chat.completion",
        "created": 1720000000,
        "model": UPSTREAM_MODEL,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Native chat",
                "tool_calls": [
                    expected_tool("call_chat_a", "lookup", " { \"n\" : 1 } "),
                    expected_tool("call_chat_b", "compute", "{\"n\":2}")
                ]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": expected_usage()
    });
    let expected = json!({
        "message": upstream["choices"][0]["message"],
        "finish_reason": upstream["choices"][0]["finish_reason"],
        "usage": upstream["usage"]
    });
    let actual = request_through_gateway(
        "non_stream_native_chat_json_preserves_tools_without_responses_mapping",
        &upstream,
        &upstream.to_string(),
        "application/json",
        "/chat/completions",
        "/v1/chat/completions",
        true,
        expected,
    );
    assert_eq!(actual, upstream);
}

#[test]
fn non_stream_native_chat_sse_preserves_interleaved_tool_deltas() {
    let _lock = test_env_guard();
    for (suffix, text) in [
        ("pure_tools", None),
        ("mixed", Some("Native streamed chat")),
    ] {
        let expected = json!({
            "message": {
                "role": "assistant",
                "content": text,
                "tool_calls": [
                    expected_tool("call_chat_delta_a", "lookup", " { \"n\" : 1 } "),
                    expected_tool("call_chat_delta_b", "compute", "{\"parts\":[\"ha\",\"ha\",\"ha\"]}")
                ]
            },
            "finish_reason": "tool_calls",
            "usage": expected_usage()
        });
        let upstream = json!({
            "id": "chatcmpl_native_sse_tools",
            "object": "chat.completion",
            "created": 1720000000,
            "model": UPSTREAM_MODEL,
            "choices": [{"index": 0, "message": expected["message"], "finish_reason": "tool_calls"}],
            "usage": expected_usage()
        });
        // Tool 1 is first observed before tool 0; output order must follow the
        // native Chat indices. Name and argument fragments are append-only.
        let chunks = [
            json!({"index": 0, "delta": {"role": "assistant", "content": text, "tool_calls": [
                {"index": 1, "id": "call_chat_delta_b", "type": "function", "function": {"name": "com", "arguments": "{\"parts\":["}},
                {"index": 0, "id": "call_chat_delta_a", "type": "function", "function": {"name": "look", "arguments": " { "}}
            ]}, "finish_reason": null}),
            json!({"index": 0, "delta": {"tool_calls": [
                {"index": 0, "function": {"name": "up", "arguments": "\"n\" : 1 } "}},
                {"index": 1, "function": {"name": "pute", "arguments": "\"ha\","}}
            ]}, "finish_reason": null}),
            json!({"index": 0, "delta": {"tool_calls": [
                {"index": 1, "function": {"arguments": "\"ha\","}}
            ]}, "finish_reason": null}),
            json!({"index": 0, "delta": {"tool_calls": [
                {"index": 1, "function": {"arguments": "\"ha\"]}"}}
            ]}, "finish_reason": null}),
            json!({"index": 0, "delta": {}, "finish_reason": "tool_calls"}),
        ];
        let mut wire_body = chunks
            .iter()
            .map(|choice| {
                format!(
                    "data: {}\n\n",
                    json!({"id": "chatcmpl_native_sse_tools", "object": "chat.completion.chunk", "created": 1720000000, "model": UPSTREAM_MODEL, "choices": [choice]})
                )
            })
            .collect::<String>();
        wire_body.push_str(&format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"id": "chatcmpl_native_sse_tools", "object": "chat.completion.chunk", "created": 1720000000, "model": UPSTREAM_MODEL, "choices": [], "usage": expected_usage()})
        ));
        let actual = request_through_gateway(
            &format!("non_stream_native_chat_sse_interleaved_{suffix}"),
            &upstream,
            &wire_body,
            "text/event-stream",
            "/chat/completions",
            "/v1/chat/completions",
            true,
            expected.clone(),
        );
        assert_eq!(actual["id"], upstream["id"]);
        assert_eq!(actual["choices"][0]["message"], expected["message"]);
        assert_eq!(actual["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(actual["usage"], expected["usage"]);
    }
}

#[test]
fn non_stream_chat_preserves_repeated_argument_deltas_without_done_snapshot() {
    let _lock = test_env_guard();
    let mut results = Vec::new();
    for (suffix, include_done) in [("without_done", false), ("with_done", true)] {
        let upstream = responses(json!([
            {"type": "function_call", "id": "fc_repeat_a", "call_id": "call_repeat_a", "name": "lookup", "arguments": "{\"x\":\"aaaa\"}"},
            {"type": "function_call", "id": "fc_repeat_b", "call_id": "call_repeat_b", "name": "compute", "arguments": "{\"n\":2}"}
        ]));
        let mut final_response = upstream.clone();
        final_response["output"] = json!([]);
        let mut events = vec![
            json!({"type": "response.output_item.added", "output_index": 1, "item": {"type": "function_call", "id": "fc_repeat_a", "call_id": "call_repeat_a", "name": "lookup", "arguments": ""}}),
            json!({"type": "response.output_item.added", "output_index": 3, "item": {"type": "function_call", "id": "fc_repeat_b", "call_id": "call_repeat_b", "name": "compute", "arguments": ""}}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "item_id": "fc_repeat_a", "delta": "{\"x\":\""}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 3, "item_id": "fc_repeat_b", "delta": "{\"n\":"}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "item_id": "fc_repeat_a", "delta": "aa"}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 3, "item_id": "fc_repeat_b", "delta": "2}"}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "item_id": "fc_repeat_a", "delta": "aa"}),
            json!({"type": "response.function_call_arguments.done", "output_index": 3, "item_id": "fc_repeat_b", "arguments": "{\"n\":2}"}),
            json!({"type": "response.function_call_arguments.done", "output_index": 3, "item_id": "fc_repeat_b", "arguments": "{\"n\":2}"}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "item_id": "fc_repeat_a", "delta": "\"}"}),
        ];
        if include_done {
            events.push(json!({"type": "response.function_call_arguments.done", "output_index": 1, "item_id": "fc_repeat_a", "arguments": "{\"x\":\"aaaa\"}"}));
        }
        events.push(json!({"type": "response.completed", "response": final_response}));
        let mut wire_body = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        wire_body.push_str("data: [DONE]\n\n");
        // Without a done snapshot, no final output item can repair a dropped repeated
        // delta. With a done snapshot, its authoritative arguments must replace the
        // accumulated value. Replayed done snapshots for the second tool must not
        // append duplicate JSON or affect the first tool.
        let expected = json!({
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    expected_tool("call_repeat_a", "lookup", "{\"x\":\"aaaa\"}"),
                    expected_tool("call_repeat_b", "compute", "{\"n\":2}")
                ]
            },
            "finish_reason": "tool_calls",
            "usage": expected_usage()
        });
        let actual = request_through_gateway(
            &format!("non_stream_chat_repeated_deltas_{suffix}"),
            &upstream,
            &wire_body,
            "text/event-stream",
            "/responses",
            "/v1/chat/completions",
            false,
            expected.clone(),
        );
        results.push((actual, expected));
    }
    // Collect both actual HTTP fixtures before checking semantic equality, so a
    // failing baseline preserves evidence for both delta-only and done paths.
    for (actual, expected) in results {
        assert_chat(&actual, &expected);
        let arguments = actual["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("first tool arguments string");
        assert_eq!(
            serde_json::from_str::<Value>(arguments).expect("first tool arguments JSON"),
            json!({"x": "aaaa"})
        );
    }
}

// These public HTTP stream cases deliberately use a separate capture namespace:
// the original 17 non-stream fixture names and their SDK contract stay intact.
fn stream_http_tools_fixture() -> (Value, Vec<Value>, String) {
    let final_response = responses(json!([
        reasoning("Think. "),
        message("Before. "),
        {"type": "function_call", "id": "fc_stream_a", "call_id": "call_stream_a", "name": "lookup", "arguments": "{\"city\":\"上海\",\"units\":\"c\"}"},
        message("Between. "),
        {"type": "function_call", "id": "fc_stream_b", "call_id": "call_stream_b", "name": "compute", "arguments": "{\"n\":2}"},
        message("After."),
        {"type": "function_call", "id": "fc_stream_c", "call_id": "call_stream_c", "name": "finish", "arguments": "{\"ok\":true}"}
    ]));
    let events = vec![
        json!({"type": "response.created", "response": {
            "id": "resp_non_stream_tools", "model": UPSTREAM_MODEL,
            "created_at": 1720000000, "status": "in_progress", "output": []
        }}),
        json!({"type": "response.output_item.added", "output_index": 0, "item": {"type": "reasoning", "summary": []}}),
        json!({"type": "response.reasoning_summary_text.delta", "output_index": 0, "delta": "Think. "}),
        json!({"type": "response.output_item.added", "output_index": 1, "item": {"type": "message", "role": "assistant", "content": []}}),
        json!({"type": "response.output_text.delta", "output_index": 1, "delta": "Before. "}),
        // Arguments precede ID/name; sparse Responses positions must become
        // dense Chat tool slots even though messages/reasoning occupy positions.
        json!({"type": "response.output_item.added", "output_index": 2, "item": {"type": "function_call", "id": "fc_stream_a", "call_id": "", "name": "", "arguments": ""}}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 2, "item_id": "fc_stream_a", "delta": "{\"city\":\"上"}),
        json!({"type": "response.output_item.added", "output_index": 4, "item": {"type": "function_call", "id": "fc_stream_b", "call_id": "call_stream_b", "name": "compute", "arguments": ""}}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 4, "item_id": "fc_stream_b", "delta": "{\"n\":"}),
        json!({"type": "response.output_text.delta", "output_index": 3, "delta": "Between. "}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 2, "item_id": "fc_stream_a", "delta": "海\",\"units\":\"c\"}"}),
        json!({"type": "response.function_call_arguments.done", "output_index": 4, "item_id": "fc_stream_b", "arguments": "{\"n\":2}"}),
        // The first tool's late metadata comes from done; the third exists only
        // in completed. Replaying both snapshots must not duplicate arguments.
        json!({"type": "response.output_item.done", "output_index": 2, "item": final_response["output"][2]}),
        json!({"type": "response.output_text.delta", "output_index": 5, "delta": "After."}),
        json!({"type": "response.completed", "response": final_response}),
    ];
    let mut wire = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<String>();
    wire.push_str("data: [DONE]\n\n");
    (final_response, events, wire)
}

fn stream_http_expected_tools() -> Value {
    json!([
        {"id": "call_stream_a", "type": "function", "function": {"name": "lookup", "arguments": "{\"city\":\"上海\",\"units\":\"c\"}"}},
        {"id": "call_stream_b", "type": "function", "function": {"name": "compute", "arguments": "{\"n\":2}"}},
        {"id": "call_stream_c", "type": "function", "function": {"name": "finish", "arguments": "{\"ok\":true}"}}
    ])
}

fn request_stream_tools_through_gateway(
    case: &str,
    converted_chat: bool,
    upstream_sse: &str,
    expected: Value,
) -> String {
    let dir = new_test_dir(&format!("codexmanager-{case}"));
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open isolated stream HTTP db");
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_once_with_content_type(upstream_sse, "text/event-stream");
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);
    let path = if converted_chat {
        seed_account_route(&storage);
        "/v1/chat/completions"
    } else {
        seed_aggregate_route(&storage, &upstream_addr, "/responses");
        "/v1/responses"
    };
    let input = json!([{
        "type": "message", "role": "user",
        "content": [{"type": "input_text", "text": "test stream tools"}]
    }]);
    let tools = json!([
        {"type": "function", "name": "lookup", "parameters": {"type": "object"}},
        {"type": "function", "name": "compute", "parameters": {"type": "object"}},
        {"type": "function", "name": "finish", "parameters": {"type": "object"}}
    ]);
    let request = if converted_chat {
        json!({
            "model": MODEL, "messages": [{"role": "user", "content": "test stream tools"}],
            "stream": true,
            "tools": [
                {"type": "function", "function": {"name": "lookup", "parameters": {"type": "object"}}},
                {"type": "function", "function": {"name": "compute", "parameters": {"type": "object"}}},
                {"type": "function", "function": {"name": "finish", "parameters": {"type": "object"}}}
            ]
        })
    } else {
        json!({"model": MODEL, "input": input, "tools": tools, "stream": true})
    };
    let server = codexmanager_service::start_one_shot_server().expect("start stream HTTP gateway");
    let mut headers = vec![
        ("Content-Type", "application/json"),
        ("Authorization", "Bearer pk_non_stream_tools"),
    ];
    if !converted_chat {
        headers.push(("User-Agent", "codex_cli_rs/s02-stream-test"));
    }
    // Reuse the unchanged two-second raw client: a partial body/EOF timeout is
    // still a failure, and captures contain the actual received SSE.
    let (status, actual_sse) = post_http_raw(
        &server.addr,
        path,
        &serde_json::to_string(&request).expect("serialize stream HTTP request"),
        &headers,
    );
    server.join();
    upstream_join.join().expect("join stream HTTP mock");
    let captured = upstream_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(captured.len(), 1, "{case}: exactly one upstream request");
    let upstream_request: Value =
        serde_json::from_slice(&decode_upstream_request_body(&captured[0]))
            .expect("parse actual stream upstream request");
    if let Some(root) = std::env::var_os("CM_REPAIR_FIXTURE_DIR") {
        let root = PathBuf::from(root).join("stream-http");
        fs::create_dir_all(&root).expect("create stream HTTP capture directory");
        fs::write(
            root.join(format!("{case}.json")),
            serde_json::to_vec_pretty(&json!({
                "case": case, "path": path, "request": request, "status": status,
                "route_kind": if converted_chat {"account_pool"} else {"aggregate_api"},
                "upstream_path": captured[0].path, "upstream_request": upstream_request,
                "upstream_sse": upstream_sse, "actual_sse": actual_sse, "expected": expected
            }))
            .expect("serialize actual stream HTTP capture"),
        )
        .expect("write actual stream HTTP capture");
    }
    assert_eq!(
        status, 200,
        "{case}: gateway returns a complete SSE success"
    );
    assert_eq!(captured[0].path, "/backend-api/codex/responses");
    assert_eq!(upstream_request["model"], UPSTREAM_MODEL);
    assert_eq!(upstream_request["stream"], true);
    assert_eq!(upstream_request["input"], input);
    assert!(upstream_request.get("messages").is_none());
    assert_eq!(upstream_request["tools"], tools);
    actual_sse
}

fn stream_http_events(body: &str) -> Vec<Value> {
    assert!(body.ends_with("\n\n"), "complete final SSE frame: {body}");
    let data = body
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .collect::<Vec<_>>();
    assert_eq!(
        data.last().copied(),
        Some("[DONE]"),
        "final terminal marker"
    );
    assert_eq!(data.iter().filter(|data| **data == "[DONE]").count(), 1);
    data[..data.len() - 1]
        .iter()
        .map(|data| {
            let event: Value = serde_json::from_str(data).expect("complete SSE data JSON");
            assert!(event.get("error").is_none(), "no SSE error frame: {event}");
            assert!(
                event.pointer("/response/error").is_none(),
                "no response error: {event}"
            );
            event
        })
        .collect()
}

#[test]
fn stream_http_native_responses_function_tools() {
    let _lock = test_env_guard();
    let (response, expected_events, upstream_sse) = stream_http_tools_fixture();
    let actual = request_stream_tools_through_gateway(
        "stream_http_native_responses_function_tools",
        false,
        &upstream_sse,
        json!({
            "tools": stream_http_expected_tools(), "text": "Before. Between. After.",
            "reasoning": "Think. ", "usage": response["usage"], "response": response
        }),
    );
    assert_eq!(
        actual,
        upstream_sse
            .strip_suffix("data: [DONE]\n\n")
            .expect("fixture contains an optional post-completion marker"),
        "native Responses preserves all bytes through response.completed"
    );
    // Native Responses ends at completed. The Chat helper below deliberately
    // keeps its separate [DONE] contract.
    assert!(
        actual.ends_with("\n\n"),
        "complete final SSE frame: {actual}"
    );
    let events = actual
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| {
            serde_json::from_str::<Value>(data.trim()).expect("complete Responses SSE JSON")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events, expected_events,
        "native HTTP preserves every ordered SSE event"
    );
    let completed = events.last().expect("completed response event");
    assert_eq!(completed["type"], "response.completed");
    assert_eq!(completed["response"], response);
    assert_eq!(completed["response"]["status"], "completed");
    assert_eq!(completed["response"]["usage"]["total_tokens"], 12);
}

#[test]
fn stream_http_account_chat_function_tools() {
    let _lock = test_env_guard();
    let (_, _, upstream_sse) = stream_http_tools_fixture();
    let expected_tools = stream_http_expected_tools();
    let expected_usage = json!({"prompt_tokens": 7, "completion_tokens": 5, "total_tokens": 12});
    let actual = request_stream_tools_through_gateway(
        "stream_http_account_chat_function_tools",
        true,
        &upstream_sse,
        json!({
            "tools": expected_tools, "text": "Before. Between. After.",
            "reasoning": "Think. ", "usage": expected_usage, "finish_reason": "tool_calls"
        }),
    );
    let events = stream_http_events(&actual);
    let mut tools = Vec::new();
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut reasoning_content = String::new();
    let mut roles = Vec::new();
    let mut finishes = Vec::new();
    let mut usages = Vec::new();
    for (position, event) in events.iter().enumerate() {
        assert_eq!(event["object"], "chat.completion.chunk");
        assert_eq!(event["id"], "resp_non_stream_tools");
        assert_eq!(event["model"], UPSTREAM_MODEL);
        let choices = event["choices"].as_array().expect("Chat choices");
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0]["index"], 0);
        let delta = &choices[0]["delta"];
        for (field, target) in [
            ("content", &mut text),
            ("reasoning", &mut reasoning),
            ("reasoning_content", &mut reasoning_content),
        ] {
            if let Some(fragment) = delta.get(field) {
                target.push_str(fragment.as_str().expect("text fragment"));
            }
        }
        if let Some(role) = delta.get("role") {
            roles.push(role.clone());
        }
        if let Some(deltas) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool in deltas {
                let index = tool["index"].as_u64().expect("tool index") as usize;
                assert!(index <= tools.len(), "new tools append one contiguous slot");
                if index == tools.len() {
                    assert_eq!(tool["type"], "function");
                    tools.push(json!({"id": "", "type": "function", "function": {"name": "", "arguments": ""}}));
                }
                assert!(
                    tool["function"].is_object(),
                    "metadata-only deltas retain function"
                );
                for field in ["/id", "/function/name", "/function/arguments"] {
                    if let Some(fragment) = tool.pointer(field) {
                        let target = tools[index].pointer_mut(field).unwrap();
                        *target = json!(format!(
                            "{}{}",
                            target.as_str().unwrap(),
                            fragment.as_str().expect("tool fragment")
                        ));
                    }
                }
            }
        }
        if !choices[0]["finish_reason"].is_null() {
            finishes.push((position, choices[0]["finish_reason"].clone()));
        }
        if let Some(usage) = event.get("usage") {
            usages.push((position, usage.clone()));
        }
    }
    assert_eq!(
        json!(tools),
        expected_tools,
        "IDs, names, exact arguments and tool order"
    );
    assert_eq!(text, "Before. Between. After.");
    assert_eq!(reasoning, "Think. ");
    assert_eq!(reasoning_content, "Think. ");
    assert_eq!(roles, vec![json!("assistant")]);
    assert_eq!(finishes, vec![(events.len() - 1, json!("tool_calls"))]);
    assert_eq!(usages, vec![(events.len() - 1, expected_usage)]);
    assert_eq!(events.last().unwrap()["choices"][0]["delta"], json!({}));
}
